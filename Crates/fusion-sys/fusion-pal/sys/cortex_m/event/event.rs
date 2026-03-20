//! Cortex-M bare-metal event backend.
//!
//! This backend still does not pretend NVIC is a backend-neutral readiness reactor. What it can
//! surface honestly is:
//! - a small local completion queue for software-submitted completions, and
//! - level-triggered readiness over registered NVIC IRQ lines for the selected board.
//!
//! That is enough to be a hybrid backend without inventing a kernel where there isn't one.

use core::arch::asm;
use core::mem::MaybeUninit;
use core::ptr;

use crate::pal::event::{
    EventBase, EventCaps, EventCompletion, EventCompletionOp, EventError, EventInterest, EventKey,
    EventModel, EventNotification, EventReadiness, EventRecord, EventRegistration,
    EventRegistrationMode, EventSource, EventSourceHandle, EventSupport,
};

const CORTEX_M_EVENT_QUEUE_CAPACITY: usize = 64;
const CORTEX_M_IRQ_REGISTRATION_CAPACITY: usize = 64;
const CORTEX_M_NVIC_ISPR: *const u32 = 0xE000_E200 as *const u32;

const CORTEX_M_EVENT_SUPPORT: EventSupport = EventSupport {
    caps: EventCaps::READINESS
        .union(EventCaps::COMPLETION)
        .union(EventCaps::SUBMIT)
        .union(EventCaps::LEVEL_TRIGGERED)
        .union(EventCaps::ONESHOT)
        .union(EventCaps::TIMEOUT),
    model: EventModel::Hybrid,
    max_events: Some(CORTEX_M_EVENT_QUEUE_CAPACITY + CORTEX_M_IRQ_REGISTRATION_CAPACITY),
    implementation: crate::pal::event::EventImplementationKind::Emulated,
};

#[derive(Debug, Clone, Copy)]
struct PendingCompletion {
    key: EventKey,
    operation: EventCompletionOp,
}

#[derive(Debug, Clone, Copy)]
struct RegisteredIrq {
    key: EventKey,
    irqn: u16,
    interest: EventInterest,
    mode: EventRegistrationMode,
}

/// Cortex-M event provider type.
#[derive(Debug, Clone, Copy, Default)]
pub struct CortexMEvent;

/// Small deterministic Cortex-M completion poller.
#[derive(Debug)]
pub struct CortexMPoller {
    queue: [MaybeUninit<PendingCompletion>; CORTEX_M_EVENT_QUEUE_CAPACITY],
    head: usize,
    len: usize,
    registrations: [Option<RegisteredIrq>; CORTEX_M_IRQ_REGISTRATION_CAPACITY],
    next_key: u64,
    timeout_armed: bool,
}

/// Selected Cortex-M event provider type.
pub type PlatformEvent = CortexMEvent;
/// Selected Cortex-M poller type.
pub type PlatformPoller = CortexMPoller;

/// Returns the selected Cortex-M event provider.
#[must_use]
pub const fn system_event() -> PlatformEvent {
    PlatformEvent::new()
}

impl CortexMEvent {
    /// Creates a new Cortex-M event provider handle.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl CortexMPoller {
    #[must_use]
    const fn new() -> Self {
        Self {
            queue: [MaybeUninit::uninit(); CORTEX_M_EVENT_QUEUE_CAPACITY],
            head: 0,
            len: 0,
            registrations: [None; CORTEX_M_IRQ_REGISTRATION_CAPACITY],
            next_key: 1,
            timeout_armed: false,
        }
    }

    fn next_key(&mut self) -> EventKey {
        let key = EventKey(self.next_key);
        self.next_key = self.next_key.wrapping_add(1).max(1);
        key
    }

    fn push(&mut self, operation: EventCompletionOp) -> Result<EventKey, EventError> {
        if self.len == CORTEX_M_EVENT_QUEUE_CAPACITY {
            return Err(EventError::resource_exhausted());
        }

        let key = self.next_key();
        let tail = (self.head + self.len) % CORTEX_M_EVENT_QUEUE_CAPACITY;
        self.queue[tail].write(PendingCompletion { key, operation });
        self.len += 1;
        Ok(key)
    }

