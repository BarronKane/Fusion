#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CurrentGreenYieldAction {
    Requeue,
    WaitReadiness {
        source: EventSourceHandle,
        interest: EventInterest,
    },
}

include!("scheduler.rs");

struct CurrentGreenContext {
    inner: *const GreenPoolInner,
    slot_index: usize,
    id: u64,
    fiber_id: FiberId,
    courier_id: Option<CourierId>,
    context_id: Option<ContextId>,
}

fn current_green_slot() -> Option<&'static GreenTaskSlot> {
    let context = system_fiber_context().ok()?;
    let slot = context.cast::<GreenTaskSlot>();
    if slot.is_null() {
        return None;
    }
    Some(unsafe { &*slot })
}

fn current_green_context() -> Option<CurrentGreenContext> {
    current_green_slot()?.current_context().ok()
}

include!("query.rs");
include!("errors.rs");

#[doc(hidden)]
#[derive(Clone, Copy)]
pub struct CooperativeGreenLockToken {
    slot: *const (),
    depth_index: usize,
}

impl CooperativeGreenLockToken {
    const fn inactive() -> Self {
        Self {
            slot: core::ptr::null(),
            depth_index: 0,
        }
    }
}

#[doc(hidden)]
#[must_use]
pub fn is_in_green_context() -> bool {
    current_green_context().is_some()
}

/// Enters one cooperative green lock scope for the current running fiber, if any.
///
/// # Errors
///
/// Returns an honest synchronization error when ranked acquisition would violate cooperative lock
/// ordering or when the per-fiber lock nesting budget is exhausted.
pub fn enter_current_green_cooperative_lock(
    rank: Option<u16>,
    span: Option<CooperativeExclusionSpan>,
) -> Result<CooperativeGreenLockToken, SyncError> {
    let Some(slot) = current_green_slot() else {
        return Ok(CooperativeGreenLockToken::inactive());
    };
    slot.enter_cooperative_lock(rank, span)
}

pub fn exit_current_green_cooperative_lock(token: CooperativeGreenLockToken) {
    if token.slot.is_null() {
        return;
    }
    let slot = token.slot.cast::<GreenTaskSlot>();
    unsafe { &*slot }.exit_cooperative_lock(token.depth_index);
}

#[doc(hidden)]
#[must_use]
pub fn current_green_cooperative_lock_depth() -> usize {
    current_green_slot().map_or(0, GreenTaskSlot::cooperative_lock_depth)
}

/// Copies the currently active green exclusion spans into `output` and returns how many were
/// copied.
#[must_use]
pub fn current_green_exclusion_spans(output: &mut [CooperativeExclusionSpan]) -> usize {
    current_green_slot().map_or(0, |slot| slot.copy_active_exclusion_spans(output))
}

/// Returns whether all `required_clear_spans` are currently clear in the active green context.
///
/// This is the neutral eligibility predicate surface for urgent inline admission: callers may use
/// it to decide whether an inline path can run now or should fall back to deferred dispatch.
#[must_use]
pub fn current_green_exclusion_allows(required_clear_spans: &[CooperativeExclusionSpan]) -> bool {
    if required_clear_spans.is_empty() {
        return true;
    }
    let Some(slot) = current_green_slot() else {
        return true;
    };

    let depth = slot.cooperative_lock_depth();
    for index in 0..depth {
        let raw = slot.cooperative_exclusion_spans[index].load(Ordering::Acquire);
        let Some(active) = NonZeroU16::new(raw).map(CooperativeExclusionSpan) else {
            continue;
        };
        if required_clear_spans.contains(&active) {
            return false;
        }
    }
    true
}

/// Returns whether all exclusion spans present in one required-clear summary tree are currently
/// clear in the active green context.
#[must_use]
pub fn current_green_exclusion_allows_tree(tree: &CooperativeExclusionSummaryTree) -> bool {
    let Some(slot) = current_green_slot() else {
        return true;
    };
    slot.exclusion_summary_tree_allows(tree)
}

/// Enters one named cooperative exclusion span for the current running green context.
///
/// # Errors
///
/// Returns an honest synchronization error when the exclusion nesting budget is exhausted.
pub fn enter_current_green_exclusion_span(
    span: CooperativeExclusionSpan,
) -> Result<CooperativeExclusionGuard, SyncError> {
    enter_current_green_cooperative_lock(None, Some(span))
        .map(|token| CooperativeExclusionGuard { token })
}

/// RAII guard for one current green exclusion span.
#[must_use]
pub struct CooperativeExclusionGuard {
    token: CooperativeGreenLockToken,
}

impl Drop for CooperativeExclusionGuard {
    fn drop(&mut self) {
        exit_current_green_cooperative_lock(self.token);
    }
}

fn ensure_current_green_handoff_unlocked() -> Result<(), FiberError> {
    if current_green_cooperative_lock_depth() != 0 {
        return Err(FiberError::state_conflict());
    }
    Ok(())
}

fn set_current_green_yield_action(action: CurrentGreenYieldAction) {
    if let Some(slot) = current_green_slot() {
        let _ = slot.set_yield_action(action);
    }
}

fn take_current_green_yield_action(
    inner: &GreenPoolInner,
    slot_index: usize,
) -> Result<CurrentGreenYieldAction, FiberError> {
    inner.tasks.slot(slot_index)?.take_yield_action()
}

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
struct GreenPoolMetadataHeader {
    metadata_len: usize,
    carrier_count: usize,
    task_capacity: usize,
    reactor_enabled: bool,
}

