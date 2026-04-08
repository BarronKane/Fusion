/// Backing request for one current-thread fiber-pool storage domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FiberPoolBackingRequest {
    /// Minimum bytes the backing resource should expose for this domain.
    pub bytes: usize,
    /// Maximum alignment this domain may honestly require.
    pub align: usize,
}

/// Explicit backing plan for one current-thread fiber pool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CurrentFiberPoolBackingPlan {
    /// Control block backing.
    pub control: FiberPoolBackingRequest,
    /// Green runtime metadata backing.
    pub runtime_metadata: FiberPoolBackingRequest,
    /// Fiber stack-slab metadata backing.
    pub stack_metadata: FiberPoolBackingRequest,
    /// Fiber stack payload backing.
    pub stacks: FiberPoolBackingRequest,
}

/// Packed one-slab layout for one current-thread fiber pool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CurrentFiberPoolCombinedBackingPlan {
    /// Total owning slab request for all current-thread fiber-pool domains.
    pub slab: FiberPoolBackingRequest,
    /// Control block range inside the slab.
    pub control: ResourceRange,
    /// Green runtime metadata range inside the slab.
    pub runtime_metadata: ResourceRange,
    /// Fiber stack-slab metadata range inside the slab.
    pub stack_metadata: ResourceRange,
    /// Fiber stack payload range inside the slab.
    pub stacks: ResourceRange,
}

fn align_up_packed(offset: usize, align: usize) -> Result<usize, FiberError> {
    if align == 0 || !align.is_power_of_two() {
        return Err(FiberError::invalid());
    }
    let mask = align - 1;
    offset
        .checked_add(mask)
        .map(|value| value & !mask)
        .ok_or_else(FiberError::resource_exhausted)
}

const fn bound_partition_backing_kind(
    kind: ResourceBackingKind,
) -> Result<ResourceBackingKind, FiberError> {
    match kind {
        ResourceBackingKind::Borrowed
        | ResourceBackingKind::StaticRegion
        | ResourceBackingKind::Partition => Ok(ResourceBackingKind::Partition),
        _ => Err(FiberError::unsupported()),
    }
}

fn partition_bound_resource(
    handle: &MemoryResourceHandle,
    range: ResourceRange,
) -> Result<MemoryResourceHandle, FiberError> {
    let info = match handle {
        MemoryResourceHandle::Bound(resource) => resource.info(),
        MemoryResourceHandle::Virtual(_) => return Err(FiberError::unsupported()),
    };
    let region = handle
        .subview(range)
        .map(|view| unsafe { view.raw_region() })
        .map_err(|_| FiberError::invalid())?;
    let resource = BoundMemoryResource::new(BoundResourceSpec::new(
        region,
        info.domain,
        bound_partition_backing_kind(info.backing)?,
        info.attrs,
        info.geometry,
        info.layout,
        info.contract,
        info.support,
        handle.state(),
    ))
    .map_err(|_| FiberError::invalid())?;
    Ok(MemoryResourceHandle::from(resource))
}

const fn fiber_resource_base_alignment_from_addr(addr: usize) -> usize {
    if addr == 0 {
        1
    } else {
        1usize << addr.trailing_zeros()
    }
}

fn fiber_resource_base_alignment(handle: &MemoryResourceHandle) -> usize {
    fiber_resource_base_alignment_from_addr(handle.view().base_addr().get())
}

impl CurrentFiberPoolBackingPlan {
    fn combined_with_base_alignment(
        self,
        base_align: usize,
    ) -> Result<CurrentFiberPoolCombinedBackingPlan, FiberError> {
        let mut max_align = self.control.align;
        if self.runtime_metadata.align > max_align {
            max_align = self.runtime_metadata.align;
        }
        if self.stack_metadata.align > max_align {
            max_align = self.stack_metadata.align;
        }
        if self.stacks.align > max_align {
            max_align = self.stacks.align;
        }

        let mut cursor = if base_align >= max_align {
            0
        } else {
            max_align.saturating_sub(1)
        };
        let control_offset = align_up_packed(cursor, self.control.align)?;
        cursor = control_offset
            .checked_add(self.control.bytes)
            .ok_or_else(FiberError::resource_exhausted)?;
        let runtime_metadata_offset = align_up_packed(cursor, self.runtime_metadata.align)?;
        cursor = runtime_metadata_offset
            .checked_add(self.runtime_metadata.bytes)
            .ok_or_else(FiberError::resource_exhausted)?;
        let stack_metadata_offset = align_up_packed(cursor, self.stack_metadata.align)?;
        cursor = stack_metadata_offset
            .checked_add(self.stack_metadata.bytes)
            .ok_or_else(FiberError::resource_exhausted)?;
        let stacks_offset = align_up_packed(cursor, self.stacks.align)?;
        let total_bytes = stacks_offset
            .checked_add(self.stacks.bytes)
            .ok_or_else(FiberError::resource_exhausted)?;

        Ok(CurrentFiberPoolCombinedBackingPlan {
            slab: FiberPoolBackingRequest {
                bytes: total_bytes,
                align: max_align,
            },
            control: ResourceRange::new(control_offset, self.control.bytes),
            runtime_metadata: ResourceRange::new(
                runtime_metadata_offset,
                self.runtime_metadata.bytes,
            ),
            stack_metadata: ResourceRange::new(stack_metadata_offset, self.stack_metadata.bytes),
            stacks: ResourceRange::new(stacks_offset, self.stacks.bytes),
        })
    }

