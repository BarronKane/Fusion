//! fusion-sys thread provider facade.

use core::time::Duration;

use fusion_pal::sys::mem::{
    MemBackingCaps,
    MemBaseContract,
    MemCaps,
    MemCatalogContract,
    MemCatalogCaps,
    MemResourceAttrs,
    system_mem,
};
use fusion_pal::sys::thread::{
    PlatformThread,
    system_thread as pal_system_thread,
};

use crate::alloc::{
    AllocError,
    AllocErrorKind,
    AllocPolicy,
    Allocator,
    AllocatorDomainId,
    ExtentLease,
    MemoryPoolExtentRequest,
};
use crate::mem::resource::{
    BoundMemoryResource,
    BoundResourceSpec,
    MemoryResourceHandle,
    ResourceError,
    ResourceInfo,
    ResourceState,
};
use crate::sync::{
    OnceInitError,
    OnceLock,
};
use crate::thread::handle::ThreadHandle;
use super::{
    RawThreadEntry,
    ThreadConfig,
    ThreadError,
    ThreadId,
    ThreadObservation,
    ThreadPlacementOutcome,
    ThreadPlacementRequest,
    ThreadPriorityRange,
    ThreadSchedulerClass,
    ThreadSchedulerObservation,
    ThreadSchedulerRequest,
    ThreadStackObservation,
    ThreadSupport,
    ThreadTermination,
};

/// Preferred runtime-backing realization reported by PAL/sys truth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RuntimeBackingPreference {
    /// The platform can readily plant runtime backing through its native virtual-memory story.
    PlatformAcquired,
    /// The platform should be bootstrapped from explicit caller-owned or board-owned regions.
    ExplicitBound,
    /// Both are plausible; higher layers should choose deliberately instead of guessing.
    Mixed,
}

/// Coarse runtime-construction truth surfaced from PAL/sys memory capabilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RuntimeConstructionSupport {
    /// `true` when the platform can actively acquire anonymous/runtime backing on demand.
    pub can_acquire_runtime_backing: bool,
    /// `true` when explicit pre-existing/bound regions are a first-class construction story.
    pub can_bind_explicit_backing: bool,
    /// `true` when PAL/sys can inventory owned memory regions that already exist on the machine.
    pub can_inventory_owned_regions: bool,
    /// Preferred backing realization for truthful runtime bootstrap on this platform.
    pub preferred_backing: RuntimeBackingPreference,
}

/// Error kind for explicit runtime-backing acquisition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RuntimeBackingErrorKind {
    /// Explicit runtime backing is unsupported on the selected platform.
    Unsupported,
    /// The request or selected platform resource was invalid.
    Invalid,
    /// The platform had no remaining runtime backing capacity.
    ResourceExhausted,
    /// The selected runtime-backing path hit a synchronization/state conflict.
    StateConflict,
}

/// Error returned when the system runtime-backing path cannot be realized honestly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RuntimeBackingError {
    kind: RuntimeBackingErrorKind,
}

impl RuntimeBackingError {
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            kind: RuntimeBackingErrorKind::Unsupported,
        }
    }

    #[must_use]
    pub const fn invalid() -> Self {
        Self {
            kind: RuntimeBackingErrorKind::Invalid,
        }
    }

    #[must_use]
    pub const fn resource_exhausted() -> Self {
        Self {
            kind: RuntimeBackingErrorKind::ResourceExhausted,
        }
    }

    #[must_use]
    pub const fn state_conflict() -> Self {
        Self {
            kind: RuntimeBackingErrorKind::StateConflict,
        }
    }

    #[must_use]
    pub const fn kind(self) -> RuntimeBackingErrorKind {
        self.kind
    }
}

impl From<ResourceError> for RuntimeBackingError {
    fn from(value: ResourceError) -> Self {
        match value.kind {
            crate::mem::resource::ResourceErrorKind::InvalidRequest
            | crate::mem::resource::ResourceErrorKind::InvalidRange => Self::invalid(),
            crate::mem::resource::ResourceErrorKind::OutOfMemory => Self::resource_exhausted(),
            crate::mem::resource::ResourceErrorKind::UnsupportedRequest
            | crate::mem::resource::ResourceErrorKind::UnsupportedOperation
            | crate::mem::resource::ResourceErrorKind::ContractViolation => Self::unsupported(),
            crate::mem::resource::ResourceErrorKind::Platform(_)
            | crate::mem::resource::ResourceErrorKind::SynchronizationFailure(_) => {
                Self::state_conflict()
            }
        }
    }
}