    #[must_use]
    const fn pop(&mut self) -> Option<PendingCompletion> {
        if self.len == 0 {
            return None;
        }

        let index = self.head;
        self.head = (self.head + 1) % CORTEX_M_EVENT_QUEUE_CAPACITY;
        self.len -= 1;
        // SAFETY: this slot was initialized by `push`, and `head/len` ensure it is popped once.
        Some(unsafe { self.queue[index].assume_init_read() })
    }

    fn register_irq(&mut self, registration: EventRegistration) -> Result<EventKey, EventError> {
        if registration.interest != EventInterest::READABLE {
            return Err(EventError::unsupported());
        }

        let irqn = u16::try_from(registration.source.0).map_err(|_| EventError::invalid())?;
        if !irq_is_known(irqn) {
            return Err(EventError::invalid());
        }
        if reserved_timeout_irq() == Some(irqn) {
            return Err(EventError::state_conflict());
        }
        if self
            .registrations
            .iter()
            .flatten()
            .any(|registered| registered.irqn == irqn)
        {
            return Err(EventError::invalid());
        }

        let slot_index = self
            .registrations
            .iter_mut()
            .position(|entry| entry.is_none())
            .ok_or_else(EventError::resource_exhausted)?;

        validate_registration_mode(irqn, registration.mode)?;
        board_irq_enable(irqn)?;

        let key = self.next_key();
        self.registrations[slot_index] = Some(RegisteredIrq {
            key,
            irqn,
            interest: registration.interest,
            mode: registration.mode,
        });
        Ok(key)
    }

    fn reregister_irq(
        &mut self,
        key: EventKey,
        registration: EventRegistration,
    ) -> Result<(), EventError> {
        if registration.interest != EventInterest::READABLE {
            return Err(EventError::unsupported());
        }

        let registered = self
            .registrations
            .iter_mut()
            .find(|entry| entry.is_some_and(|registered| registered.key == key))
            .and_then(Option::as_mut)
            .ok_or_else(EventError::invalid)?;
        validate_registration_mode(registered.irqn, registration.mode)?;
        registered.interest = registration.interest;
        registered.mode = registration.mode;
        Ok(())
    }

    fn deregister_irq(&mut self, key: EventKey) -> Result<(), EventError> {
        let slot = self
            .registrations
            .iter_mut()
            .find(|entry| entry.is_some_and(|registered| registered.key == key))
            .ok_or_else(EventError::invalid)?;
        if let Some(registered) = slot.as_ref() {
            board_irq_disable(registered.irqn)?;
        }
        *slot = None;
        Ok(())
    }

    fn write_ready_irqs(
        &mut self,
        events: &mut [EventRecord],
        start: usize,
    ) -> Result<usize, EventError> {
        let mut written = start;
        for slot in &mut self.registrations {
            if written >= events.len() {
                break;
            }
            let Some(registered) = slot.as_mut() else {
                continue;
            };
            if !registered.interest.contains(EventInterest::READABLE) {
                continue;
            }
            if !irq_is_pending(registered.irqn) {
                continue;
            }

            match registered.mode {
                EventRegistrationMode::LevelSticky => {}
                EventRegistrationMode::LevelAckOnPoll => {
                    board_irq_acknowledge(registered.irqn)?;
                }
                EventRegistrationMode::OneShot => {
                    board_irq_disable(registered.irqn)?;
                    board_irq_acknowledge(registered.irqn)?;
                }
            }

            events[written] = EventRecord {
                key: registered.key,
                notification: EventNotification::Readiness(EventReadiness::READABLE),
            };
            written += 1;

            if registered.mode == EventRegistrationMode::OneShot {
                *slot = None;
            }
        }

        Ok(written)
    }

    #[must_use]
    fn has_registered_irqs(&self) -> bool {
        self.registrations.iter().any(Option::is_some)
    }

    fn has_wait_sources(&self) -> bool {
        self.has_registered_irqs() || self.timeout_armed
    }
}

impl EventBase for CortexMEvent {
    type Poller = CortexMPoller;

    fn support(&self) -> EventSupport {
        CORTEX_M_EVENT_SUPPORT
    }
}

