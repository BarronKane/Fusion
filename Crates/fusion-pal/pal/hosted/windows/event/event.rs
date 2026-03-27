//! Windows fusion-pal event backend.

use crate::contract::runtime::event::{UnsupportedEvent, UnsupportedPoller};

/// Selected Windows event provider type.
pub type PlatformEvent = UnsupportedEvent;
/// Selected Windows poller type.
pub type PlatformPoller = UnsupportedPoller;

/// Returns the selected Windows event provider.
#[must_use]
pub const fn system_event() -> PlatformEvent {
    PlatformEvent::new()
}