#[derive(Debug)]
struct GreenPoolMetadata {
    mapping: Region,
    tasks: MetadataSlice<GreenTaskSlot>,
    initialized_tasks: usize,
    carriers: MetadataSlice<CarrierQueue>,
    carrier_contexts: MetadataSlice<CarrierLoopContext>,
    initialized_carriers: usize,
}

#[derive(Debug, Clone, Copy)]
struct CarrierLoopContext {
    control: NonNull<GreenPoolControlBlock>,
    carrier_index: usize,
}

impl GreenPoolMetadata {
    fn new_in_region(
        mapping: Region,
        carrier_count: usize,
        task_capacity: usize,
        scheduling: GreenScheduling,
        priority_age_cap: Option<FiberTaskAgeCap>,
        reactor_enabled: bool,
        fast: bool,
    ) -> Result<(Self, GreenTaskRegistry, MetadataSlice<CarrierQueue>), FiberError> {
        if carrier_count == 0 || task_capacity == 0 {
            return Err(FiberError::invalid());
        }

        let mut metadata = Self {
            mapping,
            tasks: MetadataSlice::empty(),
            initialized_tasks: 0,
            carriers: MetadataSlice::empty(),
            carrier_contexts: MetadataSlice::empty(),
            initialized_carriers: 0,
        };
        let result = Self::initialize_into(
            &mut metadata,
            carrier_count,
            task_capacity,
            scheduling,
            priority_age_cap,
            reactor_enabled,
            fast,
        );
        match result {
            Ok((tasks, carriers)) => Ok((metadata, tasks, carriers)),
            Err(error) => Err(error),
        }
    }

    fn metadata_bytes(
        carrier_count: usize,
        task_capacity: usize,
        scheduling: GreenScheduling,
        reactor_enabled: bool,
        final_align: usize,
    ) -> Result<usize, FiberError> {
        let mut bytes = size_of::<GreenPoolMetadataHeader>();
        bytes = fiber_align_up(bytes, align_of::<GreenTaskSlot>())?;
        bytes = bytes
            .checked_add(
                size_of::<GreenTaskSlot>()
                    .checked_mul(task_capacity)
                    .ok_or_else(FiberError::resource_exhausted)?,
            )
            .ok_or_else(FiberError::resource_exhausted)?;
        bytes = fiber_align_up(bytes, align_of::<usize>())?;
        bytes = bytes
            .checked_add(
                size_of::<usize>()
                    .checked_mul(task_capacity)
                    .ok_or_else(FiberError::resource_exhausted)?,
            )
            .ok_or_else(FiberError::resource_exhausted)?;
        bytes = fiber_align_up(bytes, align_of::<CarrierQueue>())?;
        bytes = bytes
            .checked_add(
                size_of::<CarrierQueue>()
                    .checked_mul(carrier_count)
                    .ok_or_else(FiberError::resource_exhausted)?,
            )
            .ok_or_else(FiberError::resource_exhausted)?;
        #[cfg(feature = "std")]
        {
            bytes = fiber_align_up(bytes, align_of::<CarrierLoopContext>())?;
            bytes = bytes
                .checked_add(
                    size_of::<CarrierLoopContext>()
                        .checked_mul(carrier_count)
                        .ok_or_else(FiberError::resource_exhausted)?,
                )
                .ok_or_else(FiberError::resource_exhausted)?;
        }

        for _ in 0..carrier_count {
            match scheduling {
                GreenScheduling::Fifo | GreenScheduling::WorkStealing => {
                    bytes = fiber_align_up(bytes, align_of::<usize>())?;
                    bytes = bytes
                        .checked_add(
                            size_of::<usize>()
                                .checked_mul(task_capacity)
                                .ok_or_else(FiberError::resource_exhausted)?,
                        )
                        .ok_or_else(FiberError::resource_exhausted)?;
                }
                GreenScheduling::Priority => {
                    bytes = fiber_align_up(bytes, align_of::<PriorityBucket>())?;
                    bytes = bytes
                        .checked_add(
                            size_of::<PriorityBucket>()
                                .checked_mul(FIBER_PRIORITY_LEVELS)
                                .ok_or_else(FiberError::resource_exhausted)?,
                        )
                        .ok_or_else(FiberError::resource_exhausted)?;
                    bytes = fiber_align_up(bytes, align_of::<usize>())?;
                    bytes = bytes
                        .checked_add(
                            size_of::<usize>()
                                .checked_mul(task_capacity)
                                .ok_or_else(FiberError::resource_exhausted)?,
                        )
                        .ok_or_else(FiberError::resource_exhausted)?;
                    bytes = fiber_align_up(bytes, align_of::<i8>())?;
                    bytes = bytes
                        .checked_add(task_capacity)
                        .ok_or_else(FiberError::resource_exhausted)?;
                    bytes = fiber_align_up(bytes, align_of::<u64>())?;
                    bytes = bytes
                        .checked_add(
                            size_of::<u64>()
                                .checked_mul(task_capacity)
                                .ok_or_else(FiberError::resource_exhausted)?,
                        )
                        .ok_or_else(FiberError::resource_exhausted)?;
                }
            }
            if reactor_enabled {
                bytes = fiber_align_up(bytes, align_of::<Option<CarrierWaiterRecord>>())?;
                bytes = bytes
                    .checked_add(
                        size_of::<Option<CarrierWaiterRecord>>()
                            .checked_mul(task_capacity)
                            .ok_or_else(FiberError::resource_exhausted)?,
                    )
                    .ok_or_else(FiberError::resource_exhausted)?;
            }
        }

        fiber_align_up(bytes, final_align)
    }