    /// Packs the per-domain requests into one conservative owning-slab layout.
    ///
    /// The total byte count includes worst-case padding for an arbitrarily aligned caller-owned
    /// slab base.
    pub fn combined(self) -> Result<CurrentFiberPoolCombinedBackingPlan, FiberError> {
        self.combined_with_base_alignment(1)
    }

    /// Packs the per-domain requests into one owning slab for a caller that can guarantee the
    /// slab base is aligned to at least `base_align`.
    ///
    /// When `base_align` satisfies the slab alignment, the layout becomes exact instead of
    /// reserving worst-case arbitrary-base padding.
    pub fn combined_for_base_alignment(
        self,
        base_align: usize,
    ) -> Result<CurrentFiberPoolCombinedBackingPlan, FiberError> {
        self.combined_with_base_alignment(base_align)
    }
}

/// Explicit backing resources for one current-thread fiber pool.
#[derive(Debug)]
pub struct CurrentFiberPoolBacking {
    /// Control block resource.
    pub control: MemoryResourceHandle,
    /// Green runtime metadata resource.
    pub runtime_metadata: MemoryResourceHandle,
    /// Fiber stack-slab metadata resource.
    pub stack_metadata: MemoryResourceHandle,
    /// Fiber stack payload resource.
    pub stacks: MemoryResourceHandle,
    /// Optional owned slab retaining the backing lifetime for partitioned explicit resources.
    pub slab_owner: Option<fusion_sys::alloc::ExtentLease>,
}

/// Public current-thread fiber pool wrapper for manual same-thread driving.
///
/// This is a manual/bootstrap runner surface, not the final autonomous courier runtime model.
#[derive(Debug)]
pub struct CurrentFiberPool {
    inner: GreenPoolLease,
    _not_send_sync: PhantomData<*mut ()>,
}

impl CurrentFiberPool {
    pub(crate) fn install_runtime_dispatch_cookie(
        &self,
        cookie: fusion_pal::sys::runtime_dispatch::RuntimeDispatchCookie,
    ) {
        self.inner.install_runtime_dispatch_cookie(cookie);
    }

    pub(crate) fn default_task_class(&self) -> Result<FiberStackClass, FiberError> {
        self.inner.stacks.default_task_class()
    }

    pub(crate) fn closure_task_attributes<F>(&self) -> Result<FiberTaskAttributes, FiberError>
    where
        F: 'static,
    {
        closure_spawn_task_attributes::<F>(self.default_task_class()?)
    }

    /// Returns the explicit backing plan for one manually-driven current-thread fiber pool.
    ///
    /// This plan is currently honest for the legacy single-slab stack configuration. Class-backed
    /// current-thread pools still use the older hosted-style construction path and are rejected
    /// here until their backing domains are split out properly.
    pub fn backing_plan(
        config: &FiberPoolConfig<'_>,
    ) -> Result<CurrentFiberPoolBackingPlan, FiberError> {
        Self::backing_plan_with_planning_support(
            config,
            FiberPlanningSupport::from_fiber_support(FiberSystem::new().support()),
        )
    }

