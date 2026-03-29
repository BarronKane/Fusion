//! Shared generic PCU executor identifiers and descriptor vocabulary.

/// Opaque PCU executor identifier surfaced by one backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuExecutorId(pub u8);

/// Coarse executor class for one surfaced PCU executor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuExecutorClass {
    /// CPU-backed software execution.
    Cpu,
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
    /// Lowering or adaptation executor (for example a `SPIR-V` adapter).
    Adapter,
    /// Executor projected from another domain or machine.
    Remote,
    /// Backend-specific or currently unclassified executor.
    Other,
}

/// Provenance for one surfaced executor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuExecutorOrigin {
    /// Synthesized by Fusion itself rather than bound from one topology node.
    Synthetic,
    /// Bound from one topology-discovered hardware node.
    TopologyBound,
    /// Projected from another courier or domain.
    Projected,
}

/// Static descriptor for one surfaced PCU executor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuExecutorDescriptor {
    /// Stable executor identifier.
    pub id: PcuExecutorId,
    /// Human-readable executor name.
    pub name: &'static str,
    /// Coarse executor class.
    pub class: PcuExecutorClass,
    /// Provenance for this executor.
    pub origin: PcuExecutorOrigin,
}

/// Exclusive executor claim returned by one backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuExecutorClaim {
    pub(crate) executor: PcuExecutorId,
}

impl PcuExecutorClaim {
    /// Returns the claimed executor identifier.
    #[must_use]
    pub const fn executor(self) -> PcuExecutorId {
        self.executor
    }

    /// Back-compat alias while higher layers stop pretending every executor is a "device."
    #[must_use]
    pub const fn device(self) -> PcuExecutorId {
        self.executor()
    }
}

/// Back-compat alias while the higher layers stop saying “device” when they mean “executor.”
pub type PcuDeviceId = PcuExecutorId;

/// Back-compat alias while the higher layers stop saying “device” when they mean “executor.”
pub type PcuDeviceClass = PcuExecutorClass;

/// Back-compat alias while the higher layers stop saying “device” when they mean “executor.”
pub type PcuDeviceDescriptor = PcuExecutorDescriptor;

/// Back-compat alias while the higher layers stop saying “device” when they mean “executor.”
pub type PcuDeviceClaim = PcuExecutorClaim;
