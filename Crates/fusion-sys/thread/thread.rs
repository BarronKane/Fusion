//! fusion-sys-level thread wrappers built on top of fusion-pal-truthful backends.
//!
//! `fusion-sys::thread` is intentionally a thin policy layer over the fusion-pal thread contracts.
//! It owns handles, exposes a stable wrapper surface, and keeps platform selection below the
//! fusion-pal boundary where it belongs.

mod carrier;
mod handle;
mod pool;
mod system;
mod time;
/// Scheduler-adjacent vector-dispatch ownership contracts and wrappers.
pub mod vector;

pub use carrier::*;
pub use fusion_pal::sys::thread::{
    RawThreadEntry,
    ThreadAuthoritySet,
    ThreadBaseContract,
    ThreadClusterId,
    ThreadConfig,
    ThreadConstraintMode,
    ThreadCoreClassId,
    ThreadCoreId,
    ThreadEntryReturn,
    ThreadError,
    ThreadErrorKind,
    ThreadExecutionLocation,
    ThreadGuarantee,
    ThreadId,
    ThreadIdentityStability,
    ThreadImplementationKind,
    ThreadJoinPolicy,
    ThreadLifecycle,
    ThreadLifecycleCaps,
    ThreadLifecycleSupport,
    ThreadLocalityCaps,
    ThreadLocalitySupport,
    ThreadLogicalCpuId,
    ThreadMigrationPolicy,
    ThreadObservation,
    ThreadObservationControlContract,
    ThreadPlacementCaps,
    ThreadPlacementControlContract,
    ThreadPlacementOutcome,
    ThreadPlacementPhase,
    ThreadPlacementRequest,
    ThreadPlacementSupport,
    ThreadPlacementTarget,
    ThreadPriority,
    ThreadPriorityOrder,
    ThreadPriorityRange,
    ThreadProcessorGroupId,
    ThreadRunState,
    ThreadSchedulerCaps,
    ThreadSchedulerClass,
    ThreadSchedulerControlContract,
    ThreadSchedulerModel,
    ThreadSchedulerObservation,
    ThreadSchedulerRequest,
    ThreadSchedulerSupport,
    ThreadStackBacking,
    ThreadStackCaps,
    ThreadStackLocalityPolicy,
    ThreadStackLockPolicy,
    ThreadStackObservation,
    ThreadStackObservationControlContract,
    ThreadStackPrefaultPolicy,
    ThreadStackRequest,
    ThreadStackSupport,
    ThreadStartMode,
    ThreadSupport,
    ThreadSuspendControlContract,
    ThreadTermination,
    ThreadTerminationKind,
};

pub use handle::*;
pub use pool::*;
pub use system::*;
pub use time::*;
