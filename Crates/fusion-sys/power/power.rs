//! fusion-sys-level power wrappers built on top of fusion-pal-truthful backends.

pub use fusion_pal::sys::power::{
    PowerBase,
    PowerCaps,
    PowerControl,
    PowerError,
    PowerErrorKind,
    PowerImplementationKind,
    PowerModeDepth,
    PowerModeDescriptor,
    PowerSupport,
};
use fusion_pal::sys::power::{
    PlatformPower,
    system_power as pal_system_power,
};

/// fusion-sys power provider wrapper around the selected fusion-pal backend.
#[derive(Debug, Clone, Copy)]
pub struct PowerSystem {
    inner: PlatformPower,
}

impl PowerSystem {
    /// Creates a wrapper for the selected platform power provider.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            inner: pal_system_power(),
        }
    }

    /// Reports the truthful power surface for the selected backend.
    #[must_use]
    pub fn support(&self) -> PowerSupport {
        PowerBase::support(&self.inner)
    }

    /// Returns the named power modes surfaced by the selected backend.
    #[must_use]
    pub fn modes(&self) -> &'static [PowerModeDescriptor] {
        PowerControl::modes(&self.inner)
    }

    /// Enters one named power mode.
    ///
    /// # Errors
    ///
    /// Returns any honest backend failure, including unsupported or invalid modes.
    pub fn enter_mode(&self, name: &str) -> Result<(), PowerError> {
        PowerControl::enter_mode(&self.inner, name)
    }
}

impl Default for PowerSystem {
    fn default() -> Self {
        Self::new()
    }
}