    /// Returns the explicit backing plan for one manually-driven current-thread fiber pool under
    /// one explicit planning-time context surface.
    ///
    /// This is the build-time honest path for targets like bare metal, where slab sizing should
    /// reflect the target context ABI instead of whatever host happened to run `build.rs`.
    pub fn backing_plan_with_planning_support(
        config: &FiberPoolConfig<'_>,
        planning: FiberPlanningSupport,
    ) -> Result<CurrentFiberPoolBackingPlan, FiberError> {
        let effective_backing =
            apply_fiber_sizing_strategy_backing(config.stack_backing, config.sizing)?;
        if !planning.supports_current_thread() {
            return Err(FiberError::unsupported());
        }
        if planning.guard_required && config.guard_pages == 0 {
            return Err(FiberError::invalid());
        }
        let task_capacity_per_carrier = config.task_capacity_per_carrier()?;
        if config.growth_chunk == 0 || task_capacity_per_carrier == 0 {
            return Err(FiberError::invalid());
        }
        if !config.uses_classes() && config.growth_chunk > config.max_fibers_per_carrier {
            return Err(FiberError::invalid());
        }
        if matches!(config.scheduling, GreenScheduling::WorkStealing) {
            return Err(FiberError::unsupported());
        }
        if !config.classes.is_empty() {
            return Err(FiberError::unsupported());
        }
        if config.guard_pages != 0 {
            return Err(FiberError::unsupported());
        }

        let alignment = planning.min_stack_alignment.max(1);
        let (slot_stride, _) = FiberStackSlab::build_backing(
            effective_backing,
            0,
            1,
            alignment,
            planning.stack_direction,
        )?;
        let stacks = apply_fiber_backing_request(
            FiberPoolBackingRequest {
                bytes: slot_stride
                    .checked_mul(config.max_fibers_per_carrier)
                    .ok_or_else(FiberError::resource_exhausted)?,
                align: alignment,
            },
            config.sizing,
        )?;
        let stack_metadata = apply_fiber_backing_request(
            FiberPoolBackingRequest {
                bytes: FiberStackSlab::metadata_bytes(config.max_fibers_per_carrier, false, 1)?,
                align: align_of::<FiberStackSlabHeader>(),
            },
            config.sizing,
        )?;
        let runtime_metadata = apply_fiber_backing_request(
            FiberPoolBackingRequest {
                bytes: GreenPoolMetadata::metadata_bytes(
                    1,
                    config.max_fibers_per_carrier,
                    config.scheduling,
                    false,
                    green_pool_metadata_alignment(),
                )?,
                align: green_pool_metadata_alignment(),
            },
            config.sizing,
        )?;
        let control = apply_fiber_backing_request(
            FiberPoolBackingRequest {
                bytes: size_of::<GreenPoolControlBlock>(),
                align: align_of::<GreenPoolControlBlock>(),
            },
            config.sizing,
        )?;
        Ok(CurrentFiberPoolBackingPlan {
            control,
            runtime_metadata,
            stack_metadata,
            stacks,
        })
    }

    /// Creates one manually-driven current-thread fiber pool with one carrier.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the backend cannot support same-thread fiber switching, or
    /// when the configured stack backing cannot be realized.
    pub fn new(config: &FiberPoolConfig<'_>) -> Result<Self, FiberError> {
        if let Some(backing) = current_fiber_pool_owned_backing(config)? {
            return Self::from_backing(config, backing);
        }
        let support = FiberSystem::new().support();
        if !support.context.caps.contains(ContextCaps::MAKE)
            || !support.context.caps.contains(ContextCaps::SWAP)
        {
            return Err(FiberError::unsupported());
        }
        if support.context.guard_required && config.guard_pages == 0 {
            return Err(FiberError::invalid());
        }
        let task_capacity_per_carrier = config.task_capacity_per_carrier()?;
        if config.growth_chunk == 0 || task_capacity_per_carrier == 0 {
            return Err(FiberError::invalid());
        }
        if !config.uses_classes() && config.growth_chunk > config.max_fibers_per_carrier {
            return Err(FiberError::invalid());
        }
        if matches!(config.scheduling, GreenScheduling::WorkStealing) {
            return Err(FiberError::unsupported());
        }
        let alignment = support.context.min_stack_alignment.max(16);
        let stacks = FiberStackStore::new(config, alignment, support.context.stack_direction)?;
        let task_capacity = stacks.total_capacity();
        let (runtime_region, metadata_region) =
            green_pool_runtime_regions(1, task_capacity, config.scheduling, false, config.sizing)?;
        let (pool_metadata, tasks, carriers) = match GreenPoolMetadata::new_in_region(
            metadata_region,
            1,
            task_capacity,
            config.scheduling,
            config.priority_age_cap,
            false,
            true,
        ) {
            Ok(parts) => parts,
            Err(error) => {
                let _ = unsafe { system_mem().unmap(runtime_region) };
                return Err(error);
            }
        };

        let inner = GreenPoolLease::new(
            runtime_region,
            GreenPoolInner {
                support,
                courier_id: config.courier_id,
                context_id: config.context_id,
                runtime_sink: config.runtime_sink,
                launch_control: config.launch_control,
                launch_request: config.launch_request,
                scheduling: config.scheduling,
                #[cfg(feature = "std")]
                spawn_locality_policy: config.spawn_locality_policy,
                capacity_policy: config.capacity_policy,
                yield_budget_supported: yield_budget_enforcement_supported(),
                #[cfg(feature = "std")]
                yield_budget_policy: config.yield_budget_policy,
                shutdown: AtomicBool::new(false),
                client_refs: AtomicUsize::new(1),
                active: AtomicUsize::new(0),
                root_registered: AtomicBool::new(false),
                launch_registered: AtomicBool::new(false),
                next_id: AtomicUsize::new(1),
                next_carrier: AtomicUsize::new(0),
                runtime_dispatch_cookie: AtomicUsize::new(0),
                carriers,
                tasks,
                stacks,
                #[cfg(feature = "std")]
                yield_budget_runtime: GreenYieldBudgetRuntime::new(1),
            },
            pool_metadata,
        )?;
        inner.tasks.initialize_owner(inner.as_ptr());
        Ok(Self {
            inner,
            _not_send_sync: PhantomData,
        })
    }

