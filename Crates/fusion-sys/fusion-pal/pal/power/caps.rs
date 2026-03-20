//! Capability vocabulary for fusion-pal power control.

use bitflags::bitflags;

/// Shared implementation-category vocabulary specialized for power support.
pub use crate::pal::caps::ImplementationKind as PowerImplementationKind;

bitflags! {
    /// Power-management features the backend can honestly surface.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct PowerCaps: u32 {
        /// The backend can enumerate named power modes.
        const ENUMERATE = 1 << 0;
        /// The backend can enter a named power mode.
        const ENTER     = 1 << 1;
    }
}

/// Full capability surface for a backend power provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PowerSupport {
    /// Backend-supported power features.
    pub caps: PowerCaps,
    /// Native, emulated, or unsupported implementation category.
    pub implementation: PowerImplementationKind,
}

impl PowerSupport {
    /// Returns a fully unsupported power surface.
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            caps: PowerCaps::empty(),
            implementation: PowerImplementationKind::Unsupported,
        }
    }
}
