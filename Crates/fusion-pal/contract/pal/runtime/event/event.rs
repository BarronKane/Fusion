//! Backend-neutral event and reactor vocabulary.
//!
//! The event fusion-pal is intentionally honest about the split between readiness-style pollers
//! and completion-style pollers. Readiness backends such as `epoll` or `kqueue` do not
//! have identical semantics to completion backends such as IOCP, so the model is surfaced
//! explicitly instead of being smoothed over into folklore.

mod caps;
mod error;
mod unsupported;

use core::time::Duration;

use bitflags::bitflags;

pub use caps::*;
pub use error::*;
pub use unsupported::*;

/// Opaque OS object or descriptor registered with an event poller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EventSourceHandle(pub usize);

/// Stable key returned when a source is registered with a poller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EventKey(pub u64);

/// Registration policy for a source handle attached to an event backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EventRegistrationMode {
    /// Leave the source asserted until the producer or consumer clears it explicitly.
    LevelSticky,
    /// Acknowledge the source after surfacing one readiness notification.
    LevelAckOnPoll,
    /// Surface one readiness notification, then disable or drop the registration.
    OneShot,
}

/// Full registration request for one event source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EventRegistration {
    /// Source handle associated with the registration.
    pub source: EventSourceHandle,
    /// Interest set requested for the source.
    pub interest: EventInterest,
    /// Registration delivery policy requested for the source.
    pub mode: EventRegistrationMode,
}

bitflags! {
    /// Requested interest set for a registered event source.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct EventInterest: u32 {
        /// Readability or receive-side interest.
        const READABLE = 1 << 0;
        /// Writability or send-side interest.
        const WRITABLE = 1 << 1;
        /// Priority or out-of-band interest.
        const PRIORITY = 1 << 2;
        /// Error notifications are desired.
        const ERROR    = 1 << 3;
        /// Hangup or peer-close notifications are desired.
        const HANGUP   = 1 << 4;
    }
}

bitflags! {
    /// Readiness bits returned by readiness-oriented pollers.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct EventReadiness: u32 {
        /// Read side became ready.
        const READABLE = 1 << 0;
        /// Write side became ready.
        const WRITABLE = 1 << 1;
        /// Priority or out-of-band data is ready.
        const PRIORITY = 1 << 2;
        /// An error condition is present.
        const ERROR    = 1 << 3;
        /// The source reported a hangup or peer close.
        const HANGUP   = 1 << 4;
    }
}

/// Completion-oriented event payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EventCompletion {
    /// Optional completed transfer size if the backend can report it honestly.
    pub bytes_transferred: Option<usize>,
    /// Whether the completed operation succeeded.
    pub success: bool,
}

/// Completion operation kind submitted to a completion-oriented backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EventCompletionOpKind {
    /// Completion for a read-style operation.
    Read,
    /// Completion for a write-style operation.
    Write,
    /// Completion for an accept-style operation.
    Accept,
    /// Completion for a connect-style operation.
    Connect,
    /// Backend-specific completion operation.
    Custom(u16),
}

/// Completion-style operation submitted to a backend poller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EventCompletionOp {
    /// Source associated with the submitted operation.
    pub source: EventSourceHandle,
    /// Operation kind associated with the submission.
    pub kind: EventCompletionOpKind,
    /// Opaque caller-owned token echoed back by backend completion records later.
    pub user_data: usize,
}

/// Concrete notification produced by a backend poller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EventNotification {
    /// Readiness-oriented notification.
    Readiness(EventReadiness),
    /// Completion-oriented notification.
    Completion(EventCompletion),
}

/// Event record emitted by a poller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EventRecord {
    /// Registration key associated with the notification.
    pub key: EventKey,
    /// Concrete notification payload.
    pub notification: EventNotification,
}

/// Capability trait for event-poller backends.
pub trait EventBaseContract {
    /// Opaque poller handle owned by the selected backend.
    type Poller;

    /// Reports the truthful event-poller surface for this backend.
    fn support(&self) -> EventSupport;
}

/// Registration and polling contract for a backend event source.
pub trait EventSourceContract: EventBaseContract {
    /// Creates a new poller instance.
    ///
    /// # Errors
    ///
    /// Returns any honest backend failure, including unsupported polling surfaces or
    /// resource exhaustion.
    fn create(&self) -> Result<Self::Poller, EventError>;

    /// Registers a source handle and returns the backend key used to identify it later.
    ///
    /// # Errors
    ///
    /// Returns any honest registration failure, including invalid source handles,
    /// unsupported interest modes, or backend resource exhaustion.
    fn register(
        &self,
        poller: &mut Self::Poller,
        source: EventSourceHandle,
        interest: EventInterest,
    ) -> Result<EventKey, EventError>;

    /// Registers a source handle with an explicit delivery policy.
    ///
    /// # Errors
    ///
    /// Returns any honest registration failure, including invalid source handles, unsupported
    /// interest or delivery modes, or backend resource exhaustion.
    fn register_with(
        &self,
        poller: &mut Self::Poller,
        registration: EventRegistration,
    ) -> Result<EventKey, EventError> {
        if registration.mode != EventRegistrationMode::LevelSticky {
            return Err(EventError::unsupported());
        }

        self.register(poller, registration.source, registration.interest)
    }

    /// Updates the registration interest for an existing source.
    ///
    /// # Errors
    ///
    /// Returns any honest backend failure when the registration cannot be updated.
    fn reregister(
        &self,
        poller: &mut Self::Poller,
        key: EventKey,
        interest: EventInterest,
    ) -> Result<(), EventError>;

    /// Updates an existing registration with an explicit delivery policy.
    ///
    /// # Errors
    ///
    /// Returns any honest backend failure when the registration cannot be updated.
    fn reregister_with(
        &self,
        poller: &mut Self::Poller,
        key: EventKey,
        registration: EventRegistration,
    ) -> Result<(), EventError> {
        if registration.mode != EventRegistrationMode::LevelSticky {
            return Err(EventError::unsupported());
        }

        self.reregister(poller, key, registration.interest)
    }

    /// Removes a previously registered source.
    ///
    /// # Errors
    ///
    /// Returns any honest deregistration failure.
    fn deregister(&self, poller: &mut Self::Poller, key: EventKey) -> Result<(), EventError>;

    /// Submits a completion-style operation to the backend when supported.
    ///
    /// # Errors
    ///
    /// Returns any honest backend failure, including unsupported completion submission or
    /// invalid operation descriptors.
    fn submit(
        &self,
        poller: &mut Self::Poller,
        operation: EventCompletionOp,
    ) -> Result<EventKey, EventError>;

    /// Polls the backend for ready or completed events.
    ///
    /// # Errors
    ///
    /// Returns any honest poll failure, including timeout or backend state conflict when
    /// that is how the selected backend reports it.
    fn poll(
        &self,
        poller: &mut Self::Poller,
        events: &mut [EventRecord],
        timeout: Option<Duration>,
    ) -> Result<usize, EventError>;
}
