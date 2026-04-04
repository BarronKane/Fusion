#[derive(Debug, Clone, Copy)]
struct FiberStackLease {
    pool_index: usize,
    slot_index: usize,
    class: FiberStackClass,
    stack: FiberStack,
}

#[derive(Debug)]
struct FiberStackPoolEntry {
    class: FiberStackClass,
    slab: FiberStackSlab,
}

#[derive(Debug)]
struct FiberStackClassPools {
    mapping: Region,
    entries: NonNull<FiberStackPoolEntry>,
    len: usize,
    total_capacity: usize,
}

#[derive(Debug)]
enum FiberStackStore {
    Legacy(FiberStackSlab),
    Classes(FiberStackClassPools),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct FixedStackLayout {
    usable_size: usize,
    guard: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct ElasticStackLayout {
    initial: usize,
    max: usize,
    guard: usize,
    detector: usize,
}

struct ElasticStackMeta {
    reservation_base: usize,
    reservation_end: usize,
    page_size: usize,
    telemetry: FiberTelemetry,
    initial_committed_pages: u32,
    max_committed_pages: u32,
    fiber_id: AtomicUsize,
    carrier_id: AtomicUsize,
    capacity_token: AtomicUsize,
    initial_detector_page: usize,
    initial_guard_page: usize,
    detector_page: AtomicUsize,
    guard_page: AtomicUsize,
    at_capacity: AtomicBool,
    capacity_pending: AtomicBool,
    occupied: AtomicBool,
    growth_events: AtomicU32,
    committed_pages: AtomicU32,
    active: AtomicBool,
}

impl fmt::Debug for ElasticStackMeta {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ElasticStackMeta")
            .field("reservation_base", &self.reservation_base)
            .field("reservation_end", &self.reservation_end)
            .field("page_size", &self.page_size)
            .field("telemetry", &self.telemetry)
            .field("initial_committed_pages", &self.initial_committed_pages)
            .field("max_committed_pages", &self.max_committed_pages)
            .field("fiber_id", &self.fiber_id.load(Ordering::Acquire))
            .field("carrier_id", &self.carrier_id.load(Ordering::Acquire))
            .field(
                "capacity_token",
                &self.capacity_token.load(Ordering::Acquire),
            )
            .field("initial_detector_page", &self.initial_detector_page)
            .field("initial_guard_page", &self.initial_guard_page)
            .field("detector_page", &self.detector_page.load(Ordering::Acquire))
            .field("guard_page", &self.guard_page.load(Ordering::Acquire))
            .field("at_capacity", &self.at_capacity.load(Ordering::Acquire))
            .field(
                "capacity_pending",
                &self.capacity_pending.load(Ordering::Acquire),
            )
            .field("occupied", &self.occupied.load(Ordering::Acquire))
            .field("growth_events", &self.growth_events.load(Ordering::Acquire))
            .field(
                "committed_pages",
                &self.committed_pages.load(Ordering::Acquire),
            )
            .field("active", &self.active.load(Ordering::Acquire))
            .finish()
    }
}

#[derive(Debug)]
enum FiberStackBackingState {
    Fixed(FixedStackLayout),
    Elastic {
        layout: ElasticStackLayout,
        metadata: MetadataSlice<ElasticStackMeta>,
    },
}

#[derive(Debug)]
enum FiberStackSlabStorage {
    VirtualCombined(Region),
    Explicit {
        stack: MemoryResourceHandle,
        metadata: MemoryResourceHandle,
    },
}

#[derive(Debug)]
struct FiberStackSlab {
    storage: FiberStackSlabStorage,
    region: Region,
    metadata_bytes: usize,
    slot_stride: usize,
    capacity: usize,
    initial_slots: usize,
    chunk_size: usize,
    growth: GreenGrowth,
    telemetry: FiberTelemetry,
    huge_pages: HugePagePolicy,
    stack_direction: ContextStackDirection,
    backing: FiberStackBackingState,
    peak_used_bytes: AtomicUsize,
    state: SyncMutex<FiberStackSlabState>,
}

#[derive(Debug)]
struct FiberStackSlabState {
    free: MetadataIndexStack,
    allocated: MetadataSlice<bool>,
    committed_slots: usize,
}

#[derive(Debug, Clone, Copy)]
struct FiberStackRegionLayout {
    region: Region,
    slot_stride: usize,
    capacity: usize,
    stack_direction: ContextStackDirection,
}

impl FiberStackSlabState {
    fn new(
        free_entries: MetadataSlice<usize>,
        allocated: MetadataSlice<bool>,
        initial_slots: usize,
    ) -> Result<Self, FiberError> {
        for index in 0..allocated.len() {
            unsafe {
                allocated.write(index, false)?;
            }
        }
        Ok(Self {
            free: MetadataIndexStack::with_prefix(free_entries, initial_slots)?,
            allocated,
            committed_slots: initial_slots,
        })
    }
}

// SAFETY: the mapped region is immutable after construction and slot bookkeeping is serialized
// through `state`.
unsafe impl Send for FiberStackSlab {}
// SAFETY: the mapped region is immutable after construction and slot bookkeeping is serialized
// through `state`.
unsafe impl Sync for FiberStackSlab {}

impl FiberStackSlab {
    const fn storage_uses_mem_protect(&self) -> bool {
        matches!(self.storage, FiberStackSlabStorage::VirtualCombined(_))
    }

    fn new(
        config: &FiberPoolConfig<'_>,
        alignment: usize,
        stack_direction: ContextStackDirection,
    ) -> Result<Self, FiberError> {
        let backing = apply_fiber_sizing_strategy_backing(config.stack_backing, config.sizing)?;
        let guard_pages = config.guard_pages;
        let count = config.max_fibers_per_carrier;
        let growth_chunk = config.growth_chunk;
        let growth = config.growth;
        let telemetry = config.telemetry;
        let huge_pages = config.huge_pages;
        if count == 0
            || growth_chunk == 0
            || growth_chunk > count
            || alignment == 0
            || !alignment.is_power_of_two()
        {
            return Err(FiberError::invalid());
        }
        if guard_pages != 0 && matches!(stack_direction, ContextStackDirection::Unknown) {
            return Err(FiberError::unsupported());
        }

        let memory = system_mem();
        Self::validate_huge_page_policy(memory.support().advice, huge_pages)?;
        let page = memory.page_info().alloc_granule.get();
        let rounded_guard = guard_pages
            .checked_mul(page)
            .ok_or_else(FiberError::resource_exhausted)?;
        let (slot_stride, backing) =
            Self::build_backing(backing, rounded_guard, page, alignment, stack_direction)?;
        let total = apply_fiber_sizing_strategy_bytes(
            slot_stride
                .checked_mul(count)
                .ok_or_else(FiberError::resource_exhausted)?,
            config.sizing,
        )?;
        let elastic = matches!(backing, FiberStackBackingState::Elastic { .. });
        let metadata_len = apply_fiber_sizing_strategy_bytes(
            Self::metadata_bytes(count, elastic, page)?,
            config.sizing,
        )?;
        let mapping_len = metadata_len
            .checked_add(total)
            .ok_or_else(FiberError::resource_exhausted)?;

        let mapping = unsafe {
            memory.map(&MapRequest {
                len: mapping_len,
                align: page,
                protect: Protect::NONE,
                flags: MapFlags::PRIVATE,
                attrs: RegionAttrs::VIRTUAL_ONLY,
                cache: CachePolicy::Default,
                placement: Placement::Anywhere,
                backing: Backing::Anonymous,
            })
        }
        .map_err(fiber_error_from_mem)?;
        let metadata_region = mapping
            .subrange(0, metadata_len)
            .map_err(fiber_error_from_mem)?;
        unsafe { memory.protect(metadata_region, Protect::READ | Protect::WRITE) }
            .map_err(fiber_error_from_mem)?;
        let region = mapping
            .subrange(metadata_len, total)
            .map_err(fiber_error_from_mem)?;

        let initial_slots = match growth {
            GreenGrowth::Fixed => count,
            GreenGrowth::OnDemand => count.min(growth_chunk),
        };
        let (header, state, elastic_metadata) =
            Self::initialize_metadata(metadata_region, count, slot_stride, initial_slots, elastic)?;

        let mut slab = Self {
            storage: FiberStackSlabStorage::VirtualCombined(mapping),
            region,
            metadata_bytes: metadata_len,
            slot_stride,
            capacity: count,
            initial_slots,
            chunk_size: growth_chunk,
            growth,
            telemetry,
            huge_pages,
            stack_direction,
            backing: match backing {
                FiberStackBackingState::Fixed(layout) => FiberStackBackingState::Fixed(layout),
                FiberStackBackingState::Elastic { layout, .. } => FiberStackBackingState::Elastic {
                    layout,
                    metadata: elastic_metadata.ok_or_else(FiberError::invalid)?,
                },
            },
            peak_used_bytes: AtomicUsize::new(0),
            state: SyncMutex::new(state),
        };
        debug_assert_eq!(header.capacity, count);
        debug_assert_eq!(header.slot_stride, slot_stride);

        slab.initialize_slots(initial_slots)?;
        slab.apply_huge_page_policy()?;

        Ok(slab)
    }

