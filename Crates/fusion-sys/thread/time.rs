//! Monotonic runtime-time surface layered on top of the selected thread backend.
//!
//! This stays deliberately narrow: the runtime needs one truthful monotonic timebase and,
//! where available, one honest relative sleep primitive. Wall-clock time and richer timer/alarm
//! policy belong elsewhere.

use core::convert::TryFrom;
use core::sync::atomic::{
    AtomicU32,
    Ordering,
};
use core::time::Duration;

use bitflags::bitflags;

#[cfg(all(target_os = "none", feature = "sys-cortex-m"))]
use fusion_pal::sys::soc::cortex_m::hal::soc::board as cortex_m_soc_board;

use crate::event::EventRegistration;
#[cfg(feature = "sys-cortex-m")]
use crate::event::{
    EventInterest,
    EventRegistrationMode,
    cortex_m::CortexMIrqSource,
};
use super::{
    ThreadAuthoritySet,
    ThreadError,
    ThreadGuarantee,
    ThreadImplementationKind,
    ThreadSchedulerCaps,
    ThreadSupport,
    ThreadSystem,
};

#[cfg(not(feature = "sys-cortex-m"))]
const NANOS_PER_SECOND: u128 = 1_000_000_000;
#[cfg(feature = "sys-cortex-m")]
const NANOS_PER_SECOND_U64: u64 = 1_000_000_000;
const U32_WRAP_HALF_RANGE: u32 = u32::MAX / 2;

static MONOTONIC_EXTENDED32_HIGH: AtomicU32 = AtomicU32::new(0);
static MONOTONIC_EXTENDED32_LAST_LOW: AtomicU32 = AtomicU32::new(0);

bitflags! {
    /// Fine-grained monotonic runtime-time capabilities surfaced by the backend.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MonotonicRuntimeTimeCaps: u32 {
        /// Supports observing the current monotonic runtime timebase.
        const NOW                = 1 << 0;
        /// Supports relative sleep against the runtime timebase.
        const SLEEP_FOR          = 1 << 1;
        /// Supports sleeping until one canonical monotonic deadline.
        const SLEEP_UNTIL        = 1 << 2;
        /// Supports hot-path raw deadline comparison without widening to canonical space.
        const RAW_DEADLINE_COMPARE = 1 << 3;
        /// Exposes one board/runtime-visible one-shot timeout alarm source.
        const ONE_SHOT_ALARM     = 1 << 4;
    }
}

/// Public canonical monotonic instant.
///
/// This is an opaque widened counter value measured against one backend-defined monotonic origin.
/// Callers may compare instants from the same running system, but must not assign portable
/// wall-clock meaning to the numeric origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CanonicalInstant(u64);

impl CanonicalInstant {
    /// Returns the raw widened tick value.
    #[must_use]
    pub const fn raw(self) -> u64 {
        self.0
    }
}

/// Canonicalization strategy used to widen the selected monotonic timebase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MonotonicCanonicalization {
    /// One native 64-bit raw counter backs the monotonic surface directly.
    Native64,
    /// One 32-bit raw counter is widened into canonical `u64` space.
    Extended32,
}

/// Truthful deadline-wait source available to the monotonic runtime-time surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MonotonicDeadlineWaitKind {
    /// Deadlines are serviced by one reserved one-shot hardware alarm source.
    ReservedOneShotAlarm,
    /// Deadlines are serviced through relative sleep against the same monotonic timebase.
    RelativeSleep,
}

/// Deadline-wait support summary surfaced alongside the monotonic runtime-time contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MonotonicDeadlineWaitSupport {
    /// Backend implementation shape used for deadline waits.
    pub kind: MonotonicDeadlineWaitKind,
    /// Reserved IRQ line used by the deadline source when one exists.
    pub irqn: Option<u16>,
    /// Effective counter/compare width used by the deadline source in bits.
    pub counter_bits: Option<u32>,
    /// Effective deadline tick rate in ticks per second.
    pub tick_hz: Option<u64>,
    /// Maximum truthful relative wait the source can admit, when one finite bound exists.
    pub max_relative_timeout: Option<Duration>,
}

