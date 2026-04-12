//! Windows fusion-pal PCU executor backend.
//!
//! Hosted Windows has no truthful hardware queue-dispatch engine surfaced through this
//! backend, so we expose one synthetic CPU executor for contract compatibility. The
//! executor is explicit in metadata and never claims device-backed behavior.

use core::sync::atomic::{
    AtomicBool,
    Ordering,
};

use crate::contract::drivers::pcu::{
    PcuBaseContract,
    PcuCaps,
    PcuCommandOpCaps,
    PcuCommandSupport,
    PcuControlContract,
    PcuDispatchOpCaps,
    PcuDispatchPolicyCaps,
    PcuDispatchSupport,
    PcuError,
    PcuExecutorClaim,
    PcuExecutorClass,
    PcuExecutorDescriptor,
    PcuExecutorId,
    PcuExecutorOrigin,
    PcuExecutorSupport,
    PcuFeatureSupport,
    PcuPrimitiveCaps,
    PcuPrimitiveSupport,
    PcuSignalOpCaps,
    PcuSignalSupport,
    PcuStreamCapabilities,
    PcuStreamSupport,
    PcuSupport,
    PcuTransactionFeatureCaps,
    PcuTransactionSupport,
};

const HOST_CPU_EXECUTOR_ID: PcuExecutorId = PcuExecutorId(0);

const HOST_CPU_EXECUTOR_SUPPORT: PcuExecutorSupport = PcuExecutorSupport {
    primitives: PcuPrimitiveCaps::all(),
    dispatch_policy: PcuDispatchPolicyCaps::SERIAL
        .union(PcuDispatchPolicyCaps::PERSISTENT_INSTALL)
        .union(PcuDispatchPolicyCaps::CPU_FALLBACK)
        .union(PcuDispatchPolicyCaps::ORDERED_SUBMISSION),
    dispatch_instructions: PcuDispatchOpCaps::all(),
    stream_instructions: PcuStreamCapabilities::all(),
    command_instructions: PcuCommandOpCaps::all(),
    transaction_features: PcuTransactionFeatureCaps::all(),
    signal_instructions: PcuSignalOpCaps::all(),
};

static HOST_EXECUTORS: [PcuExecutorDescriptor; 1] = [PcuExecutorDescriptor {
    id: HOST_CPU_EXECUTOR_ID,
    name: "host-cpu",
    class: PcuExecutorClass::Cpu,
    origin: PcuExecutorOrigin::Synthetic,
    support: HOST_CPU_EXECUTOR_SUPPORT,
}];

static HOST_CPU_EXECUTOR_CLAIMED: AtomicBool = AtomicBool::new(false);

const HOST_PRIMITIVE_SUPPORT: PcuPrimitiveSupport = PcuPrimitiveSupport {
    primitives: PcuFeatureSupport::new(PcuPrimitiveCaps::all(), PcuPrimitiveCaps::all()),
};
const HOST_DISPATCH_SUPPORT: PcuDispatchSupport = PcuDispatchSupport {
    flags: PcuDispatchPolicyCaps::SERIAL
        .union(PcuDispatchPolicyCaps::PERSISTENT_INSTALL)
        .union(PcuDispatchPolicyCaps::CPU_FALLBACK)
        .union(PcuDispatchPolicyCaps::ORDERED_SUBMISSION),
    instructions: PcuFeatureSupport::new(PcuDispatchOpCaps::all(), PcuDispatchOpCaps::all()),
};
const HOST_STREAM_SUPPORT: PcuStreamSupport = PcuStreamSupport {
    instructions: PcuFeatureSupport::new(
        PcuStreamCapabilities::all(),
        PcuStreamCapabilities::all(),
    ),
};
const HOST_COMMAND_SUPPORT: PcuCommandSupport = PcuCommandSupport {
    instructions: PcuFeatureSupport::new(PcuCommandOpCaps::all(), PcuCommandOpCaps::all()),
};
const HOST_TRANSACTION_SUPPORT: PcuTransactionSupport = PcuTransactionSupport {
    features: PcuFeatureSupport::new(
        PcuTransactionFeatureCaps::all(),
        PcuTransactionFeatureCaps::all(),
    ),
};
const HOST_SIGNAL_SUPPORT: PcuSignalSupport = PcuSignalSupport {
    instructions: PcuFeatureSupport::new(PcuSignalOpCaps::all(), PcuSignalOpCaps::all()),
};

/// Windows generic PCU executor provider.
#[derive(Debug, Clone, Copy, Default)]
pub struct WindowsPcu;

/// Selected Windows programmable-IO provider type.
pub type PlatformPcu = WindowsPcu;

/// Returns the selected Windows programmable-IO provider.
#[must_use]
pub const fn system_pcu() -> PlatformPcu {
    PlatformPcu::new()
}

impl WindowsPcu {
    /// Creates a new Windows PCU provider handle.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl PcuBaseContract for WindowsPcu {
    fn support(&self) -> PcuSupport {
        PcuSupport {
            caps: PcuCaps::ENUMERATE_EXECUTORS
                | PcuCaps::CLAIM_EXECUTOR
                | PcuCaps::DISPATCH
                | PcuCaps::COMPLETION_STATUS
                | PcuCaps::EXTERNAL_RESOURCES,
            implementation: crate::contract::drivers::pcu::PcuImplementationKind::Native,
            executor_count: HOST_EXECUTORS.len() as u8,
            primitive_support: HOST_PRIMITIVE_SUPPORT,
            dispatch_support: HOST_DISPATCH_SUPPORT,
            stream_support: HOST_STREAM_SUPPORT,
            command_support: HOST_COMMAND_SUPPORT,
            transaction_support: HOST_TRANSACTION_SUPPORT,
            signal_support: HOST_SIGNAL_SUPPORT,
        }
    }

    fn executors(&self) -> &'static [PcuExecutorDescriptor] {
        &HOST_EXECUTORS
    }
}

impl PcuControlContract for WindowsPcu {
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