    fn initialize_into(
        metadata: &mut Self,
        carrier_count: usize,
        task_capacity: usize,
        scheduling: GreenScheduling,
        priority_age_cap: Option<FiberTaskAgeCap>,
        reactor_enabled: bool,
        fast: bool,
    ) -> Result<(GreenTaskRegistry, MetadataSlice<CarrierQueue>), FiberError> {
        let mut cursor = MetadataCursor::new(metadata.mapping);
        let header_slice = cursor.reserve_slice::<GreenPoolMetadataHeader>(1)?;
        let task_slots = cursor.reserve_slice::<GreenTaskSlot>(task_capacity)?;
        let free_entries = cursor.reserve_slice::<usize>(task_capacity)?;
        let carriers = cursor.reserve_slice::<CarrierQueue>(carrier_count)?;
        let carrier_contexts = cursor.reserve_slice::<CarrierLoopContext>(carrier_count)?;
        metadata.tasks = task_slots;
        metadata.carriers = carriers;
        metadata.carrier_contexts = carrier_contexts;

        let header = GreenPoolMetadataHeader {
            metadata_len: metadata.mapping.len,
            carrier_count,
            task_capacity,
            reactor_enabled,
        };
        unsafe {
            header_slice.write(0, header)?;
        }

        let tasks = GreenTaskRegistry::new(task_slots, free_entries, fast)?;
        metadata.initialized_tasks = task_slots.len();

        for carrier_index in 0..carrier_count {
            let (
                queue_entries,
                priority_buckets,
                priority_next,
                priority_values,
                priority_enqueue_epochs,
            ) = match scheduling {
                GreenScheduling::Fifo | GreenScheduling::WorkStealing => (
                    Some(cursor.reserve_slice::<usize>(task_capacity)?),
                    None,
                    None,
                    None,
                    None,
                ),
                GreenScheduling::Priority => (
                    None,
                    Some(cursor.reserve_slice::<PriorityBucket>(FIBER_PRIORITY_LEVELS)?),
                    Some(cursor.reserve_slice::<usize>(task_capacity)?),
                    Some(cursor.reserve_slice::<i8>(task_capacity)?),
                    Some(cursor.reserve_slice::<u64>(task_capacity)?),
                ),
            };
            let waiters = if reactor_enabled {
                Some(cursor.reserve_slice::<Option<CarrierWaiterRecord>>(task_capacity)?)
            } else {
                None
            };
            let queue = CarrierQueue::new(
                scheduling,
                CarrierQueueSlices {
                    queue_entries,
                    priority_buckets,
                    priority_next,
                    priority_values,
                    priority_enqueue_epochs,
                    waiters,
                },
                priority_age_cap,
                initial_steal_seed(carrier_index),
                fast,
            )?;
            unsafe {
                carriers.write(carrier_index, queue)?;
            }
            metadata.initialized_carriers += 1;
        }

        Ok((tasks, carriers))
    }

    fn initialize_carrier_contexts(
        &self,
        control: NonNull<GreenPoolControlBlock>,
    ) -> Result<(), FiberError> {
        for carrier_index in 0..self.carrier_contexts.len() {
            unsafe {
                self.carrier_contexts.write(
                    carrier_index,
                    CarrierLoopContext {
                        control,
                        carrier_index,
                    },
                )?;
            }
        }
        Ok(())
    }
}

impl Drop for GreenPoolMetadata {
    fn drop(&mut self) {
        for index in 0..self.initialized_carriers {
            unsafe {
                self.carriers.ptr.as_ptr().add(index).drop_in_place();
            }
        }
        for index in 0..self.initialized_tasks {
            unsafe {
                self.tasks.ptr.as_ptr().add(index).drop_in_place();
            }
        }
    }
}

#[repr(C)]
#[derive(Debug)]
enum GreenPoolControlBacking {
    VirtualCachedRegion(Region),
    Owned {
        control: MemoryResourceHandle,
        metadata: MemoryResourceHandle,
        slab_owner: Option<fusion_sys::alloc::ExtentLease>,
    },
}

#[repr(C)]
struct GreenPoolControlBlock {
    header: SharedHeader,
    region: Region,
    backing: ManuallyDrop<GreenPoolControlBacking>,
    metadata: ManuallyDrop<GreenPoolMetadata>,
    inner: GreenPoolInner,
}

struct GreenPoolLease {
    ptr: NonNull<GreenPoolControlBlock>,
}

unsafe impl Send for GreenPoolLease {}
unsafe impl Sync for GreenPoolLease {}

impl fmt::Debug for GreenPoolLease {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GreenPoolLease")
            .field("ptr", &self.ptr)
            .finish_non_exhaustive()
    }
}

