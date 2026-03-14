//! fusion-sys-level event wrappers built on top of fusion-pal-truthful backends.
//!
//! `fusion-sys::event` is the narrow policy-free layer above the fusion-pal event contracts. It
//! keeps the readiness-vs-completion distinction intact and exposes selected backend
//! pollers without pretending different kernel event models are secretly identical.

use core::time::Duration;

pub use fusion_pal::sys::event::{
    EventBase, EventCaps, EventCompletion, EventCompletionOp, EventCompletionOpKind, EventError,
    EventErrorKind, EventImplementationKind, EventInterest, EventKey, EventModel,
    EventNotification, EventReadiness, EventRecord, EventSource, EventSourceHandle, EventSupport,
};
use fusion_pal::sys::event::{PlatformEvent, PlatformPoller, system_event as pal_system_event};

/// fusion-sys event provider wrapper around the selected fusion-pal backend.
#[derive(Debug, Clone, Copy)]
pub struct EventSystem {
    inner: PlatformEvent,
}

/// Owned poller handle for the selected backend.
#[derive(Debug)]
pub struct EventPoller {
    inner: PlatformPoller,
}

impl EventSystem {
    /// Creates a wrapper for the selected platform event provider.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            inner: pal_system_event(),
        }
    }

    /// Reports the truthful event surface for the selected backend.
    #[must_use]
    pub fn support(&self) -> EventSupport {
        EventBase::support(&self.inner)
    }

    /// Creates a new backend poller instance.
    ///
    /// # Errors
    ///
    /// Returns any honest backend failure, including unsupported polling surfaces.
    pub fn create(&self) -> Result<EventPoller, EventError> {
        let poller = EventSource::create(&self.inner)?;
        Ok(EventPoller { inner: poller })
    }

    /// Registers a source with the backend poller.
    ///
    /// # Errors
    ///
    /// Returns any honest backend registration failure.
    pub fn register(
        &self,
        poller: &mut EventPoller,
        source: EventSourceHandle,
        interest: EventInterest,
    ) -> Result<EventKey, EventError> {
        EventSource::register(&self.inner, &mut poller.inner, source, interest)
    }

    /// Updates an existing registration.
    ///
    /// # Errors
    ///
    /// Returns any honest backend re-registration failure.
    pub fn reregister(
        &self,
        poller: &mut EventPoller,
        key: EventKey,
        interest: EventInterest,
    ) -> Result<(), EventError> {
        EventSource::reregister(&self.inner, &mut poller.inner, key, interest)
    }

    /// Removes an existing registration.
    ///
    /// # Errors
    ///
    /// Returns any honest backend deregistration failure.
    pub fn deregister(&self, poller: &mut EventPoller, key: EventKey) -> Result<(), EventError> {
        EventSource::deregister(&self.inner, &mut poller.inner, key)
    }

    /// Submits a completion-style operation when the backend supports it.
    ///
    /// # Errors
    ///
    /// Returns any honest backend failure, including unsupported completion submission.
    pub fn submit(
        &self,
        poller: &mut EventPoller,
        operation: EventCompletionOp,
    ) -> Result<EventKey, EventError> {
        EventSource::submit(&self.inner, &mut poller.inner, operation)
    }

    /// Polls the backend for ready or completed events.
    ///
    /// # Errors
    ///
    /// Returns any honest backend polling failure.
    pub fn poll(
        &self,
        poller: &mut EventPoller,
        events: &mut [EventRecord],
        timeout: Option<Duration>,
    ) -> Result<usize, EventError> {
        EventSource::poll(&self.inner, &mut poller.inner, events, timeout)
    }
}

impl Default for EventSystem {
    fn default() -> Self {
        Self::new()
    }
}
