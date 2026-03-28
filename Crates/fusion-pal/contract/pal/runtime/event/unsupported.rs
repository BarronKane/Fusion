//! Backend-neutral unsupported event implementation.

use super::{
    EventBase,
    EventCompletionOp,
    EventError,
    EventInterest,
    EventKey,
    EventRecord,
    EventSource,
    EventSourceHandle,
    EventSupport,
};

/// Unsupported event provider placeholder.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnsupportedEvent;

/// Unsupported poller placeholder.
#[derive(Debug, Default, PartialEq, Eq, Hash)]
pub struct UnsupportedPoller;

impl UnsupportedEvent {
    /// Creates a new unsupported event provider placeholder.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl EventBase for UnsupportedEvent {
    type Poller = UnsupportedPoller;

    fn support(&self) -> EventSupport {
        EventSupport::unsupported()
    }
}

impl EventSource for UnsupportedEvent {
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