impl GreenPoolLease {
    fn new(
        region: Region,
        inner: GreenPoolInner,
        metadata: GreenPoolMetadata,
    ) -> Result<Self, FiberError> {
        if region.len < size_of::<GreenPoolControlBlock>()
            || !region
                .base
                .get()
                .is_multiple_of(align_of::<GreenPoolControlBlock>())
        {
            let _ = unsafe { system_mem().unmap(region) };
            return Err(FiberError::invalid());
        }

        let ptr = core::ptr::NonNull::new(region.base.cast::<GreenPoolControlBlock>())
            .ok_or_else(FiberError::invalid)?;
        // SAFETY: the control mapping is uniquely owned here, properly aligned, and large enough
        // to host exactly one green-pool control block.
        unsafe {
            ptr.as_ptr().write(GreenPoolControlBlock {
                header: SharedHeader::new(),
                region,
                backing: ManuallyDrop::new(GreenPoolControlBacking::VirtualCachedRegion(region)),
                metadata: ManuallyDrop::new(metadata),
                inner,
            });
        }
        Ok(Self { ptr })
    }

    fn new_with_backing(
        control: MemoryResourceHandle,
        metadata_resource: MemoryResourceHandle,
        slab_owner: Option<fusion_sys::alloc::ExtentLease>,
        inner: GreenPoolInner,
        metadata: GreenPoolMetadata,
    ) -> Result<Self, FiberError> {
        let region = unsafe { control.view().raw_region() };
        if region.len < size_of::<GreenPoolControlBlock>()
            || !region
                .base
                .get()
                .is_multiple_of(align_of::<GreenPoolControlBlock>())
        {
            return Err(FiberError::invalid());
        }

        let ptr = core::ptr::NonNull::new(region.base.cast::<GreenPoolControlBlock>())
            .ok_or_else(FiberError::invalid)?;
        unsafe {
            ptr.as_ptr().write(GreenPoolControlBlock {
                header: SharedHeader::new(),
                region,
                backing: ManuallyDrop::new(GreenPoolControlBacking::Owned {
                    control,
                    metadata: metadata_resource,
                    slab_owner,
                }),
                metadata: ManuallyDrop::new(metadata),
                inner,
            });
        }
        Ok(Self { ptr })
    }

    fn try_clone(&self) -> Result<Self, FiberError> {
        self.block()
            .header
            .try_retain()
            .map_err(fiber_error_from_sync)?;
        Ok(Self { ptr: self.ptr })
    }

    const fn as_ptr(&self) -> *const GreenPoolInner {
        core::ptr::from_ref(&self.block().inner)
    }

    const fn block(&self) -> &GreenPoolControlBlock {
        // SAFETY: a live lease always points at a live green-pool control block.
        unsafe { self.ptr.as_ref() }
    }

    fn memory_footprint(&self) -> FiberPoolMemoryFootprint {
        let block = self.block();
        FiberPoolMemoryFootprint {
            carrier_count: block.inner.carriers.len(),
            task_capacity: block.inner.stacks.total_capacity(),
            stack: block.inner.stacks.memory_footprint(),
            runtime_metadata_bytes: block.metadata.mapping.len,
            control_bytes: block.region.len.saturating_sub(block.metadata.mapping.len),
        }
    }
}

impl Deref for GreenPoolLease {
    type Target = GreenPoolInner;

    fn deref(&self) -> &Self::Target {
        &self.block().inner
    }
}

impl Drop for GreenPoolLease {
    fn drop(&mut self) {
        let Ok(release) = self.block().header.release() else {
            return;
        };
        if release != SharedRelease::Last {
            return;
        }
        unsafe { destroy_green_pool_block(self.ptr.as_ptr()) };
    }
}

unsafe fn destroy_green_pool_block(block: *mut GreenPoolControlBlock) {
    // SAFETY: callers only invoke this after consuming the final intrusive reference to the
    // control block. The inner value must be dropped before the metadata mapping is released,
    // and the control mapping itself is only unmapped after both have been torn down.
    unsafe {
        ptr::drop_in_place(addr_of_mut!((*block).inner));
        let metadata = ManuallyDrop::take(&mut (*block).metadata);
        let backing = ManuallyDrop::take(&mut (*block).backing);
        let region = (*block).region;
        drop(metadata);
        match backing {
            GreenPoolControlBacking::VirtualCachedRegion(cached_region) => {
                debug_assert_eq!(cached_region, region);
                if !cache_green_runtime_region(region).unwrap_or(false) {
                    let _ = system_mem().unmap(region);
                }
            }
            GreenPoolControlBacking::Owned {
                control,
                metadata,
                slab_owner,
            } => {
                let _ = control.resolved();
                let _ = metadata.resolved();
                drop(slab_owner);
            }
        }
    }
}

fn green_runtime_region_cache()
-> Result<&'static SyncMutex<[Option<Region>; GREEN_RUNTIME_REGION_CACHE_SLOTS]>, FiberError> {
    GREEN_RUNTIME_REGION_CACHE
        .get_or_init(|| SyncMutex::new([None; GREEN_RUNTIME_REGION_CACHE_SLOTS]))
        .map_err(fiber_error_from_sync)
}

fn try_take_cached_green_runtime_region(len: usize) -> Result<Option<Region>, FiberError> {
    let cache = green_runtime_region_cache()?;
    let mut guard = cache.lock().map_err(fiber_error_from_sync)?;
    for slot in &mut *guard {
        if let Some(region) = *slot
            && region.len == len
        {
            *slot = None;
            return Ok(Some(region));
        }
    }
    Ok(None)
}

