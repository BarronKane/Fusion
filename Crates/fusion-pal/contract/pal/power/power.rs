//! Backend-neutral power-management vocabulary.

mod caps;
mod error;
mod unsupported;

pub use caps::*;
pub use error::*;
pub use unsupported::*;

/// Coarse sleep-depth classification for a power mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PowerModeDepth {
    /// Light idle or standby state.
    Idle,
    /// Ordinary sleep state.
    Sleep,
    /// Deep-sleep or retention-heavy low-power state.
    DeepSleep,
    /// Backend-specific mode that does not fit a simple shared bucket.
    Other,
}

/// Static power-mode descriptor exposed by a backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PowerModeDescriptor {
    /// Human-readable mode name.
    pub name: &'static str,
    /// Coarse depth classification for the mode.
    pub depth: PowerModeDepth,
    /// Coarse wake sources surfaced by the backend.
    pub wake_sources: &'static [&'static str],
    /// Coarse domains or sinks typically gated in the mode.
    pub gated_domains: &'static [&'static str],
}

/// Capability trait for power-control backends.
pub trait PowerBaseContract {
    /// Reports the truthful power-management surface for this backend.
    fn support(&self) -> PowerSupport;
}

/// Enumeration and control contract for backend power providers.
pub trait PowerControlContract: PowerBaseContract {
    /// Returns the named power modes surfaced by the backend.
    #[must_use]
    fn modes(&self) -> &'static [PowerModeDescriptor];

    /// Enters one named power mode.
    ///
    /// # Errors
    ///
    /// Returns any honest backend failure, including unsupported modes or invalid names.
    fn enter_mode(&self, name: &str) -> Result<(), PowerError>;
}
