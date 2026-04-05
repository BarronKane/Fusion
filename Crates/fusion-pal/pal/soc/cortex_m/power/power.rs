//! Cortex-M bare-metal power backend.

use crate::contract::pal::power::{
    PowerBaseContract,
    PowerCaps,
    PowerControlContract,
    PowerError,
    PowerImplementationKind,
    PowerModeDescriptor,
    PowerSupport,
};

const CORTEX_M_POWER_SUPPORT: PowerSupport = PowerSupport {
    caps: PowerCaps::ENUMERATE.union(PowerCaps::ENTER),
    implementation: PowerImplementationKind::Emulated,
};

/// Cortex-M power provider type.
#[derive(Debug, Clone, Copy, Default)]
pub struct Power;

/// Selected Cortex-M power provider type.
pub type PlatformPower = Power;

/// Returns the selected Cortex-M power provider.
#[must_use]
pub const fn system_power() -> PlatformPower {
    PlatformPower::new()
}

impl Power {
    /// Creates a new Cortex-M power provider handle.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl PowerBaseContract for Power {
    fn support(&self) -> PowerSupport {
        CORTEX_M_POWER_SUPPORT
    }
}

impl PowerControlContract for Power {
    fn modes(&self) -> &'static [PowerModeDescriptor] {
        crate::pal::soc::cortex_m::hal::soc::board::pal_power_modes()
    }

    fn enter_mode(&self, name: &str) -> Result<(), PowerError> {
        crate::pal::soc::cortex_m::hal::soc::board::enter_power_mode(name).map_err(map_power_error)
    }
}

const fn map_power_error(error: crate::contract::pal::HardwareError) -> PowerError {
    match error.kind() {
        crate::contract::pal::HardwareErrorKind::Unsupported => PowerError::unsupported(),
        crate::contract::pal::HardwareErrorKind::Invalid => PowerError::invalid(),
        crate::contract::pal::HardwareErrorKind::ResourceExhausted
        | crate::contract::pal::HardwareErrorKind::Busy => PowerError::busy(),
        crate::contract::pal::HardwareErrorKind::StateConflict => PowerError::state_conflict(),
        crate::contract::pal::HardwareErrorKind::Platform(code) => PowerError::platform(code),
    }
}