fn cache_green_runtime_region(region: Region) -> Result<bool, FiberError> {
    let cache = green_runtime_region_cache()?;
    let mut guard = cache.lock().map_err(fiber_error_from_sync)?;
    for slot in &mut *guard {
        if slot.is_none() {
            *slot = Some(region);
            return Ok(true);
        }
    }
    Ok(false)
}

const fn green_pool_metadata_alignment() -> usize {
    let mut align = align_of::<GreenPoolMetadataHeader>();
    if align_of::<GreenTaskSlot>() > align {
        align = align_of::<GreenTaskSlot>();
    }
    if align_of::<usize>() > align {
        align = align_of::<usize>();
    }
    if align_of::<CarrierQueue>() > align {
        align = align_of::<CarrierQueue>();
    }
    #[cfg(feature = "std")]
    if align_of::<CarrierLoopContext>() > align {
        align = align_of::<CarrierLoopContext>();
    }
    if align_of::<PriorityBucket>() > align {
        align = align_of::<PriorityBucket>();
    }
    if align_of::<i8>() > align {
        align = align_of::<i8>();
    }
    if align_of::<u64>() > align {
        align = align_of::<u64>();
    }
    if align_of::<Option<CarrierWaiterRecord>>() > align {
        align = align_of::<Option<CarrierWaiterRecord>>();
    }
    align
}

fn green_pool_runtime_regions(
    carrier_count: usize,
    task_capacity: usize,
    scheduling: GreenScheduling,
    reactor_enabled: bool,
    sizing: RuntimeSizingStrategy,
) -> Result<(Region, Region), FiberError> {
    let memory = system_mem();
    let page = memory.page_info().alloc_granule.get();
    let metadata_align = green_pool_metadata_alignment();
    let control_align = align_of::<GreenPoolControlBlock>().max(metadata_align);
    let control_len = apply_fiber_sizing_strategy_bytes(
        fiber_align_up(size_of::<GreenPoolControlBlock>(), metadata_align)?,
        sizing,
    )?;
    let metadata_len = apply_fiber_sizing_strategy_bytes(
        GreenPoolMetadata::metadata_bytes(
            carrier_count,
            task_capacity,
            scheduling,
            reactor_enabled,
            metadata_align,
        )?,
        sizing,
    )?;
    let total_len = fiber_align_up(
        control_len
            .checked_add(metadata_len)
            .ok_or_else(FiberError::resource_exhausted)?,
        page,
    )?;
    let region = if let Some(region) = try_take_cached_green_runtime_region(total_len)? {
        region
    } else {
        unsafe {
            memory.map(&MapRequest {
                len: total_len,
                align: page.max(control_align),
                protect: Protect::READ | Protect::WRITE,
                flags: MapFlags::PRIVATE,
                attrs: RegionAttrs::VIRTUAL_ONLY,
                cache: CachePolicy::Default,
                placement: Placement::Anywhere,
                backing: Backing::Anonymous,
            })
        }
        .map_err(fiber_error_from_mem)?
    };

    let metadata_region = Region {
        base: region
            .base
            .checked_add(control_len)
            .ok_or_else(FiberError::resource_exhausted)?,
        len: total_len.saturating_sub(control_len),
    };
    Ok((region, metadata_region))
}

#[derive(Debug)]
struct GreenPoolInner {
    support: FiberSupport,
    courier_id: Option<CourierId>,
    context_id: Option<ContextId>,
    runtime_sink: Option<CourierRuntimeSink>,
    launch_control: Option<CourierLaunchControl<'static>>,
    launch_request: Option<CourierChildLaunchRequest<'static>>,
    scheduling: GreenScheduling,
    capacity_policy: CapacityPolicy,
    yield_budget_supported: bool,
    #[cfg(feature = "std")]
    yield_budget_policy: FiberYieldBudgetPolicy,
    shutdown: AtomicBool,
    client_refs: AtomicUsize,
    active: AtomicUsize,
    root_registered: AtomicBool,
    launch_registered: AtomicBool,
    next_id: AtomicUsize,
    next_carrier: AtomicUsize,
    carriers: MetadataSlice<CarrierQueue>,
    tasks: GreenTaskRegistry,
    stacks: FiberStackStore,
    #[cfg(feature = "std")]
    yield_budget_runtime: GreenYieldBudgetRuntime,
}

#[cfg(feature = "std")]
fn ensure_yield_budget_watchdog_started(
    inner: &GreenPoolLease,
    task: FiberTaskAttributes,
) -> Result<(), FiberError> {
    if task.yield_budget.is_none() || inner.carriers.len() <= 1 {
        return Ok(());
    }

    if inner
        .yield_budget_runtime
        .watchdog_started
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Ok(());
    }

    let watchdog_inner = match inner.try_clone() {
        Ok(clone) => clone,
        Err(error) => {
            inner
                .yield_budget_runtime
                .watchdog_started
                .store(false, Ordering::Release);
            return Err(error);
        }
    };

    if let Err(_error) = std::thread::Builder::new()
        .name("fusion-fiber-watchdog".into())
        .spawn(move || run_yield_budget_watchdog(watchdog_inner))
    {
        inner
            .yield_budget_runtime
            .watchdog_started
            .store(false, Ordering::Release);
        return Err(FiberError::state_conflict());
    }

    Ok(())
}