impl From<AllocError> for RuntimeBackingError {
    fn from(value: AllocError) -> Self {
        match value.kind {
            AllocErrorKind::InvalidRequest | AllocErrorKind::InvalidDomain => Self::invalid(),
            AllocErrorKind::Unsupported | AllocErrorKind::PolicyDenied => Self::unsupported(),
            AllocErrorKind::Busy | AllocErrorKind::SynchronizationFailure(_) => {
                Self::state_conflict()
            }
            AllocErrorKind::MetadataExhausted
            | AllocErrorKind::CapacityExhausted
            | AllocErrorKind::OutOfMemory
            | AllocErrorKind::ResourceFailure(_)
            | AllocErrorKind::PoolFailure(_) => Self::resource_exhausted(),
        }
    }
}

#[derive(Debug)]
struct RuntimeBackingAllocator {
    allocator: Allocator<1, 1>,
    domain: AllocatorDomainId,
    resource_info: ResourceInfo,
    resource_state: ResourceState,
}

impl RuntimeBackingAllocator {
    fn initialize() -> Result<Self, RuntimeBackingError> {
        let mem = system_mem();
        if !mem
            .catalog_support()
            .caps
            .contains(MemCatalogCaps::RESOURCE_INVENTORY)
        {
            return Err(RuntimeBackingError::unsupported());
        }

        let mut selected = None;
        for index in 0..mem.resource_count() {
            let Some(resource) = mem.resource(index) else {
                continue;
            };
            if resource.cpu_range.is_none()
                || resource.usable_now_len == 0
                || !resource
                    .envelope
                    .attrs
                    .contains(MemResourceAttrs::ALLOCATABLE)
            {
                continue;
            }
            if selected.as_ref().is_none_or(
                |current: &(usize, fusion_pal::sys::mem::MemCatalogResource)| {
                    resource.usable_now_len > current.1.usable_now_len
                },
            ) {
                selected = Some((index, resource));
            }
        }

        let (_, resource) = selected.ok_or_else(RuntimeBackingError::unsupported)?;
        let bound = BoundMemoryResource::from_catalog_resource(resource)?;
        let resolved = bound.resolved();
        let allocator = Allocator::<1, 1>::from_resource_with_policy(
            MemoryResourceHandle::from(bound),
            AllocPolicy::critical_safe(),
        )
        .map_err(RuntimeBackingError::from)?;
        let domain = allocator
            .default_domain()
            .ok_or_else(RuntimeBackingError::invalid)?;
        Ok(Self {
            allocator,
            domain,
            resource_info: resolved.info,
            resource_state: resolved.initial_state,
        })
    }

    fn allocate_slab(
        &self,
        bytes: usize,
        align: usize,
    ) -> Result<OwnedRuntimeSlab, RuntimeBackingError> {
        let lease = self
            .allocator
            .extent(self.domain, MemoryPoolExtentRequest { len: bytes, align })
            .map_err(RuntimeBackingError::from)?;
        let spec = BoundResourceSpec {
            range: lease.region(),
            domain: self.resource_info.domain,
            backing: crate::mem::resource::ResourceBackingKind::Partition,
            attrs: self.resource_info.attrs,
            geometry: self.resource_info.geometry,
            layout: self.resource_info.layout,
            contract: self.resource_info.contract,
            support: self.resource_info.support,
            additional_hazards: self.resource_info.hazards,
            initial_state: self.resource_state,
        };
        let handle = MemoryResourceHandle::from(
            BoundMemoryResource::new(spec).map_err(RuntimeBackingError::from)?,
        );
        Ok(OwnedRuntimeSlab { handle, lease })
    }
}

/// One exact owned runtime slab leased from the system-selected explicit-backing domain.
#[derive(Debug)]
pub struct OwnedRuntimeSlab {
    /// Partitionable runtime-backing view for higher-level runtime construction.
    pub handle: MemoryResourceHandle,
    /// Lease that keeps the backing region alive until dropped.
    pub lease: ExtentLease,
}

static CURRENT_RUNTIME_ALLOCATOR: OnceLock<RuntimeBackingAllocator> = OnceLock::new();

/// Returns the coarse runtime-construction truth for the active platform.
#[must_use]
pub fn system_runtime_construction_support() -> RuntimeConstructionSupport {
    let mem = system_mem();
    let support = mem.support();
    let catalog = mem.catalog_support();
    let can_acquire_runtime_backing = support.caps.contains(MemCaps::MAP_ANON)
        && support
            .backings
            .intersects(MemBackingCaps::ANON_PRIVATE | MemBackingCaps::ANON_SHARED);
    let can_inventory_owned_regions = catalog.caps.contains(MemCatalogCaps::RESOURCE_INVENTORY);
    let can_bind_explicit_backing =
        can_inventory_owned_regions || support.backings.contains(MemBackingCaps::BORROWED);
    let preferred_backing = match (can_acquire_runtime_backing, can_bind_explicit_backing) {
        (true, false) => RuntimeBackingPreference::PlatformAcquired,
        (false, true) => RuntimeBackingPreference::ExplicitBound,
        (true, true) => RuntimeBackingPreference::Mixed,
        (false, false) => RuntimeBackingPreference::ExplicitBound,
    };

    RuntimeConstructionSupport {
        can_acquire_runtime_backing,
        can_bind_explicit_backing,
        can_inventory_owned_regions,
        preferred_backing,
    }
}