impl MonotonicDeadlineWaitSupport {
    /// Returns the typed Cortex-M IRQ source backing this deadline wait when one exists.
    #[cfg(feature = "sys-cortex-m")]
    #[must_use]
    pub const fn cortex_m_irq_source(self) -> Option<CortexMIrqSource> {
        match self.kind {
            MonotonicDeadlineWaitKind::ReservedOneShotAlarm => match self.irqn {
                Some(irqn) => Some(CortexMIrqSource::new(irqn)),
                None => None,
            },
            MonotonicDeadlineWaitKind::RelativeSleep => None,
        }
    }

    /// Returns one recommended event registration for this deadline-wait source when the platform
    /// can surface it honestly as one event source.
    #[must_use]
    pub const fn registration(self) -> Option<EventRegistration> {
        #[cfg(feature = "sys-cortex-m")]
        {
            if let Some(source) = self.cortex_m_irq_source() {
                return Some(source.registration(
                    EventInterest::READABLE,
                    EventRegistrationMode::LevelAckOnPoll,
                ));
            }
        }

        None
    }
}

/// Internal raw monotonic timestamp used on hot scheduler paths.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MonotonicRawInstant {
    /// Native or projected 32-bit raw counter value.
    Bits32(u32),
    /// Native or projected 64-bit raw counter value.
    Bits64(u64),
}

impl MonotonicRawInstant {
    /// Returns whether `deadline` has been reached according to the raw-counter semantics.
    #[must_use]
    pub const fn deadline_reached(self, deadline: Self) -> bool {
        match (self, deadline) {
            (Self::Bits32(now), Self::Bits32(deadline)) => {
                deadline_reached_wrapping_u32(now, deadline)
            }
            (Self::Bits64(now), Self::Bits64(deadline)) => now >= deadline,
            _ => false,
        }
    }

    /// Returns whether this raw instant can compare honestly against `deadline` with wrapping math.
    #[must_use]
    pub const fn can_compare_deadline(self, deadline: Self) -> bool {
        match (self, deadline) {
            (Self::Bits32(now), Self::Bits32(deadline)) => {
                deadline.wrapping_sub(now) <= U32_WRAP_HALF_RANGE
            }
            (Self::Bits64(_), Self::Bits64(_)) => true,
            _ => false,
        }
    }
}

/// Truthful monotonic runtime-time support snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MonotonicRuntimeTimeSupport {
    /// Fine-grained runtime-time capability flags.
    pub caps: MonotonicRuntimeTimeCaps,
    /// Strength of observing the effective monotonic runtime timebase.
    pub observation: ThreadGuarantee,
    /// Evidence sources used to justify the runtime-time surface.
    pub authorities: ThreadAuthoritySet,
    /// Whether the runtime-time surface is native, emulated, or unavailable.
    pub implementation: ThreadImplementationKind,
    /// Width in bits of the raw monotonic counter, when one exists.
    pub raw_bits: Option<u32>,
    /// Raw monotonic tick rate in ticks per second, when one exists.
    pub tick_hz: Option<u64>,
    /// Canonicalization strategy used to widen the raw monotonic surface, when one exists.
    pub canonicalization: Option<MonotonicCanonicalization>,
    /// Deadline-wait source surfaced by the backend when one truthful source exists.
    pub deadline_wait: Option<MonotonicDeadlineWaitSupport>,
}

