//! Capability vocabulary for PAL event polling.

use bitflags::bitflags;

bitflags! {
    /// Event-poller features the backend can honestly surface.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct EventCaps: u32 {
        /// Readiness-style notifications are supported.
        const READINESS       = 1 << 0;
        /// Completion-style notifications are supported.
        const COMPLETION      = 1 << 1;
        /// The backend can accept explicit completion submissions.
        const SUBMIT          = 1 << 2;
        /// Level-triggered readiness is supported.
        const LEVEL_TRIGGERED = 1 << 3;
        /// Edge-triggered readiness is supported.
        const EDGE_TRIGGERED  = 1 << 4;
        /// One-shot registrations are supported.
        const ONESHOT         = 1 << 5;
        /// Poll timeouts are supported.
        const TIMEOUT         = 1 << 6;
    }
}

/// Truthful event model exposed by the backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EventModel {
    /// Readiness-oriented polling.
    Readiness,
    /// Completion-oriented polling.
    Completion,
    /// Hybrid backend able to surface both models honestly.
    Hybrid,
    /// Unsupported on this backend.
    Unsupported,
}

/// Backend implementation category for event polling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EventImplementationKind {
    /// Native kernel or platform event source.
    Native,
    /// Emulated on top of other primitives.
    Emulated,
    /// Unsupported on this backend.
    Unsupported,
}

/// Full capability surface for a backend event provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EventSupport {
    /// Backend-supported event features.
    pub caps: EventCaps,
    /// Readiness, completion, or hybrid event model.
    pub model: EventModel,
    /// Maximum events the backend can report in one poll, if bounded.
    pub max_events: Option<usize>,
    /// Native, emulated, or unsupported implementation category.
    pub implementation: EventImplementationKind,
}

impl EventSupport {
    /// Returns a fully unsupported event surface.
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            caps: EventCaps::empty(),
            model: EventModel::Unsupported,
            max_events: None,
            implementation: EventImplementationKind::Unsupported,
        }
    }
}
