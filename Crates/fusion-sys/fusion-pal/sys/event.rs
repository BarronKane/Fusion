//! Public event export for the selected platform backend.

/// Concrete event provider type, poller type, and constructor for the selected platform.
pub use super::platform::event::{PlatformEvent, PlatformPoller, system_event};
/// Backend-neutral fusion-pal event vocabulary and traits.
pub use crate::pal::event::*;