    /// Creates one manually-driven current-thread fiber pool from explicit backing resources.
    ///
    /// This path is the bare-metal honest constructor: caller-owned backing comes in from the
    /// board/application side, and the runtime consumes it without asking the platform for a
    /// surprise mapping. The current implementation covers the legacy single-slab stack shape.
    ///
    /// # Errors
    ///
    /// Returns any honest configuration, resource-shape, or bootstrap failure.
    pub fn from_backing(
        config: &FiberPoolConfig<'_>,
        backing: CurrentFiberPoolBacking,
    ) -> Result<Self, FiberError> {
        let support = FiberSystem::new().support();
        if !support.context.caps.contains(ContextCaps::MAKE)
            || !support.context.caps.contains(ContextCaps::SWAP)
        {
            return Err(FiberError::unsupported());
        }
        if support.context.guard_required && config.guard_pages == 0 {
            return Err(FiberError::invalid());
        }
        if matches!(config.scheduling, GreenScheduling::WorkStealing) {
            return Err(FiberError::unsupported());
        }
        if !config.classes.is_empty() || config.guard_pages != 0 {
            return Err(FiberError::unsupported());
        }

        let alignment = support.context.min_stack_alignment.max(16);
        let stacks = FiberStackStore::Legacy(FiberStackSlab::from_backing(
            config,
            alignment,
            support.context.stack_direction,
            backing.stacks,
            backing.stack_metadata,
        )?);
        let task_capacity = stacks.total_capacity();
        let metadata_region = unsafe { backing.runtime_metadata.view().raw_region() };
        let (pool_metadata, tasks, carriers) = GreenPoolMetadata::new_in_region(
            metadata_region,
            1,
            task_capacity,
            config.scheduling,
            config.priority_age_cap,
            false,
            true,
        )?;

        let inner = GreenPoolLease::new_with_backing(
            backing.control,
            backing.runtime_metadata,
            backing.slab_owner,
            GreenPoolInner {
                support,
                courier_id: config.courier_id,
                context_id: config.context_id,
                runtime_sink: config.runtime_sink,
                launch_control: config.launch_control,
                launch_request: config.launch_request,
                scheduling: config.scheduling,
                #[cfg(feature = "std")]
                spawn_locality_policy: config.spawn_locality_policy,
                capacity_policy: config.capacity_policy,
                yield_budget_supported: yield_budget_enforcement_supported(),
                #[cfg(feature = "std")]
                yield_budget_policy: config.yield_budget_policy,
                shutdown: AtomicBool::new(false),
                client_refs: AtomicUsize::new(1),
                active: AtomicUsize::new(0),
                root_registered: AtomicBool::new(false),
                launch_registered: AtomicBool::new(false),
                next_id: AtomicUsize::new(1),
                next_carrier: AtomicUsize::new(0),
                runtime_dispatch_cookie: AtomicUsize::new(0),
                carriers,
                tasks,
                stacks,
                #[cfg(feature = "std")]
                yield_budget_runtime: GreenYieldBudgetRuntime::new(1),
            },
            pool_metadata,
        )?;
        inner.tasks.initialize_owner(inner.as_ptr());
        Ok(Self {
            inner,
            _not_send_sync: PhantomData,
        })
    }

