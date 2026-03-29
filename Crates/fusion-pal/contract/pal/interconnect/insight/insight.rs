//! Channel-native debug/inspection vocabulary.
//!
//! Insight is not a magical sidecar outside Fusion's transport law. When enabled, a subsystem may
//! surface dedicated one-way insight side channels alongside its ordinary channels. Those insight
//! channels stay explicitly typed and bounded instead of smuggling debug payload through the
//! application's real protocol.

mod error;

pub use error::*;

/// High-level class of one insight side channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InsightChannelClass {
    /// Timeline-oriented spans/events/timing traffic.
    Timeline,
    /// Current state/counter/ownership traffic.
    State,
    /// Heavier point-in-time snapshot traffic.
    Snapshot,
    /// Control requests that shape capture behavior.
    Control,
}

/// Capture fidelity surfaced by one insight side channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InsightCaptureMode {
    /// Bounded/lossy capture is acceptable.
    Lossy,
    /// Exact capture is required.
    Exact,
}

/// Availability category for one insight surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InsightAvailabilityKind {
    /// The insight surface is available.
    Available,
    /// The insight surface is compiled out by feature policy.
    DisabledByFeature,
}

/// Static support surface for one configured insight channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InsightSupport {
    /// Availability category.
    pub availability: InsightAvailabilityKind,
    /// Insight class.
    pub class: InsightChannelClass,
    /// Capture fidelity.
    pub capture: InsightCaptureMode,
}

impl InsightSupport {
    /// Returns one available insight support surface.
    #[must_use]
    pub const fn available(class: InsightChannelClass, capture: InsightCaptureMode) -> Self {
        Self {
            availability: InsightAvailabilityKind::Available,
            class,
            capture,
        }
    }

    /// Returns one feature-disabled insight support surface.
    #[must_use]
    pub const fn disabled_by_feature(
        class: InsightChannelClass,
        capture: InsightCaptureMode,
    ) -> Self {
        Self {
            availability: InsightAvailabilityKind::DisabledByFeature,
            class,
            capture,
        }
    }
}
