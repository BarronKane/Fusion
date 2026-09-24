//! iOS fusion-pal PCU executor backend.

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

/// iOS generic PCU executor provider.
#[derive(Debug, Clone, Copy, Default)]
pub struct IosPcu;

/// Selected iOS PCU provider type.
pub type PlatformPcu = IosPcu;

/// Returns the selected iOS PCU provider.
#[must_use]
pub const fn system_pcu() -> PlatformPcu {
    PlatformPcu::new()
}

impl IosPcu {
    /// Creates a new iOS PCU provider handle.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl PcuBaseContract for IosPcu {
    fn support(&self) -> PcuSupport {
        host_pcu_support()
    }

    fn executors(&self) -> &'static [crate::contract::drivers::pcu::PcuExecutorDescriptor] {
        &HOST_EXECUTORS
    }
}

impl PcuDirectStreamBackend for IosPcu {
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