impl MonotonicRuntimeTimeSupport {
    /// Returns an explicitly unsupported monotonic runtime-time surface.
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            caps: MonotonicRuntimeTimeCaps::empty(),
            observation: ThreadGuarantee::Unsupported,
            authorities: ThreadAuthoritySet::empty(),
            implementation: ThreadImplementationKind::Unsupported,
            raw_bits: None,
            tick_hz: None,
            canonicalization: None,
            deadline_wait: None,
        }
    }

    /// Builds one monotonic runtime-time view from the selected thread support surface.
    #[must_use]
    pub fn from_thread_support(support: ThreadSupport) -> Self {
        let scheduler = support.scheduler;
        let mut caps = MonotonicRuntimeTimeCaps::empty();
        if scheduler.caps.contains(ThreadSchedulerCaps::MONOTONIC_NOW) {
            caps |= MonotonicRuntimeTimeCaps::NOW;
        }
        if scheduler.caps.contains(ThreadSchedulerCaps::SLEEP_FOR) {
            caps |= MonotonicRuntimeTimeCaps::SLEEP_FOR;
        }
        let raw_bits = monotonic_raw_bits_for_selected_backend(caps);
        let tick_hz = monotonic_tick_hz_for_selected_backend(caps);
        let canonicalization = raw_bits.map(|bits| {
            if bits >= 64 {
                MonotonicCanonicalization::Native64
            } else {
                MonotonicCanonicalization::Extended32
            }
        });
        let deadline_wait = deadline_wait_support_for_selected_backend(caps);
        if deadline_wait.is_some() {
            caps |= MonotonicRuntimeTimeCaps::SLEEP_UNTIL;
        } else if caps.contains(MonotonicRuntimeTimeCaps::NOW)
            && caps.contains(MonotonicRuntimeTimeCaps::SLEEP_FOR)
        {
            caps |= MonotonicRuntimeTimeCaps::SLEEP_UNTIL;
        }
        if deadline_wait.is_some_and(|support| {
            matches!(
                support.kind,
                MonotonicDeadlineWaitKind::ReservedOneShotAlarm
            )
        }) {
            caps |= MonotonicRuntimeTimeCaps::ONE_SHOT_ALARM;
        }
        if raw_bits.is_some() {
            caps |= MonotonicRuntimeTimeCaps::RAW_DEADLINE_COMPARE;
        }

        Self {
            caps,
            observation: if caps.contains(MonotonicRuntimeTimeCaps::NOW) {
                scheduler.observation
            } else {
                ThreadGuarantee::Unsupported
            },
            authorities: scheduler.authorities,
            implementation: scheduler.implementation,
            raw_bits,
            tick_hz,
            canonicalization,
            deadline_wait,
        }
    }

    /// Returns one recommended event registration for the selected deadline-wait source when the
    /// backend can surface that source honestly as one event.
    #[must_use]
    pub const fn deadline_wait_registration(self) -> Option<EventRegistration> {
        match self.deadline_wait {
            Some(deadline_wait) => deadline_wait.registration(),
            None => None,
        }
    }
}

/// Thin wrapper for the selected backend's monotonic runtime-time surface.
#[derive(Debug, Clone, Copy)]
pub struct MonotonicRuntimeTime {
    inner: ThreadSystem,
}