impl EventSource for CortexMEvent {
    fn create(&self) -> Result<Self::Poller, EventError> {
        Ok(CortexMPoller::new())
    }

    fn register(
        &self,
        poller: &mut Self::Poller,
        source: EventSourceHandle,
        interest: EventInterest,
    ) -> Result<EventKey, EventError> {
        poller.register_irq(EventRegistration {
            source,
            interest,
            mode: EventRegistrationMode::LevelSticky,
        })
    }

    fn register_with(
        &self,
        poller: &mut Self::Poller,
        registration: EventRegistration,
    ) -> Result<EventKey, EventError> {
        poller.register_irq(registration)
    }

    fn reregister(
        &self,
        poller: &mut Self::Poller,
        key: EventKey,
        interest: EventInterest,
    ) -> Result<(), EventError> {
        poller.reregister_irq(
            key,
            EventRegistration {
                source: EventSourceHandle(0),
                interest,
                mode: EventRegistrationMode::LevelSticky,
            },
        )
    }

    fn reregister_with(
        &self,
        poller: &mut Self::Poller,
        key: EventKey,
        registration: EventRegistration,
    ) -> Result<(), EventError> {
        poller.reregister_irq(key, registration)
    }

    fn deregister(&self, poller: &mut Self::Poller, key: EventKey) -> Result<(), EventError> {
        poller.deregister_irq(key)
    }

    fn submit(
        &self,
        poller: &mut Self::Poller,
        operation: EventCompletionOp,
    ) -> Result<EventKey, EventError> {
        poller.push(operation)
    }

    fn poll(
        &self,
        poller: &mut Self::Poller,
        events: &mut [EventRecord],
        timeout: Option<core::time::Duration>,
    ) -> Result<usize, EventError> {
        if events.is_empty() {
            return Err(EventError::invalid());
        }

        let mut written = 0;
        while written < events.len() {
            let Some(completion) = poller.pop() else {
                break;
            };
            let _ = completion.operation;
            events[written] = EventRecord {
                key: completion.key,
                notification: EventNotification::Completion(EventCompletion {
                    bytes_transferred: None,
                    success: true,
                }),
            };
            written += 1;
        }

        written = poller.write_ready_irqs(events, written)?;
        if written != 0 {
            return Ok(written);
        }

        match timeout {
            Some(duration) if !duration.is_zero() => {
                arm_board_timeout(duration)?;
                poller.timeout_armed = true;
                wait_for_interrupt();
                let timed_out = board_timeout_fired()?;
                cancel_board_timeout()?;
                poller.timeout_armed = false;
                let written = poller.write_ready_irqs(events, 0)?;
                if written != 0 {
                    return Ok(written);
                }
                if timed_out {
                    return Ok(0);
                }
                Ok(poller.write_ready_irqs(events, 0)?)
            }
            None if poller.has_wait_sources() => {
                wait_for_interrupt();
                Ok(poller.write_ready_irqs(events, 0)?)
            }
            Some(_) | None => Ok(0),
        }
    }
}

fn irq_is_known(irqn: u16) -> bool {
    super::super::hal::soc::board::irqs()
        .iter()
        .any(|descriptor| descriptor.irqn == irqn)
}

fn reserved_timeout_irq() -> Option<u16> {
    super::super::hal::soc::board::event_timeout_irq()
}

fn irq_is_pending(irqn: u16) -> bool {
    let register_index = usize::from(irqn / 32);
    let bit = u32::from(irqn % 32);
    // SAFETY: NVIC ISPR is the architected Cortex-M interrupt-pending register block. Reading it
    // is side-effect free and does not require mutable access.
    let pending = unsafe { ptr::read_volatile(CORTEX_M_NVIC_ISPR.add(register_index)) };
    (pending & (1_u32 << bit)) != 0
}

fn validate_registration_mode(irqn: u16, mode: EventRegistrationMode) -> Result<(), EventError> {
    match mode {
        EventRegistrationMode::LevelSticky => Ok(()),
        EventRegistrationMode::LevelAckOnPoll | EventRegistrationMode::OneShot => {
            if super::super::hal::soc::board::irq_acknowledge_supported(irqn) {
                Ok(())
            } else {
                Err(EventError::unsupported())
            }
        }
    }
}