    fn from_backing(
        config: &FiberPoolConfig<'_>,
        alignment: usize,
        stack_direction: ContextStackDirection,
        stack: MemoryResourceHandle,
        metadata: MemoryResourceHandle,
    ) -> Result<Self, FiberError> {
        let backing = apply_fiber_sizing_strategy_backing(config.stack_backing, config.sizing)?;
        let guard_pages = config.guard_pages;
        let count = config.max_fibers_per_carrier;
        let growth_chunk = config.growth_chunk;
        let growth = config.growth;
        let telemetry = config.telemetry;
        let huge_pages = config.huge_pages;
        if count == 0
            || growth_chunk == 0
            || growth_chunk > count
            || alignment == 0
            || !alignment.is_power_of_two()
        {
            return Err(FiberError::invalid());
        }
        if guard_pages != 0 {
            return Err(FiberError::unsupported());
        }
        if matches!(backing, FiberStackBacking::Elastic { .. }) {
            return Err(FiberError::unsupported());
        }
        if !matches!(huge_pages, HugePagePolicy::Disabled) {
            return Err(FiberError::unsupported());
        }

        let stack_region = unsafe { stack.view().raw_region() };
        let metadata_region = unsafe { metadata.view().raw_region() };
        let (slot_stride, backing) =
            Self::build_backing(backing, 0, 1, alignment, stack_direction)?;
        let total = slot_stride
            .checked_mul(count)
            .ok_or_else(FiberError::resource_exhausted)?;
        if stack_region.len < total {
            return Err(FiberError::resource_exhausted());
        }
        let elastic = matches!(backing, FiberStackBackingState::Elastic { .. });
        let metadata_len = Self::metadata_bytes(count, elastic, 1)?;
        if metadata_region.len < metadata_len {
            return Err(FiberError::resource_exhausted());
        }
        let initial_slots = match growth {
            GreenGrowth::Fixed => count,
            GreenGrowth::OnDemand => count.min(growth_chunk),
        };
        let (header, state, elastic_metadata) =
            Self::initialize_metadata(metadata_region, count, slot_stride, initial_slots, elastic)?;

        let mut slab = Self {
            storage: FiberStackSlabStorage::Explicit { stack, metadata },
            region: stack_region,
            metadata_bytes: metadata_len,
            slot_stride,
            capacity: count,
            initial_slots,
            chunk_size: growth_chunk,
            growth,
            telemetry,
            huge_pages,
            stack_direction,
            backing: match backing {
                FiberStackBackingState::Fixed(layout) => FiberStackBackingState::Fixed(layout),
                FiberStackBackingState::Elastic { layout, .. } => FiberStackBackingState::Elastic {
                    layout,
                    metadata: elastic_metadata.ok_or_else(FiberError::invalid)?,
                },
            },
            peak_used_bytes: AtomicUsize::new(0),
            state: SyncMutex::new(state),
        };
        debug_assert_eq!(header.capacity, count);
        debug_assert_eq!(header.slot_stride, slot_stride);
        slab.initialize_slots(initial_slots)?;
        Ok(slab)
    }

    fn metadata_bytes(capacity: usize, elastic: bool, page: usize) -> Result<usize, FiberError> {
        let mut bytes = size_of::<FiberStackSlabHeader>();
        bytes = fiber_align_up(bytes, align_of::<usize>())?;
        bytes = bytes
            .checked_add(
                size_of::<usize>()
                    .checked_mul(capacity)
                    .ok_or_else(FiberError::resource_exhausted)?,
            )
            .ok_or_else(FiberError::resource_exhausted)?;
        bytes = fiber_align_up(bytes, align_of::<bool>())?;
        bytes = bytes
            .checked_add(
                size_of::<bool>()
                    .checked_mul(capacity)
                    .ok_or_else(FiberError::resource_exhausted)?,
            )
            .ok_or_else(FiberError::resource_exhausted)?;
        if elastic {
            bytes = fiber_align_up(bytes, align_of::<ElasticStackMeta>())?;
            bytes = bytes
                .checked_add(
                    size_of::<ElasticStackMeta>()
                        .checked_mul(capacity)
                        .ok_or_else(FiberError::resource_exhausted)?,
                )
                .ok_or_else(FiberError::resource_exhausted)?;
        }
        fiber_align_up(bytes, page)
    }

    fn initialize_metadata(
        metadata_region: Region,
        capacity: usize,
        slot_stride: usize,
        initial_slots: usize,
        elastic: bool,
    ) -> Result<
        (
            FiberStackSlabHeader,
            FiberStackSlabState,
            Option<MetadataSlice<ElasticStackMeta>>,
        ),
        FiberError,
    > {
        let mut cursor = MetadataCursor::new(metadata_region);
        let header_slice = cursor.reserve_slice::<FiberStackSlabHeader>(1)?;
        let free_entries = cursor.reserve_slice::<usize>(capacity)?;
        let allocated = cursor.reserve_slice::<bool>(capacity)?;
        let elastic_metadata = if elastic {
            Some(cursor.reserve_slice::<ElasticStackMeta>(capacity)?)
        } else {
            None
        };

        let header = FiberStackSlabHeader {
            metadata_len: metadata_region.len,
            payload_offset: metadata_region.len,
            capacity,
            slot_stride,
            elastic,
        };
        unsafe {
            header_slice.write(0, header)?;
        }

        let state = FiberStackSlabState::new(free_entries, allocated, initial_slots)?;
        Ok((header, state, elastic_metadata))
    }

    const fn validate_huge_page_policy(
        advice_caps: MemAdviceCaps,
        policy: HugePagePolicy,
    ) -> Result<(), FiberError> {
        match policy {
            HugePagePolicy::Disabled => Ok(()),
            HugePagePolicy::Enabled { size } => {
                if !advice_caps.contains(MemAdviceCaps::HUGE_PAGE) {
                    return Err(FiberError::unsupported());
                }
                if matches!(size, HugePageSize::OneGiB) && !cfg!(target_arch = "x86_64") {
                    return Err(FiberError::unsupported());
                }
                Ok(())
            }
        }
    }