impl MonotonicRuntimeTime {
    /// Creates a wrapper for the selected platform monotonic runtime-time provider.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            inner: ThreadSystem::new(),
        }
    }

    /// Reports the supported monotonic runtime-time surface.
    #[must_use]
    pub fn support(&self) -> MonotonicRuntimeTimeSupport {
        MonotonicRuntimeTimeSupport::from_thread_support(self.inner.support())
    }

    /// Returns the current canonical monotonic instant.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot surface one truthful monotonic runtime timestamp.
    pub fn now_instant(&self) -> Result<CanonicalInstant, ThreadError> {
        match self.raw_now()? {
            MonotonicRawInstant::Bits64(raw) => Ok(CanonicalInstant(raw)),
            MonotonicRawInstant::Bits32(raw) => Ok(CanonicalInstant(extend_32_software(raw))),
        }
    }

    /// Returns the current monotonic runtime time.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot surface one truthful monotonic runtime timestamp.
    pub fn now(&self) -> Result<Duration, ThreadError> {
        let instant = self.now_instant()?;
        self.duration_from_instant(instant)
    }

    /// Sleeps for a relative duration against the monotonic runtime timebase.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot honestly sleep for the requested duration.
    pub fn sleep_for(&self, duration: Duration) -> Result<(), ThreadError> {
        if duration.is_zero() {
            return Ok(());
        }

        let max_chunk = self
            .support()
            .deadline_wait
            .and_then(|support| support.max_relative_timeout)
            .filter(|max| !max.is_zero());
        let Some(max_chunk) = max_chunk else {
            return self.inner.sleep_for(duration);
        };

        let mut remaining = duration;
        while let Some((chunk, next_remaining)) = next_relative_sleep_chunk(remaining, max_chunk) {
            self.inner.sleep_for(chunk)?;
            remaining = next_remaining;
        }
        Ok(())
    }

    /// Sleeps until one canonical monotonic deadline.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot surface one truthful monotonic timebase or cannot
    /// honestly sleep for the remaining relative duration.
    pub fn sleep_until(&self, deadline: CanonicalInstant) -> Result<(), ThreadError> {
        let remaining = self.duration_until(deadline)?;
        if remaining.is_zero() {
            return Ok(());
        }
        self.sleep_for(remaining)
    }

    /// Converts one backend-relative duration into a canonical monotonic instant.
    ///
    /// # Errors
    ///
    /// Returns an error if the conversion would overflow or the backend has no truthful timebase.
    pub fn instant_from_duration(
        &self,
        duration: Duration,
    ) -> Result<CanonicalInstant, ThreadError> {
        let tick_hz = self
            .support()
            .tick_hz
            .ok_or_else(ThreadError::unsupported)?;
        duration_to_ticks_ceil(duration, tick_hz).map(CanonicalInstant)
    }

    /// Converts one canonical monotonic instant into a backend-relative duration.
    ///
    /// # Errors
    ///
    /// Returns an error if the conversion would overflow or the backend has no truthful timebase.
    pub fn duration_from_instant(
        &self,
        instant: CanonicalInstant,
    ) -> Result<Duration, ThreadError> {
        let tick_hz = self
            .support()
            .tick_hz
            .ok_or_else(ThreadError::unsupported)?;
        ticks_to_duration_floor(instant.raw(), tick_hz)
    }

    /// Adds one duration to an existing canonical instant.
    ///
    /// # Errors
    ///
    /// Returns an error if the conversion or addition would overflow.
    pub fn checked_add_duration(
        &self,
        base: CanonicalInstant,
        duration: Duration,
    ) -> Result<CanonicalInstant, ThreadError> {
        let delta = self.instant_from_duration(duration)?.raw();
        let Some(sum) = base.raw().checked_add(delta) else {
            return Err(ThreadError::invalid());
        };
        Ok(CanonicalInstant(sum))
    }

    /// Returns the remaining duration until `deadline`, rounded up to avoid premature expiry.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot surface one truthful monotonic runtime timestamp or
    /// the conversion would overflow.
    pub fn duration_until(&self, deadline: CanonicalInstant) -> Result<Duration, ThreadError> {
        let now = self.now_instant()?;
        if deadline <= now {
            return Ok(Duration::ZERO);
        }
        let tick_hz = self
            .support()
            .tick_hz
            .ok_or_else(ThreadError::unsupported)?;
        ticks_to_duration_ceil(deadline.raw() - now.raw(), tick_hz)
    }

    /// Returns the current raw monotonic instant for hot internal scheduler paths.
    ///
    /// This stays hidden because raw-counter math is only honest when paired with the selected
    /// backend's width and comparison rules.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot surface one truthful monotonic runtime timestamp.
    #[doc(hidden)]
    pub fn raw_now(&self) -> Result<MonotonicRawInstant, ThreadError> {
        raw_now_for_selected_backend(self)
    }

    /// Converts one canonical instant back into the raw counter space used by the selected backend.
    #[doc(hidden)]
    #[must_use]
    pub fn raw_deadline_from_instant(
        &self,
        deadline: CanonicalInstant,
    ) -> Option<MonotonicRawInstant> {
        match self.support().raw_bits {
            Some(bits) if bits >= 64 => Some(MonotonicRawInstant::Bits64(deadline.raw())),
            Some(bits) if bits > 0 => Some(MonotonicRawInstant::Bits32(deadline.raw() as u32)),
            _ => None,
        }
    }

    /// Converts one canonical instant into the raw deadline space when the wrapping compare remains
    /// honest for the current scheduler horizon.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot surface the current raw timebase.
    #[doc(hidden)]
    pub fn raw_deadline_for_sleep(
        &self,
        deadline: CanonicalInstant,
    ) -> Result<Option<MonotonicRawInstant>, ThreadError> {
        let Some(raw_deadline) = self.raw_deadline_from_instant(deadline) else {
            return Ok(None);
        };
        let now = self.raw_now()?;
        Ok(now
            .can_compare_deadline(raw_deadline)
            .then_some(raw_deadline))
    }

    /// Returns the relative timeout that would honestly fit in the selected one-shot alarm source.
    ///
    /// `None` means either there is no one-shot alarm source or the requested deadline exceeds the
    /// finite timeout window surfaced by that source.
    #[doc(hidden)]
    pub fn one_shot_alarm_timeout_until(
        &self,
        deadline: CanonicalInstant,
    ) -> Result<Option<Duration>, ThreadError> {
        let Some(deadline_wait) = self.support().deadline_wait else {
            return Ok(None);
        };
        if !matches!(
            deadline_wait.kind,
            MonotonicDeadlineWaitKind::ReservedOneShotAlarm
        ) {
            return Ok(None);
        }
        let remaining = self.duration_until(deadline)?;
        Ok(one_shot_alarm_timeout_for_remaining(
            remaining,
            deadline_wait.max_relative_timeout,
        ))
    }

    /// Arms the selected one-shot alarm source until the requested canonical deadline when that
    /// deadline fits honestly inside the source's finite timeout window.
    ///
    /// Returns `Ok(false)` when no truthful one-shot alarm exists or the deadline exceeds the
    /// source's admitted timeout window.
    #[doc(hidden)]
    pub fn arm_one_shot_alarm_until(
        &self,
        deadline: CanonicalInstant,
    ) -> Result<bool, ThreadError> {
        let Some(timeout) = self.one_shot_alarm_timeout_until(deadline)? else {
            return Ok(false);
        };
        #[cfg(not(all(target_os = "none", feature = "sys-cortex-m")))]
        let _ = timeout;

        #[cfg(all(target_os = "none", feature = "sys-cortex-m"))]
        {
            cortex_m_soc_board::arm_event_timeout(timeout).map_err(map_hardware_thread_error)?;
            return Ok(true);
        }

        #[allow(unreachable_code)]
        Err(ThreadError::unsupported())
    }

    /// Cancels the selected one-shot alarm source.
    ///
    /// # Errors
    ///
    /// Returns an error when the selected backend does not expose one truthful one-shot alarm.
    #[doc(hidden)]
    pub fn cancel_one_shot_alarm(&self) -> Result<(), ThreadError> {
        if !self
            .support()
            .caps
            .contains(MonotonicRuntimeTimeCaps::ONE_SHOT_ALARM)
        {
            return Err(ThreadError::unsupported());
        }

        #[cfg(all(target_os = "none", feature = "sys-cortex-m"))]
        {
            return cortex_m_soc_board::cancel_event_timeout().map_err(map_hardware_thread_error);
        }

        #[allow(unreachable_code)]
        Err(ThreadError::unsupported())
    }

    /// Returns whether the selected one-shot alarm source has fired.
    ///
    /// # Errors
    ///
    /// Returns an error when the selected backend does not expose one truthful one-shot alarm.
    #[doc(hidden)]
    pub fn one_shot_alarm_fired(&self) -> Result<bool, ThreadError> {
        if !self
            .support()
            .caps
            .contains(MonotonicRuntimeTimeCaps::ONE_SHOT_ALARM)
        {
            return Err(ThreadError::unsupported());
        }

        #[cfg(all(target_os = "none", feature = "sys-cortex-m"))]
        {
            return cortex_m_soc_board::event_timeout_fired().map_err(map_hardware_thread_error);
        }

        #[allow(unreachable_code)]
        Err(ThreadError::unsupported())
    }
}

