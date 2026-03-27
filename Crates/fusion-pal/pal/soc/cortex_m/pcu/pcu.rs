//! Cortex-M coprocessor backend.

use core::sync::atomic::{AtomicBool, Ordering};

use crate::pcu::{
    PcuBase,
    PcuCaps,
    PcuControl,
    PcuDeviceClaim,
    PcuDeviceClass,
    PcuDeviceDescriptor,
    PcuDeviceId,
    PcuError,
    PcuSupport,
};

use crate::pal::soc::cortex_m::hal::soc::pio::{PioBase, system_pio};

const CORTEX_M_PIO_DEVICE_ID: PcuDeviceId = PcuDeviceId(0);

static CORTEX_M_PIO_DEVICES: [PcuDeviceDescriptor; 1] = [PcuDeviceDescriptor {
    id: CORTEX_M_PIO_DEVICE_ID,
    name: "cortex-m-pio",
    class: PcuDeviceClass::Io,
}];
static CORTEX_M_PIO_DEVICE_CLAIMED: AtomicBool = AtomicBool::new(false);

/// Cortex-M coprocessor provider type.
#[derive(Debug, Clone, Copy, Default)]
pub struct CortexMPcu;

/// Selected Cortex-M programmable-IO provider type.
pub type PlatformPcu = CortexMPcu;

/// Returns the selected Cortex-M coprocessor provider.
#[must_use]
pub const fn system_pcu() -> PlatformPcu {
    PlatformPcu::new()
}

impl CortexMPcu {
    /// Creates a new Cortex-M coprocessor provider handle.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl PcuBase for CortexMPcu {
    fn support(&self) -> PcuSupport {
        let support = system_pio().support();
        if support.engine_count == 0 {
            return PcuSupport::unsupported();
        }

        PcuSupport {
            caps: PcuCaps::ENUMERATE
                | PcuCaps::CLAIM_DEVICE
                | PcuCaps::DISPATCH
                | PcuCaps::COMPLETION_STATUS
                | PcuCaps::EXTERNAL_RESOURCES,
            implementation: support.implementation,
            device_count: 1,
        }
    }

    fn devices(&self) -> &'static [PcuDeviceDescriptor] {
        if system_pio().support().engine_count == 0 {
            &[]
        } else {
            &CORTEX_M_PIO_DEVICES
        }
    }
}

impl PcuControl for CortexMPcu {
    fn claim_device(&self, device: PcuDeviceId) -> Result<PcuDeviceClaim, PcuError> {
        if device != CORTEX_M_PIO_DEVICE_ID || system_pio().support().engine_count == 0 {
            return Err(PcuError::invalid());
        }
        CORTEX_M_PIO_DEVICE_CLAIMED
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| PcuError::busy())?;
        Ok(PcuDeviceClaim { device })
    }

    fn release_device(&self, claim: PcuDeviceClaim) -> Result<(), PcuError> {
        if claim.device() != CORTEX_M_PIO_DEVICE_ID {
            return Err(PcuError::invalid());
        }
        if !CORTEX_M_PIO_DEVICE_CLAIMED.swap(false, Ordering::AcqRel) {
            return Err(PcuError::state_conflict());
        }
        Ok(())
    }
}