    fn build_backing(
        backing: FiberStackBacking,
        rounded_guard: usize,
        page: usize,
        alignment: usize,
        stack_direction: ContextStackDirection,
    ) -> Result<(usize, FiberStackBackingState), FiberError> {
        let usable_alignment = alignment.max(page);
        match backing {
            FiberStackBacking::Fixed { stack_size } => {
                let rounded_stack = stack_size
                    .get()
                    .checked_next_multiple_of(usable_alignment)
                    .ok_or_else(FiberError::resource_exhausted)?;
                let slot_stride = rounded_stack
                    .checked_add(rounded_guard)
                    .ok_or_else(FiberError::resource_exhausted)?;
                Ok((
                    slot_stride,
                    FiberStackBackingState::Fixed(FixedStackLayout {
                        usable_size: rounded_stack,
                        guard: rounded_guard,
                    }),
                ))
            }
            FiberStackBacking::Elastic {
                initial_size,
                max_size,
            } => {
                if !system_fiber_host().support().elastic_stack_faults
                    || stack_direction != ContextStackDirection::Down
                    || rounded_guard != page
                {
                    return Err(FiberError::unsupported());
                }
                let rounded_initial = initial_size
                    .get()
                    .checked_next_multiple_of(page)
                    .ok_or_else(FiberError::resource_exhausted)?;
                let rounded_max = max_size
                    .get()
                    .checked_next_multiple_of(page)
                    .ok_or_else(FiberError::resource_exhausted)?;
                if rounded_initial == 0 || rounded_initial > rounded_max {
                    return Err(FiberError::invalid());
                }
                let slot_stride = rounded_max
                    .checked_add(rounded_guard)
                    .and_then(|total| total.checked_add(page))
                    .ok_or_else(FiberError::resource_exhausted)?;
                Ok((
                    slot_stride,
                    FiberStackBackingState::Elastic {
                        layout: ElasticStackLayout {
                            initial: rounded_initial,
                            max: rounded_max,
                            guard: rounded_guard,
                            detector: page,
                        },
                        metadata: MetadataSlice::empty(),
                    },
                ))
            }
        }
    }

    fn initialize_slots(&mut self, committed_slots: usize) -> Result<(), FiberError> {
        let region_layout = FiberStackRegionLayout {
            region: self.region,
            slot_stride: self.slot_stride,
            capacity: self.capacity,
            stack_direction: self.stack_direction,
        };
        let telemetry = self.telemetry;
        let use_mem_protect = self.storage_uses_mem_protect();
        match &mut self.backing {
            FiberStackBackingState::Fixed(layout) => Self::initialize_fixed_slots(
                region_layout,
                use_mem_protect,
                *layout,
                committed_slots,
            ),
            FiberStackBackingState::Elastic { layout, metadata } => Self::initialize_elastic_slots(
                region_layout,
                telemetry,
                *layout,
                committed_slots,
                metadata,
            ),
        }
    }

    fn apply_huge_page_policy(&self) -> Result<(), FiberError> {
        let HugePagePolicy::Enabled { size } = self.huge_pages else {
            return Ok(());
        };

        let memory = system_mem();
        let advice_caps = memory.support().advice;
        if !advice_caps.contains(MemAdviceCaps::HUGE_PAGE) {
            return Err(FiberError::unsupported());
        }

        for slot_index in 0..self.capacity {
            let (huge_region, no_huge_region) = self.huge_page_regions(slot_index, size)?;
            if let Some(region) = huge_region {
                unsafe { memory.advise(region, Advise::HugePage) }.map_err(fiber_error_from_mem)?;
            }
            if let Some(region) = no_huge_region
                && advice_caps.contains(MemAdviceCaps::NO_HUGE_PAGE)
            {
                unsafe { memory.advise(region, Advise::NoHugePage) }
                    .map_err(fiber_error_from_mem)?;
            }
        }

        Ok(())
    }

    fn initialize_fixed_slots(
        region_layout: FiberStackRegionLayout,
        use_mem_protect: bool,
        layout: FixedStackLayout,
        committed_slots: usize,
    ) -> Result<(), FiberError> {
        if !use_mem_protect {
            return Ok(());
        }
        let memory = system_mem();
        for slot_index in 0..region_layout.capacity.min(committed_slots) {
            let slot = Self::slot_region_from(
                region_layout.region,
                region_layout.slot_stride,
                slot_index,
            )?;
            let usable = if layout.guard == 0 {
                slot.subrange(0, layout.usable_size)
            } else {
                match region_layout.stack_direction {
                    ContextStackDirection::Down => slot.subrange(layout.guard, layout.usable_size),
                    ContextStackDirection::Up => slot.subrange(0, layout.usable_size),
                    ContextStackDirection::Unknown => {
                        Err(fusion_pal::sys::mem::MemError::unsupported())
                    }
                }
            }
            .map_err(fiber_error_from_mem)?;
            unsafe { memory.protect(usable, Protect::READ | Protect::WRITE) }
                .map_err(fiber_error_from_mem)?;
        }
        Ok(())
    }

    fn initialize_elastic_slots(
        region_layout: FiberStackRegionLayout,
        telemetry: FiberTelemetry,
        layout: ElasticStackLayout,
        committed_slots: usize,
        metadata: &MetadataSlice<ElasticStackMeta>,
    ) -> Result<(), FiberError> {
        let memory = system_mem();
        for slot_index in 0..region_layout.capacity {
            let slot = Self::slot_region_from(
                region_layout.region,
                region_layout.slot_stride,
                slot_index,
            )?;
            if slot_index < committed_slots {
                let usable = Self::elastic_initial_usable_region_from(
                    region_layout.region,
                    region_layout.slot_stride,
                    region_layout.stack_direction,
                    slot_index,
                    layout,
                )?;
                unsafe { memory.protect(usable, Protect::READ | Protect::WRITE) }
                    .map_err(fiber_error_from_mem)?;
            }
            let detector_offset = slot
                .len
                .checked_sub(layout.initial + layout.detector)
                .ok_or_else(FiberError::invalid)?;
            let detector = slot
                .subrange(detector_offset, layout.detector)
                .map_err(fiber_error_from_mem)?;
            let guard_offset = slot
                .len
                .checked_sub(layout.initial + layout.detector + layout.guard)
                .ok_or_else(FiberError::invalid)?;
            let guard = slot
                .subrange(guard_offset, layout.guard)
                .map_err(fiber_error_from_mem)?;
            unsafe {
                metadata.write(
                    slot_index,
                    ElasticStackMeta {
                        reservation_base: slot.base.addr().get(),
                        reservation_end: slot.end_addr().ok_or_else(FiberError::invalid)?,
                        page_size: layout.detector,
                        telemetry,
                        initial_committed_pages: u32::try_from(layout.initial / layout.detector)
                            .map_err(|_| FiberError::resource_exhausted())?,
                        max_committed_pages: u32::try_from(layout.max / layout.detector)
                            .map_err(|_| FiberError::resource_exhausted())?,
                        fiber_id: AtomicUsize::new(0),
                        carrier_id: AtomicUsize::new(0),
                        capacity_token: AtomicUsize::new(wake_token_to_word(
                            PlatformWakeToken::invalid(),
                        )),
                        initial_detector_page: detector.base.addr().get(),
                        initial_guard_page: guard.base.addr().get(),
                        detector_page: AtomicUsize::new(detector.base.addr().get()),
                        guard_page: AtomicUsize::new(guard.base.addr().get()),
                        at_capacity: AtomicBool::new(false),
                        capacity_pending: AtomicBool::new(false),
                        occupied: AtomicBool::new(false),
                        growth_events: AtomicU32::new(0),
                        committed_pages: AtomicU32::new(0),
                        active: AtomicBool::new(true),
                    },
                )?;
            }
        }
        register_elastic_stack_metadata(metadata.as_slice())?;
        Ok(())
    }

    fn slot_region(&self, slot_index: usize) -> Result<Region, FiberError> {
        Self::slot_region_from(self.region, self.slot_stride, slot_index)
    }

    fn slot_region_from(
        region: Region,
        slot_stride: usize,
        slot_index: usize,
    ) -> Result<Region, FiberError> {
        region
            .subrange(slot_index * slot_stride, slot_stride)
            .map_err(fiber_error_from_mem)
    }

    fn fixed_usable_region(
        &self,
        slot_index: usize,
        layout: FixedStackLayout,
    ) -> Result<Region, FiberError> {
        let slot = self.slot_region(slot_index)?;
        if layout.guard == 0 {
            return slot
                .subrange(0, layout.usable_size)
                .map_err(fiber_error_from_mem);
        }
        match self.stack_direction {
            ContextStackDirection::Down => slot
                .subrange(layout.guard, layout.usable_size)
                .map_err(fiber_error_from_mem),
            ContextStackDirection::Up => slot
                .subrange(0, layout.usable_size)
                .map_err(fiber_error_from_mem),
            ContextStackDirection::Unknown => Err(FiberError::unsupported()),
        }
    }

    fn elastic_initial_usable_region(
        &self,
        slot_index: usize,
        layout: ElasticStackLayout,
    ) -> Result<Region, FiberError> {
        Self::elastic_initial_usable_region_from(
            self.region,
            self.slot_stride,
            self.stack_direction,
            slot_index,
            layout,
        )
    }

