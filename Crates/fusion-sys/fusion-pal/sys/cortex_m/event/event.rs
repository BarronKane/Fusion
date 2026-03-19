//! Cortex-M bare-metal event backend.
//!
//! A generic readiness/completion poller is not surfaced here yet. NVIC and peripheral IRQ
//! routing are real, but they are not the same thing as a backend-neutral event reactor.

use crate::pal::event::{
    EventBase, EventCompletionOp, EventError, EventInterest, EventKey, EventRecord, EventSource,
    EventSourceHandle, EventSupport,
};

/// Cortex-M event provider type.
#[derive(Debug, Clone, Copy, Default)]
pub struct CortexMEvent;

/// Cortex-M poller placeholder.
#[derive(Debug, Default, PartialEq, Eq, Hash)]
pub struct CortexMPoller;

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

impl EventBase for CortexMEvent {
    type Poller = CortexMPoller;

    fn support(&self) -> EventSupport {
        EventSupport::unsupported()
    }
}

impl EventSource for CortexMEvent {
    fn create(&self) -> Result<Self::Poller, EventError> {
        Err(EventError::unsupported())
    }

    fn register(
        &self,
        _poller: &mut Self::Poller,
        _source: EventSourceHandle,
        _interest: EventInterest,
    ) -> Result<EventKey, EventError> {
        Err(EventError::unsupported())
    }

    fn reregister(
        &self,
        _poller: &mut Self::Poller,
        _key: EventKey,
        _interest: EventInterest,
    ) -> Result<(), EventError> {
        Err(EventError::unsupported())
    }

    fn deregister(&self, _poller: &mut Self::Poller, _key: EventKey) -> Result<(), EventError> {
        Err(EventError::unsupported())
    }

    fn submit(
        &self,
        _poller: &mut Self::Poller,
        _operation: EventCompletionOp,
    ) -> Result<EventKey, EventError> {
        Err(EventError::unsupported())
    }

    fn poll(
        &self,
        _poller: &mut Self::Poller,
        _events: &mut [EventRecord],
        _timeout: Option<core::time::Duration>,
    ) -> Result<usize, EventError> {
        Err(EventError::unsupported())
    }
}