fn board_irq_enable(irqn: u16) -> Result<(), EventError> {
    super::super::hal::soc::board::irq_enable(irqn).map_err(map_hardware_error)
}

fn board_irq_disable(irqn: u16) -> Result<(), EventError> {
    super::super::hal::soc::board::irq_disable(irqn).map_err(map_hardware_error)
}

fn board_irq_acknowledge(irqn: u16) -> Result<(), EventError> {
    super::super::hal::soc::board::irq_acknowledge(irqn).map_err(map_hardware_error)
}

fn arm_board_timeout(timeout: core::time::Duration) -> Result<(), EventError> {
    if !super::super::hal::soc::board::event_timeout_supported() {
        return Err(EventError::unsupported());
    }

    super::super::hal::soc::board::arm_event_timeout(timeout).map_err(map_hardware_error)
}

fn cancel_board_timeout() -> Result<(), EventError> {
    if !super::super::hal::soc::board::event_timeout_supported() {
        return Ok(());
    }

    super::super::hal::soc::board::cancel_event_timeout().map_err(map_hardware_error)
}

fn board_timeout_fired() -> Result<bool, EventError> {
    if !super::super::hal::soc::board::event_timeout_supported() {
        return Ok(false);
    }

    super::super::hal::soc::board::event_timeout_fired().map_err(map_hardware_error)
}

const fn map_hardware_error(error: crate::pal::hal::HardwareError) -> EventError {
    use crate::pal::hal::HardwareErrorKind;

    match error.kind() {
        HardwareErrorKind::Unsupported => EventError::unsupported(),
        HardwareErrorKind::Invalid => EventError::invalid(),
        HardwareErrorKind::Busy => EventError::busy(),
        HardwareErrorKind::ResourceExhausted => EventError::resource_exhausted(),
        HardwareErrorKind::StateConflict => EventError::state_conflict(),
        HardwareErrorKind::Platform(code) => EventError::platform(code),
    }
}

fn wait_for_interrupt() {
    // SAFETY: `WFI` is the architected Cortex-M wait-for-interrupt instruction. This backend
    // uses it only when callers explicitly pass an infinite wait (`timeout = None`) and have
    // registered at least one IRQ-backed readiness source.
    unsafe {
        asm!(
            "dsb",
            "wfi",
            "isb",
            options(nomem, nostack, preserves_flags)
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pal::event::{EventCompletionOpKind, EventSource};

    #[test]
    fn cortex_m_event_support_reports_completion_queue() {
        let support = CortexMEvent::new().support();
        assert_eq!(support.model, EventModel::Hybrid);
        assert!(support.caps.contains(EventCaps::READINESS));
        assert!(support.caps.contains(EventCaps::COMPLETION));
        assert!(support.caps.contains(EventCaps::SUBMIT));
        assert!(support.caps.contains(EventCaps::LEVEL_TRIGGERED));
        assert!(support.caps.contains(EventCaps::ONESHOT));
        assert!(support.caps.contains(EventCaps::TIMEOUT));
    }

    #[test]
    fn cortex_m_event_queue_submits_and_drains() {
        let event = CortexMEvent::new();
        let mut poller = event.create().expect("poller should be creatable");
        let key = event
            .submit(
                &mut poller,
                EventCompletionOp {
                    source: EventSourceHandle(7),
                    kind: EventCompletionOpKind::Custom(11),
                    user_data: 99,
                },
            )
            .expect("submission should queue");

        let mut records = [EventRecord {
            key: EventKey(0),
            notification: EventNotification::Completion(EventCompletion {
                bytes_transferred: None,
                success: false,
            }),
        }; 1];

        let written = event
            .poll(&mut poller, &mut records, None)
            .expect("queued completion should drain");
        assert_eq!(written, 1);
        assert_eq!(records[0].key, key);
        assert_eq!(
            records[0].notification,
            EventNotification::Completion(EventCompletion {
                bytes_transferred: None,
                success: true,
            })
        );
    }
}