    fn elastic_initial_usable_region_from(
        region: Region,
        slot_stride: usize,
        stack_direction: ContextStackDirection,
        slot_index: usize,
        layout: ElasticStackLayout,
    ) -> Result<Region, FiberError> {
        let slot = Self::slot_region_from(region, slot_stride, slot_index)?;
        match stack_direction {
            ContextStackDirection::Down => slot
                .subrange(
                    slot.len
                        .checked_sub(layout.initial)
                        .ok_or_else(FiberError::invalid)?,
                    layout.initial,
                )
                .map_err(fiber_error_from_mem),
            ContextStackDirection::Up | ContextStackDirection::Unknown => {
                Err(FiberError::unsupported())
            }
        }
    }

    fn elastic_max_usable_region(
        &self,
        slot_index: usize,
        layout: ElasticStackLayout,
    ) -> Result<Region, FiberError> {
        let slot = self.slot_region(slot_index)?;
        match self.stack_direction {
            ContextStackDirection::Down => slot
                .subrange(
                    slot.len
                        .checked_sub(layout.max)
                        .ok_or_else(FiberError::invalid)?,
                    layout.max,
                )
                .map_err(fiber_error_from_mem),
            ContextStackDirection::Up | ContextStackDirection::Unknown => {
                Err(FiberError::unsupported())
            }
        }
    }

    fn huge_page_regions(
        &self,
        slot_index: usize,
        huge_size: HugePageSize,
    ) -> Result<(Option<Region>, Option<Region>), FiberError> {
        let threshold = huge_size.bytes();
        match &self.backing {
            FiberStackBackingState::Fixed(layout) => {
                let usable = self.fixed_usable_region(slot_index, *layout)?;
                if usable.len < threshold {
                    return Ok((None, None));
                }
                Ok((Some(usable), None))
            }
            FiberStackBackingState::Elastic { layout, .. } => {
                let usable = self.elastic_max_usable_region(slot_index, *layout)?;
                if usable.len < threshold {
                    return Ok((None, None));
                }

                let lower_small_window = layout.initial + layout.guard + layout.detector;
                let lower_window = lower_small_window
                    .checked_next_multiple_of(layout.detector)
                    .ok_or_else(FiberError::resource_exhausted)?;
                if usable.len <= lower_window {
                    return Ok((None, None));
                }

                let huge_offset = lower_window;
                let huge_len = usable.len - huge_offset;
                if huge_len < threshold {
                    return Ok((None, None));
                }

                let huge_region = usable
                    .subrange(huge_offset, huge_len)
                    .map_err(fiber_error_from_mem)?;
                let no_huge_region = if huge_offset == 0 {
                    None
                } else {
                    Some(
                        usable
                            .subrange(0, huge_offset)
                            .map_err(fiber_error_from_mem)?,
                    )
                };
                Ok((Some(huge_region), no_huge_region))
            }
        }
    }

    fn acquire(&self) -> Result<FiberStackLease, FiberError> {
        let slot_index = self.acquire_slot_index()?;
        let stack = match &self.backing {
            FiberStackBackingState::Fixed(layout) => {
                let usable = self.fixed_usable_region(slot_index, *layout)?;
                FiberStack::new(
                    usable
                        .base
                        .as_non_null::<u8>()
                        .ok_or_else(FiberError::invalid)?,
                    usable.len,
                )?
            }
            FiberStackBackingState::Elastic { .. } => {
                let slot = self.slot_region(slot_index)?;
                FiberStack::new(
                    slot.base
                        .as_non_null::<u8>()
                        .ok_or_else(FiberError::invalid)?,
                    slot.len,
                )?
            }
        };

        Ok(FiberStackLease {
            pool_index: 0,
            slot_index,
            class: self.default_task_class()?,
            stack,
        })
    }

    fn release(&self, slot_index: usize) -> Result<(), FiberError> {
        if let FiberStackBackingState::Fixed(layout) = &self.backing
            && !matches!(self.telemetry, FiberTelemetry::Disabled)
        {
            let used_bytes = self.observe_fixed_slot_usage(slot_index, *layout)?;
            self.peak_used_bytes.fetch_max(used_bytes, Ordering::AcqRel);
        }

        self.reset_slot(slot_index)?;

        let mut state = self.state.lock().map_err(fiber_error_from_sync)?;
        if slot_index >= state.committed_slots || !state.allocated[slot_index] {
            return Err(FiberError::state_conflict());
        }
        state.allocated[slot_index] = false;
        state.free.push(slot_index)?;
        self.try_shrink_locked(&mut state)
    }

    const fn requires_signal_handler(&self) -> bool {
        matches!(self.backing, FiberStackBackingState::Elastic { .. })
    }

    fn stack_stats(&self) -> Option<FiberStackStats> {
        if matches!(self.telemetry, FiberTelemetry::Disabled) {
            return None;
        }

        let FiberStackBackingState::Elastic { metadata, .. } = &self.backing else {
            return Some(FiberStackStats {
                total_growth_events: 0,
                peak_used_bytes: self.peak_used_bytes.load(Ordering::Acquire),
                peak_committed_pages: 0,
                committed_distribution: FiberStackDistribution::new(),
                at_capacity_count: 0,
            });
        };

        let mut stats = FiberStackStats {
            total_growth_events: 0,
            peak_used_bytes: 0,
            peak_committed_pages: 0,
            committed_distribution: FiberStackDistribution::new(),
            at_capacity_count: 0,
        };
        for meta in &**metadata {
            if !meta.occupied.load(Ordering::Acquire) {
                continue;
            }

            let growth_events = meta.growth_events.load(Ordering::Acquire);
            let committed_pages = Self::current_committed_pages(meta);
            stats.total_growth_events += u64::from(growth_events);
            stats.peak_committed_pages = stats.peak_committed_pages.max(committed_pages);
            if meta.at_capacity.load(Ordering::Acquire) {
                stats.at_capacity_count += 1;
            }

            if stats
                .committed_distribution
                .increment(committed_pages)
                .is_err()
            {
                return None;
            }
        }
        stats.committed_distribution.sort();
        Some(stats)
    }

    const fn memory_footprint(&self) -> FiberStackMemoryFootprint {
        let usable_stack_bytes = match &self.backing {
            FiberStackBackingState::Fixed(layout) => {
                layout.usable_size.saturating_mul(self.capacity)
            }
            FiberStackBackingState::Elastic { layout, .. } => {
                layout.max.saturating_mul(self.capacity)
            }
        };
        FiberStackMemoryFootprint {
            total_capacity: self.capacity,
            reserved_stack_bytes: self.region.len,
            usable_stack_bytes,
            metadata_bytes: self.metadata_bytes,
        }
    }

    const fn max_stack_bytes(&self) -> usize {
        match &self.backing {
            FiberStackBackingState::Fixed(layout) => layout.usable_size,
            FiberStackBackingState::Elastic { layout, .. } => layout.max,
        }
    }

    const fn supports_task_class(&self, class: FiberStackClass) -> bool {
        class.size_bytes().get() <= self.max_stack_bytes()
    }

    fn default_task_class(&self) -> Result<FiberStackClass, FiberError> {
        let max = self.max_stack_bytes();
        if max < FiberStackClass::MIN.size_bytes().get() {
            return Err(FiberError::unsupported());
        }
        let highest_bit = usize::BITS - 1 - max.leading_zeros();
        let class_bytes = 1_usize
            .checked_shl(highest_bit)
            .ok_or_else(FiberError::resource_exhausted)?;
        FiberStackClass::new(NonZeroUsize::new(class_bytes).ok_or_else(FiberError::invalid)?)
    }

    fn current_committed_pages(meta: &ElasticStackMeta) -> u32 {
        if !meta.occupied.load(Ordering::Acquire) {
            return 0;
        }
        if meta.at_capacity.load(Ordering::Acquire) {
            return meta.max_committed_pages;
        }
        let detector = meta.detector_page.load(Ordering::Acquire);
        if detector == 0 {
            return meta.max_committed_pages;
        }

        let committed_with_detector = (meta.reservation_end - detector) / meta.page_size;
        let usable_pages = committed_with_detector.saturating_sub(1);
        u32::try_from(usable_pages).unwrap_or(meta.max_committed_pages)
    }