impl Default for MonotonicRuntimeTime {
    fn default() -> Self {
        Self::new()
    }
}

/// Returns the selected backend's monotonic runtime-time wrapper.
#[must_use]
pub const fn system_monotonic_time() -> MonotonicRuntimeTime {
    MonotonicRuntimeTime::new()
}

fn monotonic_raw_bits_for_selected_backend(caps: MonotonicRuntimeTimeCaps) -> Option<u32> {
    if !caps.contains(MonotonicRuntimeTimeCaps::NOW) {
        return None;
    }

    #[cfg(all(target_os = "none", feature = "sys-cortex-m"))]
    {
        if let Some(bits) = cortex_m_soc_board::monotonic_raw_bits() {
            return Some(bits);
        }
    }

    Some(64)
}

fn monotonic_tick_hz_for_selected_backend(caps: MonotonicRuntimeTimeCaps) -> Option<u64> {
    if !caps.contains(MonotonicRuntimeTimeCaps::NOW) {
        return None;
    }

    #[cfg(all(target_os = "none", feature = "sys-cortex-m"))]
    {
        if let Some(tick_hz) = cortex_m_soc_board::monotonic_tick_hz() {
            return Some(tick_hz);
        }
    }

    Some(1_000_000_000)
}

fn deadline_wait_support_for_selected_backend(
    caps: MonotonicRuntimeTimeCaps,
) -> Option<MonotonicDeadlineWaitSupport> {
    if !caps.contains(MonotonicRuntimeTimeCaps::NOW)
        || !caps.contains(MonotonicRuntimeTimeCaps::SLEEP_FOR)
    {
        return None;
    }

    #[cfg(all(target_os = "none", feature = "sys-cortex-m"))]
    {
        if let Some(timeout) = cortex_m_soc_board::event_timeout_support() {
            return Some(MonotonicDeadlineWaitSupport {
                kind: MonotonicDeadlineWaitKind::ReservedOneShotAlarm,
                irqn: timeout.irqn,
                counter_bits: timeout.counter_bits,
                tick_hz: timeout.tick_hz,
                max_relative_timeout: timeout.max_relative_timeout,
            });
        }
    }

    Some(MonotonicDeadlineWaitSupport {
        kind: MonotonicDeadlineWaitKind::RelativeSleep,
        irqn: None,
        counter_bits: monotonic_raw_bits_for_selected_backend(caps),
        tick_hz: monotonic_tick_hz_for_selected_backend(caps),
        max_relative_timeout: None,
    })
}

