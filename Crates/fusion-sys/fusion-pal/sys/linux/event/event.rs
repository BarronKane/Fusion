//! Linux fusion-pal event backend.
//!
//! The readiness/completion contract is now explicit at the fusion-pal layer, but the concrete
//! Linux `epoll` implementation is still deferred. The backend therefore reports the
//! honest unsupported surface instead of smuggling in a half-finished reactor.

use crate::pal::event::{UnsupportedEvent, UnsupportedPoller};

/// Selected Linux event provider type.
pub type PlatformEvent = UnsupportedEvent;
/// Selected Linux poller type.
pub type PlatformPoller = UnsupportedPoller;

/// Returns the selected Linux event provider.
#[must_use]
pub const fn system_event() -> PlatformEvent {
    PlatformEvent::new()
}