    fn acquire_slot_index(&self) -> Result<usize, FiberError> {
        let mut state = self.state.lock().map_err(fiber_error_from_sync)?;
        if state.free.len == 0 && matches!(self.growth, GreenGrowth::OnDemand) {
            self.grow_locked(&mut state)?;
        }
        let slot_index = state
            .free
            .pop()
            .ok_or_else(FiberError::resource_exhausted)?;
        state.allocated[slot_index] = true;
        self.mark_slot_allocated(slot_index)?;
        Ok(slot_index)
    }

    fn grow_locked(&self, state: &mut FiberStackSlabState) -> Result<(), FiberError> {
        if state.committed_slots >= self.capacity {
            return Err(FiberError::resource_exhausted());
        }

        let start = state.committed_slots;
        let end = self.capacity.min(
            start
                .checked_add(self.chunk_size)
                .ok_or_else(FiberError::resource_exhausted)?,
        );
        self.initialize_slot_range(start, end)?;
        for slot_index in start..end {
            state.free.push(slot_index)?;
        }
        state.committed_slots = end;
        Ok(())
    }

    fn try_shrink_locked(&self, state: &mut FiberStackSlabState) -> Result<(), FiberError> {
        if !matches!(self.growth, GreenGrowth::OnDemand) {
            return Ok(());
        }

        while state.committed_slots > self.initial_slots {
            let Some((tail_start, tail_end)) = self.chunk_range_ending_at(state.committed_slots)
            else {
                return Err(FiberError::state_conflict());
            };
            let Some((prev_start, prev_end)) = self.chunk_range_ending_at(tail_start) else {
                break;
            };
            if !Self::chunk_is_free(state, tail_start, tail_end)
                || !Self::chunk_is_free(state, prev_start, prev_end)
            {
                break;
            }

            self.deinitialize_slot_range(tail_start, tail_end)?;
            state.committed_slots = tail_start;
            state.free.retain_less_than(tail_start);
        }

        Ok(())
    }

    fn chunk_is_free(state: &FiberStackSlabState, start: usize, end: usize) -> bool {
        !state.allocated[start..end]
            .iter()
            .any(|allocated| *allocated)
    }

    fn chunk_range_ending_at(&self, end: usize) -> Option<(usize, usize)> {
        if end == 0 || end > self.capacity {
            return None;
        }
        let chunk_len = match end % self.chunk_size {
            0 => self.chunk_size,
            remainder => remainder,
        };
        Some((end.checked_sub(chunk_len)?, end))
    }

    fn initialize_slot_range(&self, start: usize, end: usize) -> Result<(), FiberError> {
        for slot_index in start..end {
            self.initialize_slot(slot_index)?;
        }
        Ok(())
    }

    fn deinitialize_slot_range(&self, start: usize, end: usize) -> Result<(), FiberError> {
        for slot_index in start..end {
            self.deinitialize_slot(slot_index)?;
        }
        Ok(())
    }

    fn initialize_slot(&self, slot_index: usize) -> Result<(), FiberError> {
        match &self.backing {
            FiberStackBackingState::Fixed(layout) => {
                if !self.storage_uses_mem_protect() {
                    return Ok(());
                }
                let memory = system_mem();
                let usable = self.fixed_usable_region(slot_index, *layout)?;
                unsafe { memory.protect(usable, Protect::READ | Protect::WRITE) }
                    .map_err(fiber_error_from_mem)
            }
            FiberStackBackingState::Elastic { layout, metadata } => {
                let memory = system_mem();
                let usable = self.elastic_initial_usable_region(slot_index, *layout)?;
                unsafe { memory.protect(usable, Protect::READ | Protect::WRITE) }
                    .map_err(fiber_error_from_mem)?;
                Self::reset_elastic_metadata(slot_index, metadata)
            }
        }
    }

    fn deinitialize_slot(&self, slot_index: usize) -> Result<(), FiberError> {
        match &self.backing {
            FiberStackBackingState::Fixed(_) => Ok(()),
            FiberStackBackingState::Elastic { metadata, .. } => {
                let memory = system_mem();
                let slot = self.slot_region(slot_index)?;
                unsafe { memory.protect(slot, Protect::NONE) }.map_err(fiber_error_from_mem)?;
                Self::reset_elastic_metadata(slot_index, metadata)
            }
        }
    }

    fn reset_slot(&self, slot_index: usize) -> Result<(), FiberError> {
        match &self.backing {
            FiberStackBackingState::Fixed(_) => Ok(()),
            FiberStackBackingState::Elastic { layout, metadata } => {
                let memory = system_mem();
                let slot = self.slot_region(slot_index)?;
                unsafe { memory.protect(slot, Protect::NONE) }.map_err(fiber_error_from_mem)?;
                let usable = self.elastic_initial_usable_region(slot_index, *layout)?;
                unsafe { memory.protect(usable, Protect::READ | Protect::WRITE) }
                    .map_err(fiber_error_from_mem)?;
                Self::reset_elastic_metadata(slot_index, metadata)
            }
        }
    }

    fn reset_elastic_metadata(
        slot_index: usize,
        metadata: &MetadataSlice<ElasticStackMeta>,
    ) -> Result<(), FiberError> {
        let meta = metadata.get(slot_index).ok_or_else(FiberError::invalid)?;
        meta.detector_page
            .store(meta.initial_detector_page, Ordering::Release);
        meta.guard_page
            .store(meta.initial_guard_page, Ordering::Release);
        meta.at_capacity.store(false, Ordering::Release);
        meta.capacity_pending.store(false, Ordering::Release);
        meta.fiber_id.store(0, Ordering::Release);
        meta.carrier_id.store(0, Ordering::Release);
        meta.capacity_token.store(
            wake_token_to_word(PlatformWakeToken::invalid()),
            Ordering::Release,
        );
        meta.occupied.store(false, Ordering::Release);
        meta.growth_events.store(0, Ordering::Release);
        meta.committed_pages.store(0, Ordering::Release);
        Ok(())
    }

    fn mark_slot_allocated(&self, slot_index: usize) -> Result<(), FiberError> {
        if let FiberStackBackingState::Fixed(layout) = &self.backing
            && !matches!(self.telemetry, FiberTelemetry::Disabled)
        {
            self.paint_fixed_slot(slot_index, *layout)?;
        }

        let FiberStackBackingState::Elastic { metadata, .. } = &self.backing else {
            return Ok(());
        };
        let meta = metadata.get(slot_index).ok_or_else(FiberError::invalid)?;
        meta.occupied.store(true, Ordering::Release);
        meta.growth_events.store(0, Ordering::Release);
        meta.committed_pages
            .store(meta.initial_committed_pages, Ordering::Release);
        meta.at_capacity.store(false, Ordering::Release);
        meta.capacity_pending.store(false, Ordering::Release);
        Ok(())
    }

    fn paint_fixed_slot(
        &self,
        slot_index: usize,
        layout: FixedStackLayout,
    ) -> Result<(), FiberError> {
        let usable = self.fixed_usable_region(slot_index, layout)?;
        // SAFETY: the slot's usable stack region is writable while the slot is reserved to this slab.
        unsafe {
            ptr::write_bytes(
                usable.base.get() as *mut u8,
                FIXED_STACK_WATERMARK_SENTINEL,
                usable.len,
            );
        }
        Ok(())
    }

    fn observe_fixed_slot_usage(
        &self,
        slot_index: usize,
        layout: FixedStackLayout,
    ) -> Result<usize, FiberError> {
        let usable = self.fixed_usable_region(slot_index, layout)?;
        // SAFETY: the slot remains mapped and readable until the slab releases it.
        let bytes =
            unsafe { core::slice::from_raw_parts(usable.base.get() as *const u8, usable.len) };
        let used = match self.stack_direction {
            ContextStackDirection::Down => bytes
                .iter()
                .position(|byte| *byte != FIXED_STACK_WATERMARK_SENTINEL)
                .map_or(0, |index| usable.len.saturating_sub(index)),
            ContextStackDirection::Up => bytes
                .iter()
                .rposition(|byte| *byte != FIXED_STACK_WATERMARK_SENTINEL)
                .map_or(0, |index| index.saturating_add(1)),
            ContextStackDirection::Unknown => return Err(FiberError::unsupported()),
        };
        Ok(used)
    }

