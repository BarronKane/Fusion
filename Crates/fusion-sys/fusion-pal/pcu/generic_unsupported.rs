//! Backend-neutral unsupported generic coprocessor implementation.

use super::{
    PcuBase,
    PcuControl,
    PcuDeviceClaim,
    PcuDeviceDescriptor,
    PcuDeviceId,
    PcuError,
    PcuSupport,
};

/// Unsupported generic coprocessor provider placeholder.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnsupportedPcu;

impl UnsupportedPcu {
    /// Creates a new unsupported generic coprocessor provider placeholder.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl PcuBase for UnsupportedPcu {
    fn support(&self) -> PcuSupport {
        PcuSupport::unsupported()
    }

    fn devices(&self) -> &'static [PcuDeviceDescriptor] {
        &[]
    }
}

impl PcuControl for UnsupportedPcu {
    fn claim_device(&self, _device: PcuDeviceId) -> Result<PcuDeviceClaim, PcuError> {
        Err(PcuError::unsupported())
    }

    fn release_device(&self, _claim: PcuDeviceClaim) -> Result<(), PcuError> {
        Err(PcuError::unsupported())
    }
}