fn raw_now_for_selected_backend(
    clock: &MonotonicRuntimeTime,
) -> Result<MonotonicRawInstant, ThreadError> {
    #[cfg(all(target_os = "none", feature = "sys-cortex-m"))]
    {
        if let Some(bits) = cortex_m_soc_board::monotonic_raw_bits() {
            let raw = cortex_m_soc_board::monotonic_raw_now().map_err(map_hardware_thread_error)?;
            return Ok(if bits >= 64 {
                MonotonicRawInstant::Bits64(raw)
            } else {
                MonotonicRawInstant::Bits32(raw as u32)
            });
        }
    }

    let raw = duration_to_ticks_floor(clock.inner.monotonic_now()?, 1_000_000_000)?;
    Ok(MonotonicRawInstant::Bits64(raw))
}

#[cfg(all(target_os = "none", feature = "sys-cortex-m"))]
fn map_hardware_thread_error(error: fusion_pal::contract::pal::HardwareError) -> ThreadError {
    match error.kind() {
        fusion_pal::contract::pal::HardwareErrorKind::Unsupported => ThreadError::unsupported(),
        _ => ThreadError::invalid(),
    }
}

#[doc(hidden)]
pub fn extend_32_snapshot(high: &AtomicU32, mut raw_now: impl FnMut() -> u32) -> u64 {
    loop {
        let high_before = high.load(Ordering::Acquire);
        let low = raw_now();
        let high_after = high.load(Ordering::Acquire);
        if high_before == high_after {
            return ((high_before as u64) << 32) | low as u64;
        }
    }
}