    fn attach_slot_identity(
        &self,
        slot_index: usize,
        fiber_id: u64,
        carrier_id: usize,
        capacity_token: PlatformWakeToken,
    ) -> Result<(), FiberError> {
        let FiberStackBackingState::Elastic { metadata, .. } = &self.backing else {
            return Ok(());
        };
        let meta = metadata.get(slot_index).ok_or_else(FiberError::invalid)?;
        meta.fiber_id.store(
            usize::try_from(fiber_id).unwrap_or(usize::MAX),
            Ordering::Release,
        );
        meta.carrier_id.store(carrier_id, Ordering::Release);
        meta.capacity_token
            .store(wake_token_to_word(capacity_token), Ordering::Release);
        Ok(())
    }

    fn take_capacity_event(
        &self,
        slot_index: usize,
    ) -> Result<Option<FiberCapacityEvent>, FiberError> {
        let FiberStackBackingState::Elastic { metadata, .. } = &self.backing else {
            return Ok(None);
        };
        let meta = metadata.get(slot_index).ok_or_else(FiberError::invalid)?;
        if !meta.capacity_pending.swap(false, Ordering::AcqRel) {
            return Ok(None);
        }

        Ok(Some(FiberCapacityEvent {
            fiber_id: meta.fiber_id.load(Ordering::Acquire) as u64,
            carrier_id: meta.carrier_id.load(Ordering::Acquire),
            committed_pages: Self::current_committed_pages(meta),
            reservation_pages: meta.max_committed_pages,
        }))
    }

    fn dispatch_capacity_event(
        &self,
        slot_index: usize,
        policy: CapacityPolicy,
    ) -> Result<(), FiberError> {
        let CapacityPolicy::Notify(callback) = policy else {
            return Ok(());
        };
        if let Some(event) = self.take_capacity_event(slot_index)? {
            run_capacity_callback_contained(callback, event);
        }
        Ok(())
    }
}

impl Drop for FiberStackSlab {
    fn drop(&mut self) {
        if let FiberStackBackingState::Elastic { metadata, .. } = &self.backing {
            for meta in metadata.as_slice() {
                meta.active.store(false, Ordering::Release);
            }
            let _ = unregister_elastic_stack_metadata(metadata.as_slice());
        }
        match &mut self.storage {
            FiberStackSlabStorage::VirtualCombined(mapping) => {
                let _ = unsafe { system_mem().unmap(*mapping) };
            }
            FiberStackSlabStorage::Explicit { stack, metadata } => {
                let _ = stack.resolved();
                let _ = metadata.resolved();
            }
        }
    }
}

impl FiberStackClassPools {
    fn new(
        config: &FiberPoolConfig<'_>,
        alignment: usize,
        stack_direction: ContextStackDirection,
    ) -> Result<Self, FiberError> {
        if config.classes.is_empty() {
            return Err(FiberError::invalid());
        }

        let memory = system_mem();
        let page = memory.page_info().alloc_granule.get();
        let bytes = apply_fiber_sizing_strategy_bytes(
            size_of::<FiberStackPoolEntry>()
                .checked_mul(config.classes.len())
                .ok_or_else(FiberError::resource_exhausted)?,
            config.sizing,
        )?;
        let len = fiber_align_up(bytes, page)?;
        let mapping = unsafe {
            memory.map(&MapRequest {
                len,
                align: page.max(align_of::<FiberStackPoolEntry>()),
                protect: Protect::NONE,
                flags: MapFlags::PRIVATE,
                attrs: RegionAttrs::VIRTUAL_ONLY,
                cache: CachePolicy::Default,
                placement: Placement::Anywhere,
                backing: Backing::Anonymous,
            })
        }
        .map_err(fiber_error_from_mem)?;
        unsafe { memory.protect(mapping, Protect::READ | Protect::WRITE) }
            .map_err(fiber_error_from_mem)?;

        let entries = mapping
            .base
            .as_non_null::<FiberStackPoolEntry>()
            .ok_or_else(FiberError::invalid)?;
        let mut initialized = 0;
        let mut total_capacity: usize = 0;
        let result = (|| {
            for (index, class) in config.classes.iter().copied().enumerate() {
                if class.slots_per_carrier == 0 {
                    return Err(FiberError::invalid());
                }
                if index != 0 && config.classes[index - 1].class >= class.class {
                    return Err(FiberError::invalid());
                }

                let class_config = FiberPoolConfig {
                    stack_backing: FiberStackBacking::Fixed {
                        stack_size: class.class.size_bytes(),
                    },
                    sizing: config.sizing,
                    classes: &[],
                    guard_pages: config.guard_pages,
                    growth_chunk: class.growth_chunk,
                    max_fibers_per_carrier: class.slots_per_carrier,
                    scheduling: config.scheduling,
                    priority_age_cap: config.priority_age_cap,
                    growth: config.growth,
                    telemetry: FiberTelemetry::Disabled,
                    capacity_policy: CapacityPolicy::Abort,
                    yield_budget_policy: FiberYieldBudgetPolicy::Abort,
                    reactor_policy: config.reactor_policy,
                    huge_pages: config.huge_pages,
                    courier_id: config.courier_id,
                    context_id: config.context_id,
                    runtime_sink: config.runtime_sink,
                    launch_control: config.launch_control,
                    launch_request: config.launch_request,
                };
                let slab = FiberStackSlab::new(&class_config, alignment, stack_direction)?;
                unsafe {
                    entries.as_ptr().add(index).write(FiberStackPoolEntry {
                        class: class.class,
                        slab,
                    });
                }
                initialized += 1;
                total_capacity = total_capacity
                    .checked_add(class.slots_per_carrier)
                    .ok_or_else(FiberError::resource_exhausted)?;
            }
            Ok(())
        })();

        if let Err(error) = result {
            for index in 0..initialized {
                unsafe {
                    entries.as_ptr().add(index).drop_in_place();
                }
            }
            let _ = unsafe { memory.unmap(mapping) };
            return Err(error);
        }

        Ok(Self {
            mapping,
            entries,
            len: config.classes.len(),
            total_capacity,
        })
    }

    const fn as_slice(&self) -> &[FiberStackPoolEntry] {
        unsafe { core::slice::from_raw_parts(self.entries.as_ptr(), self.len) }
    }

    fn entry(&self, index: usize) -> Result<&FiberStackPoolEntry, FiberError> {
        self.as_slice().get(index).ok_or_else(FiberError::invalid)
    }

    fn matching_pool_index(&self, class: FiberStackClass) -> Option<usize> {
        self.as_slice()
            .iter()
            .position(|entry| entry.class >= class)
    }

    fn supports_task_class(&self, class: FiberStackClass) -> bool {
        self.matching_pool_index(class).is_some()
    }

    fn default_task_class(&self) -> Result<FiberStackClass, FiberError> {
        self.as_slice()
            .last()
            .map(|entry| entry.class)
            .ok_or_else(FiberError::invalid)
    }

    fn acquire(&self, task: FiberTaskAttributes) -> Result<FiberStackLease, FiberError> {
        let pool_index = self
            .matching_pool_index(task.stack_class)
            .ok_or_else(FiberError::unsupported)?;
        let entry = self.entry(pool_index)?;
        let lease = entry.slab.acquire()?;
        Ok(FiberStackLease {
            pool_index,
            slot_index: lease.slot_index,
            class: entry.class,
            stack: lease.stack,
        })
    }

    fn release(&self, pool_index: usize, slot_index: usize) -> Result<(), FiberError> {
        self.entry(pool_index)?.slab.release(slot_index)
    }

    fn attach_slot_identity(
        &self,
        pool_index: usize,
        slot_index: usize,
        fiber_id: u64,
        carrier_id: usize,
        capacity_token: PlatformWakeToken,
    ) -> Result<(), FiberError> {
        self.entry(pool_index)?.slab.attach_slot_identity(
            slot_index,
            fiber_id,
            carrier_id,
            capacity_token,
        )
    }

