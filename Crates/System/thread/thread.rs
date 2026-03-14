//! System-level thread wrappers built on top of PAL-truthful backends.
//!
//! `fusion-sys::thread` is intentionally a thin policy layer over the PAL thread contracts.
//! It owns handles, exposes a stable wrapper surface, and keeps platform selection below the
//! PAL boundary where it belongs.

mod handle;
mod pool;
mod system;

pub use handle::*;
pub use pool::*;
pub use system::*;

pub use fusion_pal::sys::thread::{
    RawThreadEntry, ThreadAuthoritySet, ThreadBase, ThreadClusterId, ThreadConfig,
    ThreadConstraintMode, ThreadCoreClassId, ThreadCoreId, ThreadEntryReturn, ThreadError,
    ThreadErrorKind, ThreadExecutionLocation, ThreadGuarantee, ThreadId, ThreadIdentityStability,
    ThreadJoinPolicy, ThreadLifecycle, ThreadLifecycleCaps, ThreadLifecycleSupport,
    ThreadLocalityCaps, ThreadLocalitySupport, ThreadLogicalCpuId, ThreadMigrationPolicy,
    ThreadObservation, ThreadObservationControl, ThreadPlacementCaps, ThreadPlacementControl,
    ThreadPlacementOutcome, ThreadPlacementPhase, ThreadPlacementRequest, ThreadPlacementSupport,
    ThreadPriority, ThreadPriorityOrder, ThreadPriorityRange, ThreadProcessorGroupId,
    ThreadRunState, ThreadSchedulerCaps, ThreadSchedulerClass, ThreadSchedulerControl,
    ThreadSchedulerModel, ThreadSchedulerObservation, ThreadSchedulerRequest,
    ThreadSchedulerSupport, ThreadStackBacking, ThreadStackCaps, ThreadStackLocalityPolicy,
    ThreadStackLockPolicy, ThreadStackObservation, ThreadStackObservationControl,
    ThreadStackPrefaultPolicy, ThreadStackRequest, ThreadStackSupport, ThreadStartMode,
    ThreadSupport, ThreadSuspendControl, ThreadTermination, ThreadTerminationKind,
};
