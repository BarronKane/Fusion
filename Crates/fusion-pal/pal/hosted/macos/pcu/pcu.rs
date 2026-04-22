//! macOS fusion-pal PCU executor backend.

use core::sync::atomic::{
    AtomicBool,
    Ordering,
};

use crate::contract::drivers::pcu::{
    PcuBaseContract,
    PcuCommandSubmission,
    PcuControlContract,
    PcuDirectDispatchBackend,
    PcuError,
    PcuExecutorClaim,
    PcuExecutorId,
    PcuInvocationBindings,
    PcuInvocationParameters,
    PcuSignalInstallation,
    PcuStreamInstallation,
    PcuSupport,
    PcuTransactionSubmission,
};
use crate::pal::hosted::pcu_shared::{
    HOST_CPU_EXECUTOR_ID,
    HostedCpuStreamHandle,
    HostedCpuUnsupportedFiniteHandle,
    HostedCpuUnsupportedPersistentHandle,
    host_cpu_executor_descriptor,
    host_pcu_support,
    install_host_cpu_stream,
};

static HOST_EXECUTORS: [crate::contract::drivers::pcu::PcuExecutorDescriptor; 1] =
    [host_cpu_executor_descriptor()];
static HOST_CPU_EXECUTOR_CLAIMED: AtomicBool = AtomicBool::new(false);

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

impl PcuDirectDispatchBackend for MacOsPcu {
    type DispatchHandle = HostedCpuUnsupportedFiniteHandle;
    type CommandHandle = HostedCpuUnsupportedFiniteHandle;
    type TransactionHandle = HostedCpuUnsupportedFiniteHandle;
    type StreamHandle = HostedCpuStreamHandle;
    type SignalHandle = HostedCpuUnsupportedPersistentHandle;

    fn submit_dispatch_direct(
        &self,
        _submission: crate::contract::drivers::pcu::PcuDispatchSubmission<'_>,
        _bindings: PcuInvocationBindings<'_>,
        _parameters: PcuInvocationParameters<'_>,
    ) -> Result<Self::DispatchHandle, PcuError> {
        Err(PcuError::unsupported())
    }

    fn submit_command_direct(
        &self,
        _submission: PcuCommandSubmission<'_>,
        _parameters: PcuInvocationParameters<'_>,
    ) -> Result<Self::CommandHandle, PcuError> {
        Err(PcuError::unsupported())
    }

    fn submit_transaction_direct(
        &self,
        _submission: PcuTransactionSubmission<'_>,
        _bindings: PcuInvocationBindings<'_>,
        _parameters: PcuInvocationParameters<'_>,
    ) -> Result<Self::TransactionHandle, PcuError> {
        Err(PcuError::unsupported())
    }

    fn install_stream_direct(
        &self,
        installation: PcuStreamInstallation<'_>,
        bindings: PcuInvocationBindings<'_>,
        parameters: PcuInvocationParameters<'_>,
    ) -> Result<Self::StreamHandle, PcuError> {
        install_host_cpu_stream(installation, bindings, parameters)
    }

    fn install_signal_direct(
        &self,
        _installation: PcuSignalInstallation<'_>,
        _parameters: PcuInvocationParameters<'_>,
    ) -> Result<Self::SignalHandle, PcuError> {
        Err(PcuError::unsupported())
    }
}

impl PcuControlContract for MacOsPcu {
    fn claim_executor(&self, executor: PcuExecutorId) -> Result<PcuExecutorClaim, PcuError> {
        if executor != HOST_CPU_EXECUTOR_ID {
            return Err(PcuError::invalid());
        }
        HOST_CPU_EXECUTOR_CLAIMED
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| PcuError::busy())?;
        Ok(PcuExecutorClaim::new(executor))
    }

    fn release_executor(&self, claim: PcuExecutorClaim) -> Result<(), PcuError> {
        if claim.executor() != HOST_CPU_EXECUTOR_ID {
            return Err(PcuError::invalid());
        }
        if !HOST_CPU_EXECUTOR_CLAIMED.swap(false, Ordering::AcqRel) {
            return Err(PcuError::state_conflict());
        }
        Ok(())
    }
}
