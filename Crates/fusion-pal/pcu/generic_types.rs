//! Shared generic coprocessor identifiers and descriptor vocabulary.

/// Opaque coprocessor device identifier surfaced by one backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuDeviceId(pub u8);

/// Coarse device class for one surfaced coprocessor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuDeviceClass {
    /// Compute- or shader-shaped coprocessor.
    Compute,
    /// Programmable IO or data-plane engine.
    Io,
    /// DSP or signal-processing accelerator.
    Signal,
    /// Neural or ML-focused accelerator.
    Neural,
    /// Media, video, or imaging accelerator.
    Media,
    /// Backend-specific or currently unclassified device.
    Other,
}

/// Static descriptor for one surfaced coprocessor device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuDeviceDescriptor {
    /// Stable device identifier.
    pub id: PcuDeviceId,
    /// Human-readable device name.
    pub name: &'static str,
    /// Coarse device class.
    pub class: PcuDeviceClass,
}

/// Exclusive device claim returned by one backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuDeviceClaim {
    pub(crate) device: PcuDeviceId,
}

impl PcuDeviceClaim {
    /// Returns the claimed device identifier.
    #[must_use]
    pub const fn device(self) -> PcuDeviceId {
        self.device
    }
}