impl GreenPoolInner {
    fn runtime_tick(&self) -> u64 {
        current_monotonic_nanos().unwrap_or(0)
    }

    fn publish_runtime_context(&self) -> Result<(), FiberError> {
        let (Some(runtime_sink), Some(courier_id), Some(context_id)) =
            (self.runtime_sink, self.courier_id, self.context_id)
        else {
            return Ok(());
        };
        runtime_sink
            .record_context(courier_id, context_id, self.runtime_tick())
            .map_err(fiber_error_from_runtime_sink)
    }

    fn publish_runtime_summary(&self) -> Result<(), FiberError> {
        let (Some(runtime_sink), Some(courier_id)) = (self.runtime_sink, self.courier_id) else {
            return Ok(());
        };
        let (active_units, runnable_units, running_units, blocked_units) =
            self.tasks.lane_counts()?;
        let available_slots = self.tasks.available_slots()?;
        let responsiveness = runtime_sink
            .evaluate_responsiveness(courier_id, self.runtime_tick())
            .map_err(fiber_error_from_runtime_sink)?;
        let summary = CourierRuntimeSummary::new(
            match self.scheduling {
                GreenScheduling::Fifo => CourierSchedulingPolicy::CooperativeRoundRobin,
                GreenScheduling::Priority => CourierSchedulingPolicy::CooperativePriority,
                GreenScheduling::WorkStealing => CourierSchedulingPolicy::CooperativeWorkStealing,
            },
            responsiveness,
        )
        .with_fiber_lane(CourierLaneSummary {
            kind: RunnableUnitKind::Fiber,
            active_units,
            runnable_units,
            running_units,
            blocked_units,
            available_slots,
        });
        runtime_sink
            .record_runtime_summary(courier_id, summary, self.runtime_tick())
            .map_err(fiber_error_from_runtime_sink)
    }

    fn register_runtime_fiber(
        &self,
        fiber: FiberId,
        generation: u64,
        class: fusion_sys::courier::CourierFiberClass,
    ) -> Result<(), FiberError> {
        let (Some(runtime_sink), Some(courier_id)) = (self.runtime_sink, self.courier_id) else {
            return Ok(());
        };
        let is_root = self
            .root_registered
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok();
        if is_root
            && let (Some(launch_control), Some(launch_request)) =
                (self.launch_control, self.launch_request)
            && self
                .launch_registered
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
        {
            launch_control
                .register_child_courier(launch_request, self.runtime_tick(), fiber)
                .map_err(fiber_error_from_launch_control)?;
        }
        runtime_sink
            .register_fiber(
                courier_id,
                ManagedFiberSnapshot {
                    id: fiber,
                    state: fusion_sys::fiber::FiberState::Created,
                    started: false,
                    claim_awareness: fusion_sys::claims::ClaimAwareness::Blind,
                    claim_context: None,
                },
                generation,
                class,
                is_root,
                None,
                self.runtime_tick(),
            )
            .map_err(fiber_error_from_runtime_sink)?;
        self.publish_runtime_context()?;
        self.publish_runtime_summary()
    }

    fn update_runtime_fiber(
        &self,
        fiber: FiberId,
        state: fusion_sys::fiber::FiberState,
        started: bool,
    ) -> Result<(), FiberError> {
        let (Some(runtime_sink), Some(courier_id)) = (self.runtime_sink, self.courier_id) else {
            return Ok(());
        };
        runtime_sink
            .update_fiber(
                courier_id,
                ManagedFiberSnapshot {
                    id: fiber,
                    state,
                    started,
                    claim_awareness: fusion_sys::claims::ClaimAwareness::Blind,
                    claim_context: None,
                },
                self.runtime_tick(),
            )
            .map_err(fiber_error_from_runtime_sink)?;
        self.publish_runtime_summary()
    }

    fn mark_runtime_fiber_terminal(
        &self,
        fiber: FiberId,
        terminal: fusion_sys::courier::FiberTerminalStatus,
    ) -> Result<(), FiberError> {
        let (Some(runtime_sink), Some(courier_id)) = (self.runtime_sink, self.courier_id) else {
            return Ok(());
        };
        runtime_sink
            .mark_fiber_terminal(courier_id, fiber, terminal, self.runtime_tick())
            .map_err(fiber_error_from_runtime_sink)?;
        self.publish_runtime_summary()
    }

