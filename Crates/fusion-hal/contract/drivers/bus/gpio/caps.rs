//! Capability vocabulary for generic GPIO backends.

use bitflags::bitflags;

/// Implementation-category vocabulary specialized for GPIO support.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GpioImplementationKind {
    /// Native backend implementation.
    Native,
    /// Lowered or adapted implementation that preserves the public GPIO contract with caveats.
    Emulated,
    /// Unsupported placeholder.
    Unsupported,
}

bitflags! {
    /// Generic GPIO backend features the provider can honestly surface.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct GpioProviderCaps: u32 {
        /// The backend can enumerate surfaced GPIO pins.
        const ENUMERATE           = 1 << 0;
        /// Pins can be claimed explicitly.
        const CLAIM               = 1 << 1;
        /// The surfaced pin inventory is backed by one static topology declaration.
        const STATIC_TOPOLOGY     = 1 << 2;
        /// Surfaced pins can be configured as inputs.
        const INPUT               = 1 << 3;
        /// Surfaced pins can be configured as outputs.
        const OUTPUT              = 1 << 4;
        /// Surfaced pins expose alternate-function muxing.
        const ALTERNATE_FUNCTIONS = 1 << 5;
        /// Surfaced pins expose pull resistors.
        const PULLS               = 1 << 6;
        /// Surfaced pins expose selectable drive strength.
        const DRIVE_STRENGTH      = 1 << 7;
        /// Surfaced pins can act as interrupt or event sources.
        const INTERRUPTS          = 1 << 8;
    }
}

bitflags! {
    /// Honest capability set for one surfaced GPIO pin.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct GpioCapabilities: u32 {
        /// The pin can be configured as one input.
        const INPUT               = 1 << 0;
        /// The pin can be configured as one push-pull output.
        const OUTPUT              = 1 << 1;
        /// The pin exposes alternate-function muxing.
        const ALTERNATE_FUNCTIONS = 1 << 2;
        /// The pin exposes pull resistors.
        const PULLS               = 1 << 3;
        /// The pin exposes selectable drive strength.
        const DRIVE_STRENGTH      = 1 << 4;
        /// The pin can act as one interrupt/event source.
        const INTERRUPTS          = 1 << 5;
    }
}

/// Full capability surface for one generic GPIO backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GpioSupport {
    /// Backend-supported generic GPIO features.
    pub caps: GpioProviderCaps,
    /// Native, lowered-with-restrictions, or unsupported implementation category.
    pub implementation: GpioImplementationKind,
    /// Number of surfaced GPIO pins.
    pub pin_count: u16,
}

impl GpioSupport {
    /// Returns a fully unsupported generic GPIO surface.
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            caps: GpioProviderCaps::empty(),
            implementation: GpioImplementationKind::Unsupported,
            pin_count: 0,
        }
    }
}
