//! Linux fusion-pal PCU executor backend.

use core::sync::atomic::{
    AtomicBool,
    Ordering,
};

use crate::contract::drivers::pcu::{
    PcuBase,
    PcuCaps,
    PcuControl,
    PcuError,
    PcuExecutorClaim,
    PcuExecutorClass,
    PcuExecutorDescriptor,
    PcuExecutorId,
    PcuExecutorOrigin,
    PcuSupport,
};

const HOST_CPU_EXECUTOR_ID: PcuExecutorId = PcuExecutorId(0);

static HOST_EXECUTORS: [PcuExecutorDescriptor; 1] = [PcuExecutorDescriptor {
    id: HOST_CPU_EXECUTOR_ID,
    name: "host-cpu",
    class: PcuExecutorClass::Cpu,
    origin: PcuExecutorOrigin::Synthetic,
}];

static HOST_CPU_EXECUTOR_CLAIMED: AtomicBool = AtomicBool::new(false);

/// Linux generic PCU executor provider.
#[derive(Debug, Clone, Copy, Default)]
pub struct LinuxPcu;

/// Selected Linux PCU provider type.
pub type PlatformPcu = LinuxPcu;

/// Returns the selected Linux PCU provider.
#[must_use]
pub const fn system_pcu() -> PlatformPcu {
    PlatformPcu::new()
}

impl LinuxPcu {
    /// Creates a new Linux PCU provider handle.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl PcuBase for LinuxPcu {
    fn support(&self) -> PcuSupport {
        PcuSupport {
            caps: PcuCaps::ENUMERATE_EXECUTORS
                | PcuCaps::CLAIM_EXECUTOR
                | PcuCaps::DISPATCH
                | PcuCaps::COMPLETION_STATUS
                | PcuCaps::EXTERNAL_RESOURCES,
            implementation: crate::contract::drivers::pcu::PcuImplementationKind::Native,
            executor_count: HOST_EXECUTORS.len() as u8,
        }
    }

    fn executors(&self) -> &'static [PcuExecutorDescriptor] {
        &HOST_EXECUTORS
    }
}

impl PcuControl for LinuxPcu {
    fn claim_executor(&self, executor: PcuExecutorId) -> Result<PcuExecutorClaim, PcuError> {
        if executor != HOST_CPU_EXECUTOR_ID {
            return Err(PcuError::invalid());
        }
        HOST_CPU_EXECUTOR_CLAIMED
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| PcuError::busy())?;
        Ok(PcuExecutorClaim { executor })
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
