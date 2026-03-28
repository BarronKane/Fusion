//! iOS fusion-pal event backend.

use crate::contract::pal::runtime::event::{UnsupportedEvent, UnsupportedPoller};

/// Selected iOS event provider type.
pub type PlatformEvent = UnsupportedEvent;
/// Selected iOS poller type.
pub type PlatformPoller = UnsupportedPoller;

/// Returns the selected iOS event provider.
#[must_use]
pub const fn system_event() -> PlatformEvent {
    PlatformEvent::new()
}