    fn dispatch_capacity_event(
        &self,
        pool_index: usize,
        slot_index: usize,
        policy: CapacityPolicy,
    ) -> Result<(), FiberError> {
        self.entry(pool_index)?
            .slab
            .dispatch_capacity_event(slot_index, policy)
    }

    fn requires_signal_handler(&self) -> bool {
        self.as_slice()
            .iter()
            .any(|entry| entry.slab.requires_signal_handler())
    }

    fn memory_footprint(&self) -> FiberStackMemoryFootprint {
        let mut footprint = FiberStackMemoryFootprint {
            total_capacity: 0,
            reserved_stack_bytes: 0,
            usable_stack_bytes: 0,
            metadata_bytes: self.mapping.len,
        };
        for entry in self.as_slice() {
            let slab = entry.slab.memory_footprint();
            footprint.total_capacity = footprint.total_capacity.saturating_add(slab.total_capacity);
            footprint.reserved_stack_bytes = footprint
                .reserved_stack_bytes
                .saturating_add(slab.reserved_stack_bytes);
            footprint.usable_stack_bytes = footprint
                .usable_stack_bytes
                .saturating_add(slab.usable_stack_bytes);
            footprint.metadata_bytes = footprint.metadata_bytes.saturating_add(slab.metadata_bytes);
        }
        footprint
    }
}

impl Drop for FiberStackClassPools {
    fn drop(&mut self) {
        for index in 0..self.len {
            unsafe {
                self.entries.as_ptr().add(index).drop_in_place();
            }
        }
        let _ = unsafe { system_mem().unmap(self.mapping) };
    }
}

impl FiberStackStore {
    fn new(
        config: &FiberPoolConfig<'_>,
        alignment: usize,
        stack_direction: ContextStackDirection,
    ) -> Result<Self, FiberError> {
        if config.classes.is_empty() {
            return Ok(Self::Legacy(FiberStackSlab::new(
                config,
                alignment,
                stack_direction,
            )?));
        }
        Ok(Self::Classes(FiberStackClassPools::new(
            config,
            alignment,
            stack_direction,
        )?))
    }

    const fn total_capacity(&self) -> usize {
        match self {
            Self::Legacy(slab) => slab.capacity,
            Self::Classes(pools) => pools.total_capacity,
        }
    }

    fn supports_task_class(&self, class: FiberStackClass) -> bool {
        match self {
            Self::Legacy(slab) => slab.supports_task_class(class),
            Self::Classes(pools) => pools.supports_task_class(class),
        }
    }

    fn default_task_class(&self) -> Result<FiberStackClass, FiberError> {
        match self {
            Self::Legacy(slab) => slab.default_task_class(),
            Self::Classes(pools) => pools.default_task_class(),
        }
    }

    fn acquire(&self, task: FiberTaskAttributes) -> Result<FiberStackLease, FiberError> {
        match self {
            Self::Legacy(slab) => {
                let lease = slab.acquire()?;
                Ok(FiberStackLease {
                    pool_index: 0,
                    slot_index: lease.slot_index,
                    class: task.stack_class,
                    stack: lease.stack,
                })
            }
            Self::Classes(pools) => pools.acquire(task),
        }
    }

    fn release(&self, pool_index: usize, slot_index: usize) -> Result<(), FiberError> {
        match self {
            Self::Legacy(slab) => {
                if pool_index != 0 {
                    return Err(FiberError::invalid());
                }
                slab.release(slot_index)
            }
            Self::Classes(pools) => pools.release(pool_index, slot_index),
        }
    }

    fn attach_slot_identity(
        &self,
        pool_index: usize,
        slot_index: usize,
        fiber_id: u64,
        carrier_id: usize,
        capacity_token: PlatformWakeToken,
    ) -> Result<(), FiberError> {
        match self {
            Self::Legacy(slab) => {
                if pool_index != 0 {
                    return Err(FiberError::invalid());
                }
                slab.attach_slot_identity(slot_index, fiber_id, carrier_id, capacity_token)
            }
            Self::Classes(pools) => pools.attach_slot_identity(
                pool_index,
                slot_index,
                fiber_id,
                carrier_id,
                capacity_token,
            ),
        }
    }

    fn dispatch_capacity_event(
        &self,
        pool_index: usize,
        slot_index: usize,
        policy: CapacityPolicy,
    ) -> Result<(), FiberError> {
        match self {
            Self::Legacy(slab) => {
                if pool_index != 0 {
                    return Err(FiberError::invalid());
                }
                slab.dispatch_capacity_event(slot_index, policy)
            }
            Self::Classes(pools) => pools.dispatch_capacity_event(pool_index, slot_index, policy),
        }
    }

    fn requires_signal_handler(&self) -> bool {
        match self {
            Self::Legacy(slab) => slab.requires_signal_handler(),
            Self::Classes(pools) => pools.requires_signal_handler(),
        }
    }

    fn stack_stats(&self) -> Option<FiberStackStats> {
        match self {
            Self::Legacy(slab) => slab.stack_stats(),
            Self::Classes(_) => None,
        }
    }

