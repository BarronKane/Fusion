//! macOS fusion-pal event backend.

use crate::pal::event::{UnsupportedEvent, UnsupportedPoller};

/// Selected macOS event provider type.
pub type PlatformEvent = UnsupportedEvent;
/// Selected macOS poller type.
pub type PlatformPoller = UnsupportedPoller;

/// Returns the selected macOS event provider.
#[must_use]
pub const fn system_event() -> PlatformEvent {
    PlatformEvent::new()
}
