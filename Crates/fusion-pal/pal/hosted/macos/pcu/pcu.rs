//! macOS fusion-pal PCU executor backend.

use crate::contract::drivers::pcu::{
    PcuBaseContract,
    PcuDirectStreamBackend,
    PcuError,
    PcuInvocationBindings,
    PcuInvocationParameters,
    PcuStreamInstallation,
    PcuSupport,
};
use crate::pal::hosted::pcu_shared::{
    HostedCpuStreamHandle,
    host_cpu_executor_descriptor,
    host_pcu_support,
    install_host_cpu_stream,
};

static HOST_EXECUTORS: [crate::contract::drivers::pcu::PcuExecutorDescriptor; 1] =
    [host_cpu_executor_descriptor()];

/// macOS generic PCU executor provider.
#[derive(Debug, Clone, Copy, Default)]
pub struct MacOsPcu;

/// Selected macOS PCU provider type.
pub type PlatformPcu = MacOsPcu;

/// Returns the selected macOS PCU provider.
#[must_use]
pub const fn system_pcu() -> PlatformPcu {
    PlatformPcu::new()
}

impl MacOsPcu {
    /// Creates a new macOS PCU provider handle.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl PcuBaseContract for MacOsPcu {
    fn support(&self) -> PcuSupport {
        host_pcu_support()
    }

    fn executors(&self) -> &'static [crate::contract::drivers::pcu::PcuExecutorDescriptor] {
        &HOST_EXECUTORS
    }
}

impl PcuDirectStreamBackend for MacOsPcu {
    type StreamHandle = HostedCpuStreamHandle;

    fn install_stream_direct(
        &self,
        installation: PcuStreamInstallation<'_>,
        bindings: PcuInvocationBindings<'_>,
        parameters: PcuInvocationParameters<'_>,
    ) -> Result<Self::StreamHandle, PcuError> {
        install_host_cpu_stream(installation, bindings, parameters)
    }
}