    fn memory_footprint(&self) -> FiberStackMemoryFootprint {
        match self {
            Self::Legacy(slab) => slab.memory_footprint(),
            Self::Classes(pools) => pools.memory_footprint(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct ElasticRegistryEntry {
    reservation_base: usize,
    reservation_end: usize,
    meta: usize,
}

impl ElasticRegistryEntry {
    fn new(meta: &ElasticStackMeta) -> Self {
        Self {
            reservation_base: meta.reservation_base,
            reservation_end: meta.reservation_end,
            meta: core::ptr::from_ref(meta) as usize,
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct ElasticRegistrySnapshotHeader {
    len: usize,
    entries_offset: usize,
}

#[derive(Debug)]
struct ElasticRegistrySnapshot {
    region: Region,
    header: core::ptr::NonNull<ElasticRegistrySnapshotHeader>,
}

impl ElasticRegistrySnapshot {
    fn new(entries: &[ElasticRegistryEntry]) -> Result<Option<Self>, FiberError> {
        if entries.is_empty() {
            return Ok(None);
        }

        let memory = system_mem();
        let page = memory.page_info().alloc_granule.get();
        let entries_offset = fiber_align_up(
            size_of::<ElasticRegistrySnapshotHeader>(),
            align_of::<ElasticRegistryEntry>(),
        )?;
        let entries_bytes = size_of::<ElasticRegistryEntry>()
            .checked_mul(entries.len())
            .ok_or_else(FiberError::resource_exhausted)?;
        let mapping_len = fiber_align_up(
            entries_offset
                .checked_add(entries_bytes)
                .ok_or_else(FiberError::resource_exhausted)?,
            page,
        )?;

        let region = unsafe {
            memory.map(&MapRequest {
                len: mapping_len,
                align: page.max(align_of::<ElasticRegistrySnapshotHeader>()),
                protect: Protect::NONE,
                flags: MapFlags::PRIVATE,
                attrs: RegionAttrs::VIRTUAL_ONLY,
                cache: CachePolicy::Default,
                placement: Placement::Anywhere,
                backing: Backing::Anonymous,
            })
        }
        .map_err(fiber_error_from_mem)?;
        unsafe { memory.protect(region, Protect::READ | Protect::WRITE) }
            .map_err(fiber_error_from_mem)?;

        let header = core::ptr::NonNull::new(region.base.cast::<ElasticRegistrySnapshotHeader>())
            .ok_or_else(FiberError::invalid)?;
        let entries_ptr = region
            .base
            .get()
            .checked_add(entries_offset)
            .ok_or_else(FiberError::resource_exhausted)?
            as *mut ElasticRegistryEntry;
        debug_assert_eq!(
            entries_ptr.align_offset(align_of::<ElasticRegistryEntry>()),
            0
        );
        unsafe {
            header.as_ptr().write(ElasticRegistrySnapshotHeader {
                len: entries.len(),
                entries_offset,
            });
            core::ptr::copy_nonoverlapping(entries.as_ptr(), entries_ptr, entries.len());
        }

        Ok(Some(Self { region, header }))
    }

    const fn header_ptr(&self) -> *const ElasticRegistrySnapshotHeader {
        self.header.as_ptr()
    }
}

impl Drop for ElasticRegistrySnapshot {
    fn drop(&mut self) {
        let _ = unsafe { system_mem().unmap(self.region) };
    }
}

// SAFETY: snapshots are immutable after publication and keep their backing mapping alive until
// dropped after the reader drain barrier.
unsafe impl Send for ElasticRegistrySnapshot {}
// SAFETY: see above.
unsafe impl Sync for ElasticRegistrySnapshot {}

#[derive(Debug)]
struct ElasticRegistryState {
    pointers: MappedVec<usize>,
    snapshot: Option<ElasticRegistrySnapshot>,
}

static ELASTIC_STACK_REGISTRY: OnceLock<SyncMutex<ElasticRegistryState>> = OnceLock::new();
static ELASTIC_STACK_SNAPSHOT: AtomicUsize = AtomicUsize::new(0);
static ELASTIC_STACK_READERS: AtomicUsize = AtomicUsize::new(0);

fn elastic_registry() -> Result<&'static SyncMutex<ElasticRegistryState>, FiberError> {
    ELASTIC_STACK_REGISTRY
        .get_or_init(|| {
            SyncMutex::new(ElasticRegistryState {
                pointers: MappedVec::new(),
                snapshot: None,
            })
        })
        .map_err(fiber_error_from_sync)
}

fn register_elastic_stack_metadata(metadata: &[ElasticStackMeta]) -> Result<(), FiberError> {
    let registry = elastic_registry()?;
    let mut state = registry.lock().map_err(fiber_error_from_sync)?;
    let previous_len = state.pointers.len();
    for meta in metadata {
        if let Err(error) = state.pointers.push(core::ptr::from_ref(meta) as usize) {
            state.pointers.truncate(previous_len);
            return Err(error);
        }
    }
    let next_snapshot = build_elastic_snapshot(state.pointers.as_slice())?;
    commit_elastic_snapshot(&mut state, next_snapshot);
    Ok(())
}

fn unregister_elastic_stack_metadata(metadata: &[ElasticStackMeta]) -> Result<(), FiberError> {
    let registry = elastic_registry()?;
    let mut state = registry.lock().map_err(fiber_error_from_sync)?;
    state.pointers.retain(|meta_ptr| {
        !metadata
            .iter()
            .any(|meta| core::ptr::from_ref(meta) as usize == *meta_ptr)
    });
    let next_snapshot = build_elastic_snapshot(state.pointers.as_slice())?;
    commit_elastic_snapshot(&mut state, next_snapshot);
    Ok(())
}

fn build_elastic_snapshot(
    pointers: &[usize],
) -> Result<Option<ElasticRegistrySnapshot>, FiberError> {
    let mut entries = MappedVec::with_capacity(pointers.len())?;
    for meta_ptr in pointers {
        let meta = unsafe { &*(*meta_ptr as *const ElasticStackMeta) };
        entries.push(ElasticRegistryEntry::new(meta))?;
    }
    entries.sort_by_key(|entry| entry.reservation_base);
    ElasticRegistrySnapshot::new(entries.as_slice())
}

fn commit_elastic_snapshot(
    state: &mut ElasticRegistryState,
    next_snapshot: Option<ElasticRegistrySnapshot>,
) {
    let next_ptr = next_snapshot
        .as_ref()
        .map_or(0, |snapshot| snapshot.header_ptr() as usize);
    ELASTIC_STACK_SNAPSHOT.store(next_ptr, Ordering::Release);
    let previous = core::mem::replace(&mut state.snapshot, next_snapshot);
    wait_for_elastic_readers_to_drain();
    drop(previous);
}

#[allow(clippy::missing_const_for_fn)]
fn snapshot_entries(snapshot: &ElasticRegistrySnapshotHeader) -> &[ElasticRegistryEntry] {
    // SAFETY: published snapshots point at a live immutable header inside a mapped snapshot
    // region, and the entry payload immediately follows at `entries_offset`.
    let entries_ptr = (core::ptr::from_ref(snapshot).addr() + snapshot.entries_offset)
        as *const ElasticRegistryEntry;
    unsafe { core::slice::from_raw_parts(entries_ptr, snapshot.len) }
}

fn wait_for_elastic_readers_to_drain() {
    while ELASTIC_STACK_READERS.load(Ordering::Acquire) != 0 {
        core::hint::spin_loop();
    }
}

fn find_snapshot_elastic_entry(
    snapshot: &ElasticRegistrySnapshotHeader,
    fault_addr: usize,
) -> Option<ElasticRegistryEntry> {
    let entries = snapshot_entries(snapshot);
    let mut low = 0;
    let mut high = entries.len();
    while low < high {
        let mid = low + ((high - low) / 2);
        let entry = entries[mid];
        if fault_addr < entry.reservation_base {
            high = mid;
        } else if fault_addr >= entry.reservation_end {
            low = mid + 1;
        } else {
            return Some(entry);
        }
    }
    None
}

fn try_promote_elastic_stack_meta(meta: &ElasticStackMeta, fault_addr: usize) -> bool {
    if !meta.active.load(Ordering::Acquire) {
        return false;
    }

    let detector = meta.detector_page.load(Ordering::Acquire);
    let guard = meta.guard_page.load(Ordering::Acquire);
    if fault_addr >= guard && fault_addr < guard.saturating_add(meta.page_size) {
        // Guard-page faults are true stack overflow and must chain to the previous handler.
        return false;
    }
    if fault_addr < detector || fault_addr >= detector.saturating_add(meta.page_size) {
        return false;
    }
    if meta.at_capacity.load(Ordering::Acquire) {
        return false;
    }

    if system_fiber_host()
        .promote_elastic_page(detector, meta.page_size)
        .is_err()
    {
        return false;
    }

    let committed_pages =
        u32::try_from((meta.reservation_end - detector) / meta.page_size).unwrap_or(u32::MAX);
    let next_detector = guard;
    let next_guard = guard.saturating_sub(meta.page_size);
    let previously_at_capacity = meta.at_capacity.load(Ordering::Acquire);
    let at_capacity = next_guard <= meta.reservation_base;
    meta.detector_page.store(next_detector, Ordering::Release);
    meta.guard_page.store(next_guard, Ordering::Release);
    meta.at_capacity.store(at_capacity, Ordering::Release);
    if at_capacity && !previously_at_capacity {
        meta.capacity_pending.store(true, Ordering::Release);
        let token = word_to_wake_token(meta.capacity_token.load(Ordering::Acquire));
        let _ = system_fiber_host().notify_wake_token(token);
    }
    if !matches!(meta.telemetry, FiberTelemetry::Disabled) {
        meta.growth_events.fetch_add(1, Ordering::Relaxed);
        if matches!(meta.telemetry, FiberTelemetry::Full) {
            let _ = meta
                .committed_pages
                .fetch_max(committed_pages, Ordering::Relaxed);
        }
    }
    true
}

fn elastic_stack_fault_handler(fault_addr: usize) -> bool {
    if fault_addr == 0 {
        return false;
    }
    try_promote_elastic_stack_fault(fault_addr)
}

fn try_promote_elastic_stack_fault(fault_addr: usize) -> bool {
    ELASTIC_STACK_READERS.fetch_add(1, Ordering::Acquire);
    let snapshot_ptr =
        ELASTIC_STACK_SNAPSHOT.load(Ordering::Acquire) as *const ElasticRegistrySnapshotHeader;
    let promoted = if snapshot_ptr.is_null() {
        false
    } else {
        let snapshot = unsafe { &*snapshot_ptr };
        let Some(entry) = find_snapshot_elastic_entry(snapshot, fault_addr) else {
            ELASTIC_STACK_READERS.fetch_sub(1, Ordering::Release);
            return false;
        };
        let meta = unsafe { &*(entry.meta as *const ElasticStackMeta) };
        try_promote_elastic_stack_meta(meta, fault_addr)
    };
    ELASTIC_STACK_READERS.fetch_sub(1, Ordering::Release);
    promoted
}