/// Returns whether the active platform should prefer explicit bound regions for runtime backing.
#[must_use]
pub fn uses_explicit_bound_runtime_backing() -> bool {
    let support = system_runtime_construction_support();
    !support.can_acquire_runtime_backing
        && support.can_bind_explicit_backing
        && matches!(
            support.preferred_backing,
            RuntimeBackingPreference::ExplicitBound | RuntimeBackingPreference::Mixed
        )
}

/// Attempts to lease one exact owned runtime slab from the system-selected explicit-backing lane.
///
/// Returns `Ok(None)` when the current platform should not use the explicit-bound runtime path.
///
/// # Errors
///
/// Returns any honest catalog, allocator, or synchronization failure while realizing the slab.
pub fn allocate_owned_runtime_slab(
    bytes: usize,
    align: usize,
) -> Result<Option<OwnedRuntimeSlab>, RuntimeBackingError> {
    if !uses_explicit_bound_runtime_backing() {
        return Ok(None);
    }
    let allocator = CURRENT_RUNTIME_ALLOCATOR
        .get_or_try_init(RuntimeBackingAllocator::initialize)
        .map_err(|error| match error {
            OnceInitError::Sync(_) => RuntimeBackingError::state_conflict(),
            OnceInitError::Init(error) => error,
        })?;
    allocator.allocate_slab(bytes, align).map(Some)
}

/// fusion-sys thread provider wrapper around the selected fusion-pal backend.
#[derive(Debug, Clone, Copy)]
pub struct ThreadSystem {
    inner: PlatformThread,
}

