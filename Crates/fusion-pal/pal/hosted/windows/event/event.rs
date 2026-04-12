//! Windows fusion-pal event backend built on IO completion ports.
//!
//! Windows does not have a truthful generic readiness reactor analogous to `epoll` or `kqueue`.
//! The honest hosted event surface here is therefore completion-oriented: an IOCP poller that can
//! associate overlapped-capable handles and accept explicit manual completion submissions.

use core::ffi::c_void;
use core::ptr;
use core::time::Duration;

use std::collections::HashMap;

use windows::Win32::Foundation::{
    CloseHandle,
    ERROR_ACCESS_DENIED,
    ERROR_INVALID_HANDLE,
    ERROR_INVALID_PARAMETER,
    ERROR_NOT_ENOUGH_MEMORY,
    ERROR_OUTOFMEMORY,
    ERROR_TIMEOUT,
    HANDLE,
    INVALID_HANDLE_VALUE,
    WIN32_ERROR,
};
use windows::Win32::System::IO::{
    CreateIoCompletionPort,
    GetQueuedCompletionStatus,
    OVERLAPPED,
    PostQueuedCompletionStatus,
};
use windows::Win32::System::Threading::INFINITE;

use crate::contract::pal::runtime::event::{
    EventBaseContract,
    EventCaps,
    EventCompletion,
    EventCompletionOp,
    EventError,
    EventErrorKind,
    EventInterest,
    EventKey,
    EventModel,
    EventNotification,
    EventRecord,
    EventSourceContract,
    EventSourceHandle,
    EventSupport,
};

const WINDOWS_EVENT_SUPPORT: EventSupport = EventSupport {
    caps: EventCaps::COMPLETION
        .union(EventCaps::SUBMIT)
        .union(EventCaps::TIMEOUT),
    model: EventModel::Completion,
    max_events: None,
    implementation: crate::contract::pal::runtime::event::EventImplementationKind::Native,
};

/// Windows completion-poller provider.
#[derive(Debug, Clone, Copy, Default)]
pub struct WindowsEvent;

/// Windows owned IO completion port.
#[derive(Debug)]
pub struct WindowsPoller {
    port: HANDLE,
    registrations: HashMap<EventKey, EventSourceHandle>,
}

/// Selected Windows event provider type.
pub type PlatformEvent = WindowsEvent;
/// Selected Windows poller type.
pub type PlatformPoller = WindowsPoller;

/// Returns the selected Windows event provider.
#[must_use]
pub const fn system_event() -> PlatformEvent {
    PlatformEvent::new()
}

impl WindowsEvent {
    /// Creates a new Windows event provider handle.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl EventBaseContract for WindowsEvent {
    type Poller = WindowsPoller;

    fn support(&self) -> EventSupport {
        WINDOWS_EVENT_SUPPORT
    }
}

impl EventSourceContract for WindowsEvent {
    fn create(&self) -> Result<Self::Poller, EventError> {
        let port = unsafe { CreateIoCompletionPort(INVALID_HANDLE_VALUE, None, 0, 0) }
            .map_err(|error| map_hresult(error.code().0))?;
        Ok(WindowsPoller {
            port,
            registrations: HashMap::new(),
        })
    }

    fn register(
        &self,
        poller: &mut Self::Poller,
        source: EventSourceHandle,
        _interest: EventInterest,
    ) -> Result<EventKey, EventError> {
        if source.0 == 0 {
            return Err(EventError::invalid());
        }

        let key = EventKey(source.0 as u64);
        if poller.registrations.contains_key(&key) {
            return Err(EventError::state_conflict());
        }

        let raw = HANDLE(source.0 as *mut c_void);
        let associated =
            unsafe { CreateIoCompletionPort(raw, Some(poller.port), key.0 as usize, 0) }
                .map_err(|error| map_hresult(error.code().0))?;
        if associated != poller.port {
            return Err(EventError::state_conflict());
        }

        poller.registrations.insert(key, source);
        Ok(key)
    }

    fn reregister(
        &self,
        poller: &mut Self::Poller,
        key: EventKey,
        _interest: EventInterest,
    ) -> Result<(), EventError> {
        if poller.registrations.contains_key(&key) {
            Ok(())
        } else {
            Err(EventError::state_conflict())
        }
    }

    fn deregister(&self, poller: &mut Self::Poller, key: EventKey) -> Result<(), EventError> {
        if poller.registrations.contains_key(&key) {
            // IOCP association is one-way for a bound handle; Win32 does not provide a truthful
            // disassociate operation, so valid deregistration stays unsupported on this backend.
            Err(EventError::unsupported())
        } else {
            Err(EventError::state_conflict())
        }
    }

    fn submit(
        &self,
        poller: &mut Self::Poller,
        operation: EventCompletionOp,
    ) -> Result<EventKey, EventError> {
        let key = EventKey(operation.user_data as u64);
        unsafe { PostQueuedCompletionStatus(poller.port, 0, key.0 as usize, None) }
            .map_err(|error| map_hresult(error.code().0))?;
        Ok(key)
    }