fn extend_32_software(raw: u32) -> u64 {
    loop {
        let observed_last = MONOTONIC_EXTENDED32_LAST_LOW.load(Ordering::Acquire);
        let observed_high = MONOTONIC_EXTENDED32_HIGH.load(Ordering::Acquire);
        let wrapped = raw < observed_last;
        let next_high = if wrapped {
            observed_high.wrapping_add(1)
        } else {
            observed_high
        };
        if MONOTONIC_EXTENDED32_LAST_LOW
            .compare_exchange(observed_last, raw, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            continue;
        }
        if wrapped {
            let _ = MONOTONIC_EXTENDED32_HIGH.compare_exchange(
                observed_high,
                next_high,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
            let high = MONOTONIC_EXTENDED32_HIGH.load(Ordering::Acquire);
            return ((high as u64) << 32) | raw as u64;
        }
        return ((observed_high as u64) << 32) | raw as u64;
    }
}

const fn deadline_reached_wrapping_u32(now: u32, deadline: u32) -> bool {
    now.wrapping_sub(deadline) <= U32_WRAP_HALF_RANGE
}

fn next_relative_sleep_chunk(
    remaining: Duration,
    max_chunk: Duration,
) -> Option<(Duration, Duration)> {
    if remaining.is_zero() {
        return None;
    }
    if max_chunk.is_zero() || remaining <= max_chunk {
        return Some((remaining, Duration::ZERO));
    }
    Some((max_chunk, remaining.saturating_sub(max_chunk)))
}

fn one_shot_alarm_timeout_for_remaining(
    remaining: Duration,
    max_relative_timeout: Option<Duration>,
) -> Option<Duration> {
    if let Some(max_relative_timeout) = max_relative_timeout.filter(|max| !max.is_zero()) {
        return (remaining <= max_relative_timeout).then_some(remaining);
    }
    Some(remaining)
}

fn duration_to_ticks_floor(duration: Duration, tick_hz: u64) -> Result<u64, ThreadError> {
    #[cfg(feature = "sys-cortex-m")]
    {
        let whole = duration
            .as_secs()
            .checked_mul(tick_hz)
            .ok_or_else(ThreadError::invalid)?;
        let fractional = (u64::from(duration.subsec_nanos()))
            .checked_mul(tick_hz)
            .ok_or_else(ThreadError::invalid)?
            / NANOS_PER_SECOND_U64;
        return whole
            .checked_add(fractional)
            .ok_or_else(ThreadError::invalid);
    }

    #[cfg(not(feature = "sys-cortex-m"))]
    let whole = (duration.as_secs() as u128)
        .checked_mul(tick_hz as u128)
        .ok_or_else(ThreadError::invalid)?;
    #[cfg(not(feature = "sys-cortex-m"))]
    let fractional = ((duration.subsec_nanos() as u128) * (tick_hz as u128)) / NANOS_PER_SECOND;
    #[cfg(not(feature = "sys-cortex-m"))]
    u64::try_from(whole + fractional).map_err(|_| ThreadError::invalid())
}

fn duration_to_ticks_ceil(duration: Duration, tick_hz: u64) -> Result<u64, ThreadError> {
    #[cfg(feature = "sys-cortex-m")]
    {
        let whole = duration
            .as_secs()
            .checked_mul(tick_hz)
            .ok_or_else(ThreadError::invalid)?;
        let numerator = (u64::from(duration.subsec_nanos()))
            .checked_mul(tick_hz)
            .ok_or_else(ThreadError::invalid)?;
        let fractional = numerator.div_ceil(NANOS_PER_SECOND_U64);
        return whole
            .checked_add(fractional)
            .ok_or_else(ThreadError::invalid);
    }

    #[cfg(not(feature = "sys-cortex-m"))]
    let whole = (duration.as_secs() as u128)
        .checked_mul(tick_hz as u128)
        .ok_or_else(ThreadError::invalid)?;
    #[cfg(not(feature = "sys-cortex-m"))]
    let fractional_numerator = (duration.subsec_nanos() as u128) * (tick_hz as u128);
    #[cfg(not(feature = "sys-cortex-m"))]
    let fractional = fractional_numerator.div_ceil(NANOS_PER_SECOND);
    #[cfg(not(feature = "sys-cortex-m"))]
    u64::try_from(whole + fractional).map_err(|_| ThreadError::invalid())
}

fn ticks_to_duration_floor(ticks: u64, tick_hz: u64) -> Result<Duration, ThreadError> {
    #[cfg(feature = "sys-cortex-m")]
    {
        let secs = ticks / tick_hz;
        let rem_ticks = ticks % tick_hz;
        let nanos = rem_ticks
            .checked_mul(NANOS_PER_SECOND_U64)
            .ok_or_else(ThreadError::invalid)?
            / tick_hz;
        let nanos = u32::try_from(nanos).map_err(|_| ThreadError::invalid())?;
        return Ok(Duration::new(secs, nanos));
    }

    #[cfg(not(feature = "sys-cortex-m"))]
    let secs = ticks / tick_hz;
    #[cfg(not(feature = "sys-cortex-m"))]
    let rem_ticks = ticks % tick_hz;
    #[cfg(not(feature = "sys-cortex-m"))]
    let nanos = ((rem_ticks as u128) * NANOS_PER_SECOND) / (tick_hz as u128);
    #[cfg(not(feature = "sys-cortex-m"))]
    let nanos = u32::try_from(nanos).map_err(|_| ThreadError::invalid())?;
    #[cfg(not(feature = "sys-cortex-m"))]
    Ok(Duration::new(secs, nanos))
}

fn ticks_to_duration_ceil(ticks: u64, tick_hz: u64) -> Result<Duration, ThreadError> {
    if ticks == 0 {
        return Ok(Duration::ZERO);
    }
    let secs = ticks / tick_hz;
    let rem_ticks = ticks % tick_hz;
    if rem_ticks == 0 {
        return Ok(Duration::new(secs, 0));
    }

    #[cfg(feature = "sys-cortex-m")]
    {
        let nanos = rem_ticks
            .checked_mul(NANOS_PER_SECOND_U64)
            .ok_or_else(ThreadError::invalid)?
            .div_ceil(tick_hz);
        let (secs, nanos) = if nanos >= NANOS_PER_SECOND_U64 {
            (secs.checked_add(1).ok_or_else(ThreadError::invalid)?, 0_u32)
        } else {
            (secs, nanos as u32)
        };
        return Ok(Duration::new(secs, nanos));
    }

    #[cfg(not(feature = "sys-cortex-m"))]
    let nanos = ((rem_ticks as u128) * NANOS_PER_SECOND).div_ceil(tick_hz as u128);
    #[cfg(not(feature = "sys-cortex-m"))]
    let nanos = u64::try_from(nanos).map_err(|_| ThreadError::invalid())?;
    #[cfg(not(feature = "sys-cortex-m"))]
    let (secs, nanos) = if nanos >= 1_000_000_000 {
        (secs.checked_add(1).ok_or_else(ThreadError::invalid)?, 0_u32)
    } else {
        (secs, nanos as u32)
    };
    #[cfg(not(feature = "sys-cortex-m"))]
    Ok(Duration::new(secs, nanos))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapping_deadline_compare_handles_single_wrap() {
        let now = MonotonicRawInstant::Bits32(5);
        let deadline = MonotonicRawInstant::Bits32(u32::MAX - 2);
        assert!(now.deadline_reached(deadline));
    }

    #[test]
    fn wrapping_deadline_compare_rejects_far_future_deadline() {
        let now = MonotonicRawInstant::Bits32(10);
        let deadline = MonotonicRawInstant::Bits32(0x8000_0010);
        assert!(!now.can_compare_deadline(deadline));
    }

    #[test]
    fn extend_32_snapshot_stitches_epoch_and_low_word() {
        let high = AtomicU32::new(3);
        let stitched = extend_32_snapshot(&high, || 7);
        assert_eq!(stitched, ((3_u64) << 32) | 7);
    }

    #[test]
    fn relative_sleep_chunk_returns_none_when_nothing_remains() {
        assert_eq!(
            next_relative_sleep_chunk(Duration::ZERO, Duration::from_millis(1)),
            None
        );
    }

    #[test]
    fn relative_sleep_chunk_returns_full_remaining_within_limit() {
        assert_eq!(
            next_relative_sleep_chunk(Duration::from_millis(3), Duration::from_millis(5)),
            Some((Duration::from_millis(3), Duration::ZERO))
        );
    }

    #[test]
    fn relative_sleep_chunk_splits_when_remaining_exceeds_limit() {
        assert_eq!(
            next_relative_sleep_chunk(Duration::from_millis(11), Duration::from_millis(4)),
            Some((Duration::from_millis(4), Duration::from_millis(7)))
        );
    }

    #[test]
    fn one_shot_alarm_timeout_accepts_deadline_within_window() {
        assert_eq!(
            one_shot_alarm_timeout_for_remaining(
                Duration::from_millis(3),
                Some(Duration::from_millis(5))
            ),
            Some(Duration::from_millis(3))
        );
    }

    #[test]
    fn one_shot_alarm_timeout_rejects_deadline_beyond_window() {
        assert_eq!(
            one_shot_alarm_timeout_for_remaining(
                Duration::from_millis(7),
                Some(Duration::from_millis(5))
            ),
            None
        );
    }

    #[test]
    fn one_shot_alarm_timeout_accepts_unbounded_sources() {
        assert_eq!(
            one_shot_alarm_timeout_for_remaining(Duration::from_millis(7), None),
            Some(Duration::from_millis(7))
        );
    }
}