    /// Creates one current-thread fiber pool from one caller-owned bound slab.
    ///
    /// This is the deterministic owning-slab bootstrap path for bare metal and other explicit
    /// backing targets.
    ///
    /// # Errors
    ///
    /// Returns any honest sizing, partitioning, or bootstrap failure.
    pub fn from_bound_slab(
        config: &FiberPoolConfig<'_>,
        slab: MemoryResourceHandle,
    ) -> Result<Self, FiberError> {
        let layout = Self::backing_plan(config)?
            .combined_for_base_alignment(fiber_resource_base_alignment(&slab))?;
        if slab.view().len() < layout.slab.bytes {
            return Err(FiberError::resource_exhausted());
        }
        let backing = CurrentFiberPoolBacking {
            control: partition_bound_resource(&slab, layout.control)?,
            runtime_metadata: partition_bound_resource(&slab, layout.runtime_metadata)?,
            stack_metadata: partition_bound_resource(&slab, layout.stack_metadata)?,
            stacks: partition_bound_resource(&slab, layout.stacks)?,
            slab_owner: None,
        };
        Self::from_backing(config, backing)
    }

    /// Creates one current-thread fiber pool from one caller-owned static byte slab.
    ///
    /// This is the ergonomic deterministic board-facing path above `from_bound_slab(...)` for
    /// SRAM-backed static runtime storage.
    ///
    /// # Safety
    ///
    /// The caller must guarantee the supplied pointer/length pair names one valid writable static
    /// memory extent for the whole lifetime of the pool.
    ///
    /// # Errors
    ///
    /// Returns any honest binding, sizing, partitioning, or bootstrap failure.
    pub unsafe fn from_static_slab(
        config: &FiberPoolConfig<'_>,
        ptr: *mut u8,
        len: usize,
    ) -> Result<Self, FiberError> {
        let slab = MemoryResourceHandle::from(
            unsafe { BoundMemoryResource::static_allocatable_bytes(ptr, len) }
                .map_err(fiber_error_from_resource)?,
        );
        Self::from_bound_slab(config, slab)
    }

    /// Attempts to clone one current-thread pool handle.
    ///
    /// # Errors
    ///
    /// Returns an error when the shared pool root cannot be retained honestly.
    pub fn try_clone(&self) -> Result<Self, FiberError> {
        let inner = self.inner.try_clone()?;
        inner.client_refs.fetch_add(1, Ordering::AcqRel);
        Ok(Self {
            inner,
            _not_send_sync: PhantomData,
        })
    }

    /// Returns the number of active fibers currently admitted.
    #[must_use]
    pub fn active_count(&self) -> usize {
        self.inner.active.load(Ordering::Acquire)
    }

    /// Returns the owning courier identity for this current-thread fiber lane, when configured.
    #[must_use]
    pub fn courier_id(&self) -> Option<CourierId> {
        self.inner.courier_id
    }

    /// Returns the owning context identity for this current-thread fiber lane, when configured.
    #[must_use]
    pub fn context_id(&self) -> Option<ContextId> {
        self.inner.context_id
    }

    /// Returns a courier-facing run summary for this current-thread fiber lane.
    ///
    /// This is the current bridge between the existing fiber scheduler and the courier-local
    /// supervision model. The pool still owns scheduling internally, but it can now report
    /// runnable/running/blocked truth in courier vocabulary.
    ///
    /// # Errors
    ///
    /// Returns an error if the task registry cannot be observed honestly.
    pub fn runtime_summary(&self) -> Result<CourierRuntimeSummary, FiberError> {
        self.runtime_summary_with_responsiveness(CourierResponsiveness::Responsive)
    }

    /// Returns a courier-facing run summary for this current-thread fiber lane using one
    /// caller-supplied responsiveness classification.
    ///
    /// # Errors
    ///
    /// Returns an error if the task registry cannot be observed honestly.
    pub fn runtime_summary_with_responsiveness(
        &self,
        responsiveness: CourierResponsiveness,
    ) -> Result<CourierRuntimeSummary, FiberError> {
        let (active_units, runnable_units, running_units, blocked_units) =
            self.inner.tasks.lane_counts()?;
        let available_slots = self.inner.tasks.available_slots()?;
        Ok(CourierRuntimeSummary {
            policy: match self.inner.scheduling {
                GreenScheduling::Fifo => CourierSchedulingPolicy::CooperativeRoundRobin,
                GreenScheduling::Priority => CourierSchedulingPolicy::CooperativePriority,
                GreenScheduling::WorkStealing => CourierSchedulingPolicy::CooperativeWorkStealing,
            },
            run_state: if running_units != 0 {
                CourierRunState::Running
            } else if runnable_units != 0 {
                CourierRunState::Runnable
            } else {
                CourierRunState::Idle
            },
            responsiveness,
            fiber_lane: Some(CourierLaneSummary {
                kind: RunnableUnitKind::Fiber,
                active_units,
                runnable_units,
                running_units,
                blocked_units,
                available_slots,
            }),
            async_lane: None,
            control_lane: None,
        }
        .with_responsiveness(responsiveness))
    }