    fn poll(
        &self,
        poller: &mut Self::Poller,
        events: &mut [EventRecord],
        timeout: Option<Duration>,
    ) -> Result<usize, EventError> {
        if events.is_empty() {
            return Err(EventError::invalid());
        }

        let mut total = 0;
        let mut timeout_ms = timeout_to_ms(timeout)?;
        while total < events.len() {
            let mut transferred = 0_u32;
            let mut key = 0_usize;
            let mut overlapped: *mut OVERLAPPED = ptr::null_mut();
            let result = unsafe {
                GetQueuedCompletionStatus(
                    poller.port,
                    &mut transferred,
                    &mut key,
                    &mut overlapped,
                    timeout_ms,
                )
            };

            match result {
                Ok(()) => {
                    events[total] = completion_record(key, transferred, true);
                    total += 1;
                }
                Err(error) => {
                    if !overlapped.is_null() {
                        events[total] = completion_record(key, transferred, false);
                        total += 1;
                    } else {
                        let mapped = map_hresult(error.code().0);
                        if mapped.kind() == EventErrorKind::Timeout {
                            return Ok(total);
                        }
                        return Err(mapped);
                    }
                }
            }

            timeout_ms = 0;
        }

        Ok(total)
    }
}

impl Drop for WindowsPoller {
    fn drop(&mut self) {
        if !self.port.is_invalid() {
            let rc = unsafe { CloseHandle(self.port) };
            debug_assert!(rc.is_ok());
        }
    }
}

fn completion_record(key: usize, transferred: u32, success: bool) -> EventRecord {
    EventRecord {
        key: EventKey(key as u64),
        notification: EventNotification::Completion(EventCompletion {
            bytes_transferred: Some(transferred as usize),
            success,
        }),
    }
}

fn timeout_to_ms(timeout: Option<Duration>) -> Result<u32, EventError> {
    match timeout {
        None => Ok(INFINITE),
        Some(duration) => {
            let millis = duration.as_millis();
            let rounded = if millis == 0 && duration.subsec_nanos() != 0 {
                1
            } else {
                millis
            };
            u32::try_from(rounded).map_err(|_| EventError::invalid())
        }
    }
}

const fn map_win32_error(error: WIN32_ERROR) -> EventError {
    match error {
        ERROR_INVALID_PARAMETER => EventError::invalid(),
        ERROR_INVALID_HANDLE => EventError::state_conflict(),
        ERROR_TIMEOUT => EventError::timeout(),
        ERROR_ACCESS_DENIED => EventError::state_conflict(),
        ERROR_NOT_ENOUGH_MEMORY | ERROR_OUTOFMEMORY => EventError::resource_exhausted(),
        _ => EventError::platform(error.0 as i32),
    }
}

const fn map_hresult(code: i32) -> EventError {
    let raw = code as u32;
    let facility = (raw >> 16) & 0x1fff;
    if facility == 7 {
        return map_win32_error(WIN32_ERROR(raw & 0xffff));
    }
    EventError::platform(code)
}

#[cfg(all(test, feature = "std", target_os = "windows"))]
mod tests {
    use std::time::Duration;

    use super::system_event;
    use crate::contract::pal::runtime::event::{
        EventBaseContract,
        EventCaps,
        EventCompletionOp,
        EventCompletionOpKind,
        EventModel,
        EventSourceContract,
    };

    #[test]
    fn windows_event_support_reports_completion_model() {
        let support = system_event().support();

        assert_eq!(support.model, EventModel::Completion);
        assert!(
            support
                .caps
                .contains(EventCaps::COMPLETION | EventCaps::SUBMIT | EventCaps::TIMEOUT)
        );
    }

    #[test]
    fn windows_event_manual_completion_can_be_created_and_polled() {
        let provider = system_event();
        let mut poller = provider.create().expect("poller should be created");
        let key = provider
            .submit(
                &mut poller,
                EventCompletionOp {
                    source: crate::contract::pal::runtime::event::EventSourceHandle(0),
                    kind: EventCompletionOpKind::Custom(1),
                    user_data: 0x55AA,
                },
            )
            .expect("manual completion should post");

        let mut events = [crate::contract::pal::runtime::event::EventRecord {
            key: crate::contract::pal::runtime::event::EventKey(0),
            notification: crate::contract::pal::runtime::event::EventNotification::Completion(
                crate::contract::pal::runtime::event::EventCompletion {
                    bytes_transferred: None,
                    success: false,
                },
            ),
        }; 1];

        let ready = provider
            .poll(&mut poller, &mut events, Some(Duration::from_millis(1)))
            .expect("poll should succeed");

        assert_eq!(ready, 1);
        assert_eq!(events[0].key, key);
    }
}