impl ThreadSystem {
    /// Creates a wrapper for the selected platform thread provider.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            inner: pal_system_thread(),
        }
    }

    /// Reports the supported thread surface.
    #[must_use]
    pub fn support(&self) -> ThreadSupport {
        fusion_pal::sys::thread::ThreadBaseContract::support(&self.inner)
    }

    /// Returns the coarse runtime-construction truth for the active platform.
    #[must_use]
    pub fn runtime_construction_support(&self) -> RuntimeConstructionSupport {
        system_runtime_construction_support()
    }

    /// Spawns a thread using the raw fusion-pal-level entry signature.
    ///
    /// # Safety
    ///
    /// The caller must ensure the raw entry and opaque context uphold the fusion-pal thread
    /// contract for the selected backend.
    ///
    /// # Errors
    ///
    /// Returns any honest backend thread-creation failure, including lifecycle, scheduler,
    /// placement, or stack-policy rejection.
    pub unsafe fn spawn_raw(
        &self,
        config: &ThreadConfig<'_>,
        entry: RawThreadEntry,
        context: *mut (),
    ) -> Result<ThreadHandle, ThreadError> {
        // SAFETY: the caller upholds the raw fusion-pal spawn contract.
        let handle = unsafe {
            fusion_pal::sys::thread::ThreadLifecycle::spawn(&self.inner, config, entry, context)?
        };
        Ok(ThreadHandle::new(handle))
    }

    /// Returns the identifier of the current thread.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot surface a stable current-thread identifier.
    pub fn current_thread_id(&self) -> Result<ThreadId, ThreadError> {
        fusion_pal::sys::thread::ThreadLifecycle::current_thread_id(&self.inner)
    }

    /// Joins a joinable thread and returns its termination record.
    ///
    /// # Errors
    ///
    /// Returns an error if the handle is detached, invalid, or the backend cannot complete
    /// the join honestly.
    #[allow(clippy::needless_pass_by_value)]
    pub fn join(&self, handle: ThreadHandle) -> Result<ThreadTermination, ThreadError> {
        let ThreadHandle { inner } = handle;
        fusion_pal::sys::thread::ThreadLifecycle::join(&self.inner, inner)
    }

    /// Detaches a thread handle.
    ///
    /// # Errors
    ///
    /// Returns an error if the handle is not detachable or the backend cannot detach it
    /// honestly.
    #[allow(clippy::needless_pass_by_value)]
    pub fn detach(&self, handle: ThreadHandle) -> Result<(), ThreadError> {
        let ThreadHandle { inner } = handle;
        fusion_pal::sys::thread::ThreadLifecycle::detach(&self.inner, inner)
    }

    /// Suspends a thread when the backend supports it.
    ///
    /// # Errors
    ///
    /// Returns an error when the backend cannot suspend the handle honestly or does not
    /// support suspension at all.
    pub fn suspend(&self, handle: &ThreadHandle) -> Result<(), ThreadError> {
        fusion_pal::sys::thread::ThreadSuspendControlContract::suspend(&self.inner, &handle.inner)
    }

    /// Resumes a suspended thread when the backend supports it.
    ///
    /// # Errors
    ///
    /// Returns an error when the backend cannot resume the handle honestly or does not
    /// support resume at all.
    pub fn resume(&self, handle: &ThreadHandle) -> Result<(), ThreadError> {
        fusion_pal::sys::thread::ThreadSuspendControlContract::resume(&self.inner, &handle.inner)
    }

    /// Queries the class-specific numeric priority range.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot observe the requested class range honestly.
    pub fn priority_range(
        &self,
        class: ThreadSchedulerClass,
    ) -> Result<Option<ThreadPriorityRange>, ThreadError> {
        fusion_pal::sys::thread::ThreadSchedulerControlContract::priority_range(&self.inner, class)
    }

    /// Applies scheduler policy to a thread handle.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot apply or honestly degrade the requested
    /// scheduler policy.
    pub fn set_scheduler(
        &self,
        handle: &ThreadHandle,
        request: &ThreadSchedulerRequest,
    ) -> Result<ThreadSchedulerObservation, ThreadError> {
        fusion_pal::sys::thread::ThreadSchedulerControlContract::set_scheduler(
            &self.inner,
            &handle.inner,
            request,
        )
    }

    /// Queries the effective scheduler policy for a thread handle.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot observe the effective scheduler state.
    pub fn scheduler(
        &self,
        handle: &ThreadHandle,
    ) -> Result<ThreadSchedulerObservation, ThreadError> {
        fusion_pal::sys::thread::ThreadSchedulerControlContract::scheduler(
            &self.inner,
            &handle.inner,
        )
    }

    /// Yields the current thread to the scheduler.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot honestly yield the current thread.
    pub fn yield_now(&self) -> Result<(), ThreadError> {
        fusion_pal::sys::thread::ThreadSchedulerControlContract::yield_now(&self.inner)
    }

    /// Sleeps the current thread for a relative duration.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot honestly sleep for the requested duration.
    pub fn sleep_for(&self, duration: Duration) -> Result<(), ThreadError> {
        fusion_pal::sys::thread::ThreadSchedulerControlContract::sleep_for(&self.inner, duration)
    }

    /// Returns the current backend monotonic time.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot surface a truthful monotonic timestamp.
    pub fn monotonic_now(&self) -> Result<Duration, ThreadError> {
        fusion_pal::sys::thread::ThreadSchedulerControlContract::monotonic_now(&self.inner)
    }

    /// Applies placement policy to a thread handle.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot apply or honestly degrade the requested
    /// placement policy.
    pub fn set_placement(
        &self,
        handle: &ThreadHandle,
        request: &ThreadPlacementRequest<'_>,
    ) -> Result<ThreadPlacementOutcome, ThreadError> {
        fusion_pal::sys::thread::ThreadPlacementControlContract::set_placement(
            &self.inner,
            &handle.inner,
            request,
        )
    }

    /// Queries effective placement for a thread handle.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot observe the effective placement honestly.
    pub fn placement(&self, handle: &ThreadHandle) -> Result<ThreadPlacementOutcome, ThreadError> {
        fusion_pal::sys::thread::ThreadPlacementControlContract::placement(
            &self.inner,
            &handle.inner,
        )
    }

    /// Observes the current thread.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot produce a truthful current-thread observation.
    pub fn observe_current(&self) -> Result<ThreadObservation, ThreadError> {
        fusion_pal::sys::thread::ThreadObservationControlContract::observe_current(&self.inner)
    }

    /// Observes a specific thread handle.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot observe the requested handle honestly.
    pub fn observe(&self, handle: &ThreadHandle) -> Result<ThreadObservation, ThreadError> {
        fusion_pal::sys::thread::ThreadObservationControlContract::observe(
            &self.inner,
            &handle.inner,
        )
    }

    /// Observes stack information for the current thread.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot observe current-thread stack state honestly.
    pub fn observe_current_stack(&self) -> Result<ThreadStackObservation, ThreadError> {
        fusion_pal::sys::thread::ThreadStackObservationControlContract::observe_current_stack(
            &self.inner,
        )
    }

    /// Observes stack information for a specific thread handle.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot observe the requested handle's stack state
    /// honestly.
    pub fn observe_stack(
        &self,
        handle: &ThreadHandle,
    ) -> Result<ThreadStackObservation, ThreadError> {
        fusion_pal::sys::thread::ThreadStackObservationControlContract::observe_stack(
            &self.inner,
            &handle.inner,
        )
    }
}

impl Default for ThreadSystem {
    fn default() -> Self {
        Self::new()
    }
}

/// Returns the process-wide system thread provider wrapper.
#[must_use]
pub const fn system_thread() -> ThreadSystem {
    ThreadSystem::new()
}