    /// Returns the number of unreserved task slots currently available in this pool.
    ///
    /// This is admission truth, not a guess derived from active-count folklore.
    ///
    /// # Errors
    ///
    /// Returns an error when the task registry cannot be observed honestly.
    pub fn available_slots(&self) -> Result<usize, FiberError> {
        self.inner.tasks.available_slots()
    }

    /// Returns whether this pool can honestly admit the requested task class.
    #[must_use]
    pub fn supports_task_class(&self, class: FiberStackClass) -> bool {
        self.inner.stacks.supports_task_class(class)
    }

    /// Validates one explicit task-attribute bundle against this live pool.
    ///
    /// # Errors
    ///
    /// Returns an error when the requested task class is not provisioned by the current pool.
    pub fn validate_task_attributes(&self, task: FiberTaskAttributes) -> Result<(), FiberError> {
        if task.yield_budget.is_some() && !self.inner.yield_budget_supported {
            return Err(FiberError::unsupported());
        }
        if !task.execution.requires_fiber() {
            return task
                .yield_budget
                .is_none()
                .then_some(())
                .ok_or_else(FiberError::unsupported);
        }
        self.supports_task_class(task.stack_class)
            .then_some(())
            .ok_or_else(FiberError::unsupported)
    }

    /// Validates one explicit fiber task against this live pool.
    ///
    /// # Errors
    ///
    /// Returns an error when the task's declared contract is not provisioned by this pool.
    pub fn validate_explicit_task<T: ExplicitFiberTask>(&self) -> Result<(), FiberError> {
        self.validate_task_attributes(T::task_attributes()?)
    }

    /// Validates one build-generated explicit task against this live pool.
    ///
    /// # Errors
    ///
    /// Returns an error when generated metadata is missing or the resolved class is unsupported.
    #[cfg(not(feature = "critical-safe"))]
    pub fn validate_generated_task<T: GeneratedExplicitFiberTask>(&self) -> Result<(), FiberError> {
        self.validate_task_attributes(T::task_attributes()?)
    }

    /// Validates one build-generated explicit task against this live pool through its compile-time
    /// generated contract.
    ///
    /// # Errors
    ///
    /// Returns an error when the generated contract is not provisioned by the current pool.
    pub fn validate_generated_task_contract<T>(&self) -> Result<(), FiberError>
    where
        T: GeneratedExplicitFiberTask + GeneratedExplicitFiberTaskContract,
    {
        self.validate_task_attributes(
            generated_explicit_task_contract_attributes::<T>()
                .with_optional_yield_budget(T::YIELD_BUDGET),
        )
    }

    /// Validates one build-generated explicit task against this live pool.
    ///
    /// # Errors
    ///
    /// Returns an error when the generated contract is not provisioned by the current pool.
    #[cfg(feature = "critical-safe")]
    pub fn validate_generated_task<T>(&self) -> Result<(), FiberError>
    where
        T: GeneratedExplicitFiberTask + GeneratedExplicitFiberTaskContract,
    {
        self.validate_task_attributes(
            generated_explicit_task_contract_attributes::<T>()
                .with_optional_yield_budget(T::YIELD_BUDGET),
        )
    }

    /// Spawns one current-thread fiber job using build-generated metadata when available.
    ///
    /// # Errors
    ///
    /// Returns an error when the submitted closure cannot be admitted honestly.
    pub fn spawn<F, T>(&self, job: F) -> Result<CurrentFiberHandle<T>, FiberError>
    where
        F: FnOnce() -> T + Send + 'static,
        T: 'static,
    {
        let task = closure_spawn_task_attributes::<F>(self.inner.stacks.default_task_class()?)?;
        self.spawn_with_attrs(task, job)
    }

    /// Spawns one current-thread fiber job with an explicit stack-byte contract.
    ///
    /// # Errors
    ///
    /// Returns an error when the declared stack bytes cannot be mapped to a supported class.
    pub fn spawn_with_stack<const STACK_BYTES: usize, F, T>(
        &self,
        job: F,
    ) -> Result<CurrentFiberHandle<T>, FiberError>
    where
        F: FnOnce() -> T + Send + 'static,
        T: 'static,
    {
        self.spawn_with_attrs(task_attributes_from_stack_bytes::<STACK_BYTES>()?, job)
    }

    /// Spawns one current-thread fiber using explicit task attributes.
    ///
    /// # Errors
    ///
    /// Returns an error when the task cannot be admitted honestly.
    pub fn spawn_with_attrs<F, T>(
        &self,
        task: FiberTaskAttributes,
        job: F,
    ) -> Result<CurrentFiberHandle<T>, FiberError>
    where
        F: FnOnce() -> T + Send + 'static,
        T: 'static,
    {
        self.spawn_with_attrs_class(task, job, fusion_sys::courier::CourierFiberClass::Dynamic)
    }

