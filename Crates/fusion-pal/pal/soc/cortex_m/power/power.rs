//! Cortex-M bare-metal power backend.

use crate::contract::hardware::power::{
    PowerBase,
    PowerCaps,
    PowerControl,
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
pub struct CortexMPower;

/// Selected Cortex-M power provider type.
pub type PlatformPower = CortexMPower;

/// Returns the selected Cortex-M power provider.
#[must_use]
pub const fn system_power() -> PlatformPower {
    PlatformPower::new()
}

impl CortexMPower {
    /// Creates a new Cortex-M power provider handle.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl PowerBase for CortexMPower {
    fn support(&self) -> PowerSupport {
        CORTEX_M_POWER_SUPPORT
    }
}

impl PowerControl for CortexMPower {
    fn modes(&self) -> &'static [PowerModeDescriptor] {
        crate::pal::soc::cortex_m::hal::soc::board::pal_power_modes()
    }

    fn enter_mode(&self, name: &str) -> Result<(), PowerError> {
        crate::pal::soc::cortex_m::hal::soc::board::enter_power_mode(name).map_err(map_power_error)
    }
}

const fn map_power_error(error: crate::contract::hal::HardwareError) -> PowerError {
    match error.kind() {
        crate::contract::hal::HardwareErrorKind::Unsupported => PowerError::unsupported(),
        crate::contract::hal::HardwareErrorKind::Invalid => PowerError::invalid(),
        crate::contract::hal::HardwareErrorKind::ResourceExhausted
        | crate::contract::hal::HardwareErrorKind::Busy => PowerError::busy(),
        crate::contract::hal::HardwareErrorKind::StateConflict => PowerError::state_conflict(),
        crate::contract::hal::HardwareErrorKind::Platform(code) => PowerError::platform(code),
    }
}