    fn enqueue_with_signal(
        &self,
        carrier: usize,
        slot_index: usize,
        signal: bool,
    ) -> Result<(), FiberError> {
        if self.shutdown.load(Ordering::Acquire) {
            return Err(FiberError::state_conflict());
        }

        let queue = self.carriers.get(carrier).ok_or_else(FiberError::invalid)?;
        let priority = match self.tasks.priority(slot_index) {
            Ok(priority) => priority,
            Err(error) => {
                trace_spawn_failure("enqueue_with_signal.priority", Some(slot_index), &error);
                return Err(error);
            }
        };
        match queue
            .queue
            .with(|ready| ready.enqueue(slot_index, priority))
        {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                trace_spawn_failure("enqueue_with_signal.queue", Some(slot_index), &error);
                return Err(error);
            }
            Err(error) => {
                trace_spawn_failure("enqueue_with_signal.queue_lock", Some(slot_index), &error);
                return Err(error);
            }
        }
        if !signal {
            return Ok(());
        }
        if matches!(self.scheduling, GreenScheduling::WorkStealing) {
            for queue in &*self.carriers {
                queue.signal()?;
            }
            return Ok(());
        }
        queue.signal()
    }

    fn request_shutdown(&self) -> Result<(), FiberError> {
        if self.shutdown.swap(true, Ordering::AcqRel) {
            return Ok(());
        }

        for carrier in &*self.carriers {
            carrier.signal()?;
        }
        Ok(())
    }

    #[cfg(feature = "std")]
    fn dispatch_yield_budget_event(&self, event: FiberYieldBudgetEvent) {
        match self.yield_budget_policy {
            FiberYieldBudgetPolicy::Abort => {
                std::process::abort();
            }
            FiberYieldBudgetPolicy::Notify(callback) => {
                run_yield_budget_callback_contained(callback, event);
            }
        }
    }

    #[cfg(feature = "std")]
    fn begin_yield_budget_segment(
        &self,
        carrier_index: usize,
        slot_index: usize,
        task_id: u64,
        budget: Option<Duration>,
        start_nanos: u64,
    ) {
        let Some(budget) = budget else {
            self.yield_budget_runtime.carriers[carrier_index].clear();
            return;
        };
        let budget_nanos = saturating_duration_to_nanos_u64(budget);
        self.yield_budget_runtime.carriers[carrier_index].begin(
            slot_index,
            task_id,
            start_nanos,
            budget_nanos,
        );
    }

    #[cfg(feature = "std")]
    fn finish_yield_budget_segment(
        &self,
        carrier_index: usize,
        fiber_id: u64,
        budget: Option<Duration>,
        observed: Duration,
    ) -> bool {
        let state = &self.yield_budget_runtime.carriers[carrier_index];
        let Some(budget) = budget else {
            state.clear();
            return false;
        };

        let already_reported = state.reported.swap(false, Ordering::AcqRel);
        let faulted = state.faulted.swap(false, Ordering::AcqRel);
        state.clear();

        let exceeded = faulted || observed > budget;
        if !exceeded {
            return false;
        }

        if !already_reported {
            self.dispatch_yield_budget_event(FiberYieldBudgetEvent {
                fiber_id,
                carrier_id: carrier_index,
                budget,
                observed,
            });
        }
        true
    }

    #[cfg(not(feature = "std"))]
    fn finish_yield_budget_segment(
        &self,
        _carrier_index: usize,
        _fiber_id: u64,
        budget: Option<Duration>,
        observed: Duration,
    ) -> bool {
        budget.is_some_and(|budget| observed > budget)
    }

    #[cfg(feature = "std")]
    fn scan_yield_budget_overruns(&self) -> Result<(), FiberError> {
        let now_nanos = GreenYieldBudgetRuntime::now_nanos()?;
        for (carrier_id, state) in self.yield_budget_runtime.carriers.iter().enumerate() {
            let slot_index = state.slot_index.load(Ordering::Acquire);
            if slot_index == CarrierYieldBudgetState::IDLE_SLOT {
                continue;
            }

            let task_id = state.task_id.load(Ordering::Acquire);
            let budget_nanos = state.budget_nanos.load(Ordering::Acquire);
            if budget_nanos == 0 {
                continue;
            }

            let started_nanos = state.started_nanos.load(Ordering::Acquire);
            let elapsed_nanos = now_nanos.saturating_sub(started_nanos);
            if elapsed_nanos <= budget_nanos {
                continue;
            }

            let Ok(current_id) = self.tasks.current_id(slot_index) else {
                continue;
            };
            if current_id != task_id {
                continue;
            }

            if state.reported.swap(true, Ordering::AcqRel) {
                continue;
            }
            state.faulted.store(true, Ordering::Release);
            self.dispatch_yield_budget_event(FiberYieldBudgetEvent {
                fiber_id: task_id,
                carrier_id,
                budget: Duration::from_nanos(budget_nanos),
                observed: Duration::from_nanos(elapsed_nanos),
            });
        }
        Ok(())
    }

    fn migrate_ready_task(
        &self,
        slot_index: usize,
        task_id: u64,
        carrier: usize,
    ) -> Result<(), FiberError> {
        self.tasks.reassign_carrier(slot_index, task_id, carrier)?;
        if !self.tasks.execution(slot_index)?.requires_fiber() {
            return Ok(());
        }
        let (pool_index, stack_slot) = self.tasks.stack_location(slot_index, task_id)?;
        self.stacks.attach_slot_identity(
            pool_index,
            stack_slot,
            task_id,
            carrier,
            self.carriers[carrier].capacity_token(),
        )
    }

    fn try_steal_ready(&self, carrier: usize) -> Result<Option<usize>, FiberError> {
        if !matches!(self.scheduling, GreenScheduling::WorkStealing) || self.carriers.len() < 2 {
            return Ok(None);
        }

        let start = self.carriers[carrier].next_steal_start(self.carriers.len());
        for step in 0..(self.carriers.len() - 1) {
            let source = (carrier + start + step) % self.carriers.len();
            let source_queue = self.carriers.get(source).ok_or_else(FiberError::invalid)?;
            let stolen = source_queue.queue.with(CarrierReadyQueue::steal)?;

            let Some(slot_index) = stolen else {
                continue;
            };
            let task_id = self.tasks.current_id(slot_index)?;
            self.migrate_ready_task(slot_index, task_id, carrier)?;
            return Ok(Some(slot_index));
        }

        Ok(None)
    }

    fn park_on_readiness(
        &self,
        carrier_index: usize,
        slot_index: usize,
        task_id: u64,
        source: EventSourceHandle,
        interest: EventInterest,
    ) -> Result<(), FiberError> {
        let carrier = self
            .carriers
            .get(carrier_index)
            .ok_or_else(FiberError::invalid)?;
        let reactor = carrier
            .reactor
            .as_ref()
            .ok_or_else(FiberError::unsupported)?;
        reactor.register_wait(slot_index, task_id, source, interest)?;
        self.tasks
            .set_state(slot_index, task_id, GreenTaskState::Waiting)
    }

    fn dispatch_capacity_for_task(&self, slot_index: usize, id: u64) -> Result<(), FiberError> {
        if !self.tasks.execution(slot_index)?.requires_fiber() {
            return Ok(());
        }
        let (pool_index, stack_slot) = self.tasks.stack_location(slot_index, id)?;
        self.stacks
            .dispatch_capacity_event(pool_index, stack_slot, self.capacity_policy)
    }

    fn dispatch_capacity_for_carrier(&self, carrier_index: usize) -> Result<(), FiberError> {
        let mut first_error = None;
        for slot_index in 0..self.tasks.slots.len() {
            let assignment = match self.tasks.assignment(slot_index) {
                Ok(assignment) => assignment,
                Err(error) => {
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                    continue;
                }
            };
            let Some((task_id, carrier)) = assignment else {
                continue;
            };
            if carrier != carrier_index {
                continue;
            }
            if let Err(error) = self.dispatch_capacity_for_task(slot_index, task_id)
                && first_error.is_none()
            {
                first_error = Some(error);
            }
        }

        if let Some(error) = first_error {
            return Err(error);
        }
        Ok(())
    }

    fn finish_task(
        &self,
        slot_index: usize,
        id: u64,
        terminal_state: GreenTaskState,
    ) -> Result<(), FiberError> {
        let mut first_error = None;
        let runtime_fiber_id = self.tasks.current_fiber_id(slot_index)?;
        let terminal_state = match self
            .tasks
            .slot(slot_index)?
            .begin_finish(id, terminal_state)
        {
            Ok(terminal_state) => terminal_state,
            Err(error) => {
                trace_carrier_failure("finish_task.begin_finish", usize::MAX, &error);
                return Err(error);
            }
        };

        if self.tasks.execution(slot_index)?.requires_fiber() {
            let stack_location = match self.tasks.stack_location(slot_index, id) {
                Ok(stack_location) => Some(stack_location),
                Err(error) => {
                    trace_carrier_failure("finish_task.stack_location", usize::MAX, &error);
                    first_error = Some(error);
                    None
                }
            };

            if let Err(error) = self.tasks.clear_fiber(slot_index, id)
                && first_error.is_none()
            {
                trace_carrier_failure("finish_task.clear_fiber", usize::MAX, &error);
                first_error = Some(error);
            }

            if let Some((pool_index, stack_slot)) = stack_location
                && let Err(error) = self.stacks.release(pool_index, stack_slot)
                && first_error.is_none()
            {
                trace_carrier_failure("finish_task.release_stack", usize::MAX, &error);
                first_error = Some(error);
            }
        }

        self.active.fetch_sub(1, Ordering::AcqRel);

        if let Err(error) = self
            .tasks
            .slot(slot_index)?
            .settle_terminal_state(id, terminal_state)
            && first_error.is_none()
        {
            trace_carrier_failure("finish_task.settle_terminal_state", usize::MAX, &error);
            first_error = Some(error);
        }

        if let Err(error) = self.tasks.signal_completed(slot_index, id)
            && first_error.is_none()
        {
            trace_carrier_failure("finish_task.signal_completed", usize::MAX, &error);
            first_error = Some(error);
        }

        if let Err(error) = self.tasks.try_reclaim(slot_index, id)
            && first_error.is_none()
        {
            trace_carrier_failure("finish_task.try_reclaim", usize::MAX, &error);
            first_error = Some(error);
        }

        let runtime_terminal = match terminal_state {
            GreenTaskState::Completed => {
                fusion_sys::courier::FiberTerminalStatus::Completed(FiberReturn::new(0))
            }
            GreenTaskState::Failed(error) => {
                fusion_sys::courier::FiberTerminalStatus::Faulted(error.kind())
            }
            GreenTaskState::Queued
            | GreenTaskState::Running
            | GreenTaskState::Yielded
            | GreenTaskState::Waiting
            | GreenTaskState::Finishing => fusion_sys::courier::FiberTerminalStatus::Abandoned(
                fusion_sys::fiber::FiberState::Suspended,
            ),
        };
        if let Err(error) = self.mark_runtime_fiber_terminal(runtime_fiber_id, runtime_terminal)
            && first_error.is_none()
        {
            trace_carrier_failure(
                "finish_task.mark_runtime_fiber_terminal",
                usize::MAX,
                &error,
            );
            first_error = Some(error);
        }

        if let Some(error) = first_error {
            return Err(error);
        }
        Ok(())
    }
}

/// Opaque public green-thread handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GreenHandleDriveMode {
    CarrierPool,
    CurrentThread,
}