    fn spawn_with_attrs_class<F, T>(
        &self,
        task: FiberTaskAttributes,
        job: F,
        class: fusion_sys::courier::CourierFiberClass,
    ) -> Result<CurrentFiberHandle<T>, FiberError>
    where
        F: FnOnce() -> T + Send + 'static,
        T: 'static,
    {
        let handle = spawn_on_lease(
            &self.inner,
            task,
            job,
            class,
            false,
            GreenHandleDriveMode::CurrentThread,
            true,
        )?;
        Ok(CurrentFiberHandle {
            inner: handle,
            _not_send_sync: PhantomData,
        })
    }

    fn spawn_named_task_with_attrs_class<F, T>(
        &self,
        task: FiberTaskAttributes,
        job: F,
        class: fusion_sys::courier::CourierFiberClass,
    ) -> Result<CurrentFiberHandle<T>, FiberError>
    where
        F: FnOnce() -> T + Send + 'static,
        T: 'static,
    {
        let handle = spawn_on_lease(
            &self.inner,
            task,
            job,
            class,
            false,
            GreenHandleDriveMode::CurrentThread,
            false,
        )?;
        Ok(CurrentFiberHandle {
            inner: handle,
            _not_send_sync: PhantomData,
        })
    }

    /// Spawns one explicit fiber task carrying compile-time stack metadata.
    ///
    /// # Errors
    ///
    /// Returns an error when the task contract cannot be mapped or admitted honestly.
    pub fn spawn_planned<T>(&self, task: T) -> Result<CurrentFiberHandle<T::Output>, FiberError>
    where
        T: ExplicitFiberTask,
    {
        self.spawn_explicit(task)
    }

    /// Spawns one explicit fiber task carrying compile-time stack metadata.
    ///
    /// # Errors
    ///
    /// Returns an error when the task contract cannot be mapped or admitted honestly.
    pub fn spawn_explicit<T>(&self, task: T) -> Result<CurrentFiberHandle<T::Output>, FiberError>
    where
        T: ExplicitFiberTask,
    {
        let attributes = T::task_attributes()?;
        self.validate_task_attributes(attributes)?;
        self.spawn_named_task_with_attrs_class(
            attributes,
            move || task.run(),
            fusion_sys::courier::CourierFiberClass::Planned,
        )
    }

    /// Spawns one explicit fiber task using build-generated stack metadata.
    ///
    /// # Errors
    ///
    /// Returns an error when generated metadata is missing or the task cannot be admitted.
    #[cfg(not(feature = "critical-safe"))]
    pub fn spawn_generated<T>(&self, task: T) -> Result<CurrentFiberHandle<T::Output>, FiberError>
    where
        T: GeneratedExplicitFiberTask,
    {
        let attributes = T::task_attributes()?;
        self.validate_task_attributes(attributes)?;
        self.spawn_named_task_with_attrs_class(
            attributes,
            move || task.run(),
            fusion_sys::courier::CourierFiberClass::Planned,
        )
    }

    /// Spawns one explicit fiber task using a compile-time generated contract.
    ///
    /// # Errors
    ///
    /// Returns an error when the generated contract is not provisioned by the current pool.
    #[cfg(feature = "critical-safe")]
    pub fn spawn_generated<T>(&self, task: T) -> Result<CurrentFiberHandle<T::Output>, FiberError>
    where
        T: GeneratedExplicitFiberTask + GeneratedExplicitFiberTaskContract,
    {
        let attributes = generated_explicit_task_contract_attributes::<T>()
            .with_optional_yield_budget(T::YIELD_BUDGET);
        self.validate_task_attributes(attributes)?;
        self.spawn_named_task_with_attrs_class(
            attributes,
            move || task.run(),
            fusion_sys::courier::CourierFiberClass::Planned,
        )
    }

    /// Spawns one explicit fiber task using a compile-time generated contract directly.
    ///
    /// # Errors
    ///
    /// Returns an error when the generated contract is not provisioned by the current pool.
    pub fn spawn_generated_contract<T>(
        &self,
        task: T,
    ) -> Result<CurrentFiberHandle<T::Output>, FiberError>
    where
        T: GeneratedExplicitFiberTask + GeneratedExplicitFiberTaskContract,
    {
        let attributes = generated_explicit_task_contract_attributes::<T>()
            .with_optional_yield_budget(T::YIELD_BUDGET);
        self.validate_task_attributes(attributes)?;
        self.spawn_named_task_with_attrs_class(
            attributes,
            move || task.run(),
            fusion_sys::courier::CourierFiberClass::Planned,
        )
    }

    /// Pumps at most one ready task segment on the current thread.
    ///
    /// # Errors
    ///
    /// Returns an error when the current pool cannot resume the next ready fiber honestly.
    pub(crate) fn pump_once(&self) -> Result<bool, FiberError> {
        drive_current_pool_once(&self.inner)
    }

    /// Drains ready work until the current-thread pool reaches an idle state.
    ///
    /// # Errors
    ///
    /// Returns an error when one resumed task fails dishonestly.
    #[cfg(test)]
    pub(crate) fn drain_until_idle(&self) -> Result<usize, FiberError> {
        let mut steps = 0usize;
        while self.pump_once()? {
            steps = steps.saturating_add(1);
        }
        Ok(steps)
    }

    /// Returns an approximate stack-telemetry snapshot for this current-thread pool.
    #[must_use]
    pub fn stack_stats(&self) -> Option<FiberStackStats> {
        self.inner.stacks.stack_stats()
    }

    /// Returns the exact live memory footprint of this current-thread pool.
    #[must_use]
    pub fn memory_footprint(&self) -> FiberPoolMemoryFootprint {
        self.inner.memory_footprint()
    }

    /// Requests shutdown of the current-thread pool.
    ///
    /// # Errors
    ///
    /// Returns an error if the wakeup path cannot be signaled honestly.
    pub fn shutdown(&self) -> Result<(), FiberError> {
        self.inner.request_shutdown()
    }
}

fn current_fiber_pool_owned_backing(
    config: &FiberPoolConfig<'_>,
) -> Result<Option<CurrentFiberPoolBacking>, FiberError> {
    if !uses_explicit_bound_runtime_backing() {
        return Ok(None);
    }
    let layout = CurrentFiberPool::backing_plan(config)?.combined()?;
    let Some(slab) = allocate_owned_runtime_slab(layout.slab.bytes, layout.slab.align)
        .map_err(fiber_error_from_current_runtime_backing)?
    else {
        return Ok(None);
    };
    let backing = CurrentFiberPoolBacking {
        control: partition_bound_resource(&slab.handle, layout.control)?,
        runtime_metadata: partition_bound_resource(&slab.handle, layout.runtime_metadata)?,
        stack_metadata: partition_bound_resource(&slab.handle, layout.stack_metadata)?,
        stacks: partition_bound_resource(&slab.handle, layout.stacks)?,
        slab_owner: Some(slab.lease),
    };
    Ok(Some(backing))
}

fn fiber_error_from_current_runtime_backing(error: RuntimeBackingError) -> FiberError {
    match error.kind() {
        RuntimeBackingErrorKind::Unsupported => FiberError::unsupported(),
        RuntimeBackingErrorKind::Invalid => FiberError::invalid(),
        RuntimeBackingErrorKind::ResourceExhausted => FiberError::resource_exhausted(),
        RuntimeBackingErrorKind::StateConflict => FiberError::state_conflict(),
    }
}

fn fiber_error_from_runtime_sink(
    error: fusion_sys::courier::CourierRuntimeSinkError,
) -> FiberError {
    match error {
        fusion_sys::courier::CourierRuntimeSinkError::Unsupported => FiberError::unsupported(),
        fusion_sys::courier::CourierRuntimeSinkError::Invalid => FiberError::invalid(),
        fusion_sys::courier::CourierRuntimeSinkError::NotFound
        | fusion_sys::courier::CourierRuntimeSinkError::StateConflict
        | fusion_sys::courier::CourierRuntimeSinkError::Busy => FiberError::state_conflict(),
        fusion_sys::courier::CourierRuntimeSinkError::ResourceExhausted => {
            FiberError::resource_exhausted()
        }
    }
}

fn fiber_error_from_launch_control(
    error: fusion_sys::courier::CourierLaunchControlError,
) -> FiberError {
    match error {
        fusion_sys::courier::CourierLaunchControlError::Unsupported => FiberError::unsupported(),
        fusion_sys::courier::CourierLaunchControlError::Invalid => FiberError::invalid(),
        fusion_sys::courier::CourierLaunchControlError::NotFound
        | fusion_sys::courier::CourierLaunchControlError::StateConflict
        | fusion_sys::courier::CourierLaunchControlError::Busy => FiberError::state_conflict(),
        fusion_sys::courier::CourierLaunchControlError::ResourceExhausted => {
            FiberError::resource_exhausted()
        }
    }
}

impl Drop for CurrentFiberPool {
    fn drop(&mut self) {
        if self.inner.client_refs.fetch_sub(1, Ordering::AcqRel) == 1 {
            let _ = self.inner.request_shutdown();
        }
    }
}

include!("hosted.rs");
