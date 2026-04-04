use super::*;

/// Exact configured memory footprint for one combined current-thread fiber + async bundle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CurrentFiberAsyncRuntimeMemoryFootprint {
    /// Fiber-pool footprint.
    pub fibers: FiberPoolMemoryFootprint,
    /// Async-runtime footprint.
    pub executor: AsyncRuntimeMemoryFootprint,
}

impl CurrentFiberAsyncRuntimeMemoryFootprint {
    /// Returns the total configured bytes reserved by the combined runtime.
    #[must_use]
    pub const fn total_bytes(self) -> usize {
        self.fibers.total_bytes() + self.executor.total_bytes()
    }
}

/// One deferred current-thread async runtime builder.
#[derive(Debug)]
pub struct CurrentAsyncRuntimeBuilder {
    config: ExecutorConfig,
    slab: Option<MemoryResourceHandle>,
    owned_backing: Option<ExtentLease>,
}

/// Split current-thread fiber + async bootstrap result.
#[derive(Debug)]
pub struct CurrentFiberAsyncParts {
    fibers: CurrentFiberPool,
    executor: CurrentAsyncRuntimeBuilder,
}

impl CurrentFiberAsyncRuntime {
    /// Returns the current-thread fiber pool.
    #[must_use]
    pub const fn fibers(&self) -> &CurrentFiberPool {
        &self.fibers
    }

    /// Returns the current-thread async runtime.
    #[must_use]
    pub const fn executor(&self) -> &CurrentAsyncRuntime {
        &self.executor
    }

    /// Consumes the bundle into its component runtimes.
    #[must_use]
    pub fn into_parts(self) -> (CurrentFiberPool, CurrentAsyncRuntime) {
        (self.fibers, self.executor)
    }

    /// Returns the exact configured memory footprint for this combined runtime bundle.
    ///
    /// This is the selected-target planning view of the bundle's owned fiber and async domains.
    /// It intentionally describes configured backing shape, not transient live queue occupancy.
    ///
    /// # Errors
    ///
    /// Returns any honest async sizing failure while materializing the configured executor view.
    pub fn configured_memory_footprint(
        &self,
    ) -> Result<CurrentFiberAsyncRuntimeMemoryFootprint, CurrentFiberAsyncRuntimeError> {
        Ok(CurrentFiberAsyncRuntimeMemoryFootprint {
            fibers: self.fibers.memory_footprint(),
            executor: self.executor.configured_memory_footprint()?,
        })
    }

    /// Returns a courier-facing run summary for this combined current-thread runtime bundle.
    ///
    /// # Errors
    ///
    /// Returns any honest fiber or async observation failure.
    pub fn runtime_summary(&self) -> Result<CourierRuntimeSummary, CurrentFiberAsyncRuntimeError> {
        let fiber_summary = self.fibers.runtime_summary()?;
        let async_summary = self.executor.runtime_summary()?;
        let responsiveness = match (fiber_summary.responsiveness, async_summary.responsiveness) {
            (CourierResponsiveness::NonResponsive, _)
            | (_, CourierResponsiveness::NonResponsive) => CourierResponsiveness::NonResponsive,
            (CourierResponsiveness::Stale, _) | (_, CourierResponsiveness::Stale) => {
                CourierResponsiveness::Stale
            }
            (CourierResponsiveness::Responsive, CourierResponsiveness::Responsive) => {
                CourierResponsiveness::Responsive
            }
        };
        Ok(CourierRuntimeSummary {
            policy: fiber_summary.policy,
            run_state: if fiber_summary.run_state == CourierRunState::Running
                || async_summary.run_state == CourierRunState::Running
            {
                CourierRunState::Running
            } else if fiber_summary.run_state == CourierRunState::Runnable
                || async_summary.run_state == CourierRunState::Runnable
            {
                CourierRunState::Runnable
            } else {
                CourierRunState::Idle
            },
            responsiveness,
            fiber_lane: fiber_summary.fiber_lane,
            async_lane: async_summary.async_lane,
            control_lane: None,
        }
        .with_responsiveness(responsiveness))
    }
}

impl CurrentAsyncRuntimeBuilder {
    /// Builds the current-thread async runtime.
    ///
    /// # Errors
    ///
    /// Returns any honest current-thread async bootstrap failure.
    pub fn build(self) -> Result<CurrentAsyncRuntime, CurrentFiberAsyncRuntimeError> {
        match (self.slab, self.owned_backing) {
            (Some(slab), _) => Ok(CurrentAsyncRuntime::from_bound_slab(self.config, slab)?),
            (None, Some(owned_backing)) => Ok(CurrentAsyncRuntime::from_owned_extent(
                self.config,
                owned_backing,
            )?),
            (None, None) => Ok(CurrentAsyncRuntime::with_executor_config(self.config)),
        }
    }

    /// Builds the current-thread async runtime from one explicit owning slab.
    ///
    /// This is the bare-metal honest path for callers that already know they are on the
    /// explicit-backed lane and do not want the platform-acquired fallback path retained in the
    /// resulting image.
    ///
    /// # Errors
    ///
    /// Returns `Unsupported` if this builder does not carry one explicit backing slab, or any
    /// honest explicit-backed current-thread async bootstrap failure.
    pub fn build_explicit(self) -> Result<CurrentAsyncRuntime, CurrentFiberAsyncRuntimeError> {
        match (self.slab, self.owned_backing) {
            (Some(slab), _) => Ok(CurrentAsyncRuntime::from_bound_slab(self.config, slab)?),
            (None, Some(owned_backing)) => Ok(CurrentAsyncRuntime::from_owned_extent(
                self.config,
                owned_backing,
            )?),
            (None, None) => Err(CurrentFiberAsyncRuntimeError::Executor(
                super::ExecutorError::Unsupported,
            )),
        }
    }
}

impl CurrentFiberAsyncParts {
    /// Returns the current-thread fiber pool.
    #[must_use]
    pub const fn fibers(&self) -> &CurrentFiberPool {
        &self.fibers
    }

    /// Returns the deferred async runtime builder.
    #[must_use]
    pub const fn executor(&self) -> &CurrentAsyncRuntimeBuilder {
        &self.executor
    }

    /// Consumes the split bootstrap result into its component parts.
    #[must_use]
    pub fn into_parts(self) -> (CurrentFiberPool, CurrentAsyncRuntimeBuilder) {
        (self.fibers, self.executor)
    }
}

impl From<fusion_sys::fiber::FiberError> for CurrentFiberAsyncRuntimeError {
    fn from(value: fusion_sys::fiber::FiberError) -> Self {
        Self::Fiber(value)
    }
}

impl From<super::ExecutorError> for CurrentFiberAsyncRuntimeError {
    fn from(value: super::ExecutorError) -> Self {
        Self::Executor(value)
    }
}

pub(super) fn runtime_tick() -> u64 {
    match system_monotonic_time().raw_now() {
        Ok(MonotonicRawInstant::Bits32(raw)) => u64::from(raw),
        Ok(MonotonicRawInstant::Bits64(raw)) => raw,
        Err(_) => 0,
    }
}

fn runtime_align_up_packed(
    offset: usize,
    align: usize,
) -> Result<usize, CurrentFiberAsyncRuntimeError> {
    if align == 0 || !align.is_power_of_two() {
        return Err(CurrentFiberAsyncRuntimeError::Fiber(
            fusion_sys::fiber::FiberError::invalid(),
        ));
    }
    let mask = align - 1;
    offset
        .checked_add(mask)
        .map(|value| value & !mask)
        .ok_or(CurrentFiberAsyncRuntimeError::Fiber(
            fusion_sys::fiber::FiberError::resource_exhausted(),
        ))
}

const fn runtime_base_alignment_from_addr(addr: usize) -> usize {
    if addr == 0 {
        1
    } else {
        1usize << addr.trailing_zeros()
    }
}

fn runtime_resource_base_alignment(handle: &MemoryResourceHandle) -> usize {
    runtime_base_alignment_from_addr(handle.view().base_addr().get())
}

const fn runtime_partition_backing_kind(
    kind: ResourceBackingKind,
) -> Result<ResourceBackingKind, CurrentFiberAsyncRuntimeError> {
    match kind {
        ResourceBackingKind::Borrowed
        | ResourceBackingKind::StaticRegion
        | ResourceBackingKind::Partition => Ok(ResourceBackingKind::Partition),
        _ => Err(CurrentFiberAsyncRuntimeError::Executor(
            super::ExecutorError::Unsupported,
        )),
    }
}

fn partition_runtime_bound_resource(
    handle: &MemoryResourceHandle,
    range: ResourceRange,
) -> Result<MemoryResourceHandle, CurrentFiberAsyncRuntimeError> {
    let info = match handle {
        MemoryResourceHandle::Bound(resource) => resource.info(),
        MemoryResourceHandle::Virtual(_) => {
            return Err(CurrentFiberAsyncRuntimeError::Executor(
                super::ExecutorError::Unsupported,
            ));
        }
    };
    let region = handle
        .subview(range)
        .map(|view| unsafe { view.raw_region() })
        .map_err(|_| CurrentFiberAsyncRuntimeError::Executor(super::ExecutorError::Unsupported))?;
    let resource = BoundMemoryResource::new(BoundResourceSpec::new(
        region,
        info.domain,
        runtime_partition_backing_kind(info.backing)?,
        info.attrs,
        info.geometry,
        info.layout,
        info.contract,
        info.support,
        handle.state(),
    ))
    .map_err(|_| CurrentFiberAsyncRuntimeError::Executor(super::ExecutorError::Unsupported))?;
    Ok(MemoryResourceHandle::from(resource))
}

impl CurrentFiberAsyncBootstrap<'static> {
    /// Returns one deterministic bootstrap using the largest generated fiber stack contract.
    ///
    /// # Errors
    ///
    /// Returns an error when generated fiber stack metadata is unavailable.
    pub fn auto(
        max_fibers: usize,
        async_capacity: usize,
    ) -> Result<Self, CurrentFiberAsyncRuntimeError> {
        let fibers = FiberPoolBootstrap::auto(max_fibers)?;
        Ok(Self::from_parts(
            fibers,
            ExecutorConfig::new().with_capacity(async_capacity),
        ))
    }

    /// Returns one deterministic bootstrap with one explicit uniform fiber stack size.
    #[must_use]
    pub const fn uniform(
        max_fibers: usize,
        stack_size: NonZeroUsize,
        async_capacity: usize,
    ) -> Self {
        Self::from_parts(
            FiberPoolBootstrap::uniform(max_fibers, stack_size),
            ExecutorConfig::new().with_capacity(async_capacity),
        )
    }
}

impl<'a> CurrentFiberAsyncBootstrap<'a> {
    /// Returns one bootstrap from already-built fiber and executor configurations.
    #[must_use]
    pub const fn from_parts(fibers: FiberPoolBootstrap<'a>, executor: ExecutorConfig) -> Self {
        Self { fibers, executor }
    }

    /// Returns the fiber bootstrap half.
    #[must_use]
    pub const fn fibers(&self) -> &FiberPoolBootstrap<'a> {
        &self.fibers
    }

    /// Returns the async executor configuration half.
    #[must_use]
    pub const fn executor_config(&self) -> ExecutorConfig {
        self.executor
    }

    /// Applies one sizing strategy to both fiber and async sizing lanes.
    #[must_use]
    pub const fn with_sizing_strategy(mut self, sizing: RuntimeSizingStrategy) -> Self {
        self.fibers =
            FiberPoolBootstrap::from_config(self.fibers.config().with_sizing_strategy(sizing));
        self.executor = self.executor.with_sizing_strategy(sizing);
        self
    }

    /// Applies one sizing strategy only to the fiber half.
    #[must_use]
    pub const fn with_fiber_sizing_strategy(mut self, sizing: RuntimeSizingStrategy) -> Self {
        self.fibers =
            FiberPoolBootstrap::from_config(self.fibers.config().with_sizing_strategy(sizing));
        self
    }

    /// Applies one sizing strategy only to the async half.
    #[must_use]
    pub const fn with_async_sizing_strategy(mut self, sizing: RuntimeSizingStrategy) -> Self {
        self.executor = self.executor.with_sizing_strategy(sizing);
        self
    }

    /// Applies one explicit fiber guard-page count.
    #[must_use]
    pub const fn with_guard_pages(mut self, guard_pages: usize) -> Self {
        self.fibers =
            FiberPoolBootstrap::from_config(self.fibers.config().with_guard_pages(guard_pages));
        self
    }

    /// Applies one explicit owning courier identity to both fiber and async halves.
    #[must_use]
    pub const fn with_courier_id(mut self, courier_id: CourierId) -> Self {
        self.fibers =
            FiberPoolBootstrap::from_config(self.fibers.config().with_courier_id(courier_id));
        self.executor = self.executor.with_courier_id(courier_id);
        self
    }

    /// Applies one explicit owning context identity to both fiber and async halves.
    #[must_use]
    pub const fn with_context_id(mut self, context_id: ContextId) -> Self {
        self.fibers =
            FiberPoolBootstrap::from_config(self.fibers.config().with_context_id(context_id));
        self.executor = self.executor.with_context_id(context_id);
        self
    }

    /// Applies one explicit runtime-to-courier sink to both fiber and async halves.
    #[must_use]
    pub const fn with_runtime_sink(mut self, runtime_sink: CourierRuntimeSink) -> Self {
        self.fibers =
            FiberPoolBootstrap::from_config(self.fibers.config().with_runtime_sink(runtime_sink));
        self.executor = self.executor.with_runtime_sink(runtime_sink);
        self
    }

    /// Applies one explicit child-courier launch-control surface to the fiber half.
    #[must_use]
    pub const fn with_launch_control(
        mut self,
        launch_control: CourierLaunchControl<'static>,
    ) -> Self {
        self.fibers = FiberPoolBootstrap::from_config(
            self.fibers.config().with_launch_control(launch_control),
        );
        self
    }

    /// Applies one explicit child-courier launch request to the fiber half.
    #[must_use]
    pub const fn with_child_launch(
        mut self,
        launch_request: CourierChildLaunchRequest<'static>,
    ) -> Self {
        self.fibers =
            FiberPoolBootstrap::from_config(self.fibers.config().with_child_launch(launch_request));
        self
    }

    /// Returns the one-slab backing plan for this combined current-thread runtime bundle.
    ///
    /// # Errors
    ///
    /// Returns any honest fiber/async sizing or partitioning failure.
    pub fn backing_plan(
        self,
    ) -> Result<CurrentFiberAsyncRuntimeBackingPlan, CurrentFiberAsyncRuntimeError> {
        self.backing_plan_with_allocator_layout_policy(AllocatorLayoutPolicy::hosted_vm(
            fusion_pal::sys::mem::system_mem().page_info().alloc_granule,
        ))
    }

    /// Returns the one-slab backing plan for a caller that can guarantee at least
    /// `base_align` alignment for the owning slab base.
    ///
    /// # Errors
    ///
    /// Returns any honest fiber/async sizing or partitioning failure.
    pub fn backing_plan_for_base_alignment(
        self,
        base_align: usize,
    ) -> Result<CurrentFiberAsyncRuntimeBackingPlan, CurrentFiberAsyncRuntimeError> {
        self.backing_plan_for_base_alignment_with_allocator_layout_policy(
            base_align,
            AllocatorLayoutPolicy::hosted_vm(
                fusion_pal::sys::mem::system_mem().page_info().alloc_granule,
            ),
        )
    }

    /// Returns the one-slab backing plan under one explicit allocator layout policy.
    pub fn backing_plan_with_allocator_layout_policy(
        self,
        layout_policy: AllocatorLayoutPolicy,
    ) -> Result<CurrentFiberAsyncRuntimeBackingPlan, CurrentFiberAsyncRuntimeError> {
        self.backing_plan_for_base_alignment_with_allocator_layout_policy(1, layout_policy)
    }

    /// Returns the one-slab backing plan under one explicit fiber-planning surface and allocator
    /// layout policy.
    pub fn backing_plan_with_fiber_planning_support_and_allocator_layout_policy(
        self,
        fiber_planning: FiberPlanningSupport,
        layout_policy: AllocatorLayoutPolicy,
    ) -> Result<CurrentFiberAsyncRuntimeBackingPlan, CurrentFiberAsyncRuntimeError> {
        self.backing_plan_with_planning_support_and_allocator_layout_policy(
            fiber_planning,
            ExecutorPlanningSupport::selected_target(),
            layout_policy,
        )
    }

    /// Returns the one-slab backing plan under explicit fiber/executor planning surfaces and one
    /// explicit allocator layout policy.
    pub fn backing_plan_with_planning_support_and_allocator_layout_policy(
        self,
        fiber_planning: FiberPlanningSupport,
        executor_planning: ExecutorPlanningSupport,
        layout_policy: AllocatorLayoutPolicy,
    ) -> Result<CurrentFiberAsyncRuntimeBackingPlan, CurrentFiberAsyncRuntimeError> {
        self.backing_plan_for_base_alignment_with_planning_support_and_allocator_layout_policy(
            1,
            fiber_planning,
            executor_planning,
            layout_policy,
        )
    }

    /// Returns the one-slab backing plan for a caller that can guarantee at least `base_align`
    /// alignment under one explicit allocator layout policy.
    pub fn backing_plan_for_base_alignment_with_allocator_layout_policy(
        self,
        base_align: usize,
        layout_policy: AllocatorLayoutPolicy,
    ) -> Result<CurrentFiberAsyncRuntimeBackingPlan, CurrentFiberAsyncRuntimeError> {
        self.backing_plan_for_base_alignment_with_planning_support_and_allocator_layout_policy(
            base_align,
            FiberPlanningSupport::selected_runtime(),
            ExecutorPlanningSupport::selected_target(),
            layout_policy,
        )
    }

    /// Returns the one-slab backing plan for a caller that can guarantee at least `base_align`
    /// alignment under one explicit fiber-planning surface and allocator layout policy.
    pub fn backing_plan_for_base_alignment_with_fiber_planning_support_and_allocator_layout_policy(
        self,
        base_align: usize,
        fiber_planning: FiberPlanningSupport,
        layout_policy: AllocatorLayoutPolicy,
    ) -> Result<CurrentFiberAsyncRuntimeBackingPlan, CurrentFiberAsyncRuntimeError> {
        self.backing_plan_for_base_alignment_with_planning_support_and_allocator_layout_policy(
            base_align,
            fiber_planning,
            ExecutorPlanningSupport::selected_target(),
            layout_policy,
        )
    }

    /// Returns the one-slab backing plan for a caller that can guarantee at least `base_align`
    /// alignment under explicit fiber/executor planning surfaces and one allocator layout policy.
    pub fn backing_plan_for_base_alignment_with_planning_support_and_allocator_layout_policy(
        self,
        base_align: usize,
        fiber_planning: FiberPlanningSupport,
        executor_planning: ExecutorPlanningSupport,
        layout_policy: AllocatorLayoutPolicy,
    ) -> Result<CurrentFiberAsyncRuntimeBackingPlan, CurrentFiberAsyncRuntimeError> {
        let fiber_plan = CurrentFiberPool::backing_plan_with_planning_support(
            self.fibers.config(),
            fiber_planning,
        )?
        .combined_for_base_alignment(base_align)?;
        let executor_plan =
            CurrentAsyncRuntime::backing_plan_with_layout_policy_and_planning_support(
                self.executor,
                layout_policy,
                executor_planning,
            )?
            .combined_eager_for_base_alignment(base_align)?;

        let mut max_align = fiber_plan.slab.align;
        if executor_plan.slab.align > max_align {
            max_align = executor_plan.slab.align;
        }

        let mut cursor = if base_align >= max_align {
            0
        } else {
            max_align.saturating_sub(1)
        };
        let fiber_offset = runtime_align_up_packed(cursor, fiber_plan.slab.align)?;
        cursor = fiber_offset.checked_add(fiber_plan.slab.bytes).ok_or(
            CurrentFiberAsyncRuntimeError::Fiber(
                fusion_sys::fiber::FiberError::resource_exhausted(),
            ),
        )?;
        let executor_offset = runtime_align_up_packed(cursor, executor_plan.slab.align)?;
        let total_bytes = executor_offset
            .checked_add(executor_plan.slab.bytes)
            .ok_or(CurrentFiberAsyncRuntimeError::Fiber(
                fusion_sys::fiber::FiberError::resource_exhausted(),
            ))?;

        Ok(CurrentFiberAsyncRuntimeBackingPlan {
            slab: RuntimeBackingRequest {
                bytes: total_bytes,
                align: max_align,
            },
            fibers: ResourceRange::new(fiber_offset, fiber_plan.slab.bytes),
            executor: ResourceRange::new(executor_offset, executor_plan.slab.bytes),
            fiber_plan,
            executor_plan,
        })
    }

    /// Builds one current-thread fiber + async runtime bundle through platform acquisition.
    ///
    /// # Errors
    ///
    /// Returns any honest current-thread fiber or async bootstrap failure.
    pub fn build_current(self) -> Result<CurrentFiberAsyncRuntime, CurrentFiberAsyncRuntimeError> {
        let parts = self.build_current_parts()?;
        let (fibers, executor) = parts.into_parts();
        Ok(CurrentFiberAsyncRuntime {
            fibers,
            executor: executor.build()?,
        })
    }

    /// Builds one split current-thread fiber + async bootstrap through platform acquisition.
    ///
    /// The async runtime stays deferred so callers can realize it inside one spawned current-thread
    /// fiber without forcing `CurrentAsyncRuntime` itself to become `Send`.
    ///
    /// # Errors
    ///
    /// Returns any honest current-thread fiber bootstrap failure.
    pub fn build_current_parts(
        self,
    ) -> Result<CurrentFiberAsyncParts, CurrentFiberAsyncRuntimeError> {
        if uses_explicit_bound_runtime_backing() {
            let fibers = self.fibers.build_current()?;
            let layout = CurrentAsyncRuntime::backing_plan(self.executor)?;
            let combined = layout.combined_eager()?;
            let slab = allocate_owned_runtime_slab(combined.slab.bytes, combined.slab.align)
                .map_err(current_runtime_error_from_owned_backing)?;
            if let Some(slab) = slab {
                return Ok(CurrentFiberAsyncParts {
                    fibers,
                    executor: CurrentAsyncRuntimeBuilder {
                        config: self.executor,
                        slab: None,
                        owned_backing: Some(slab.lease),
                    },
                });
            }
        }
        let fibers = self.fibers.build_current()?;
        Ok(CurrentFiberAsyncParts {
            fibers,
            executor: CurrentAsyncRuntimeBuilder {
                config: self.executor,
                slab: None,
                owned_backing: None,
            },
        })
    }

    /// Builds one current-thread fiber + async runtime bundle from one caller-owned bound slab.
    ///
    /// # Errors
    ///
    /// Returns any honest sizing, partitioning, or bootstrap failure.
    pub fn from_bound_slab(
        self,
        slab: MemoryResourceHandle,
    ) -> Result<CurrentFiberAsyncRuntime, CurrentFiberAsyncRuntimeError> {
        let parts = self.from_bound_slab_parts(slab)?;
        let (fibers, executor) = parts.into_parts();
        Ok(CurrentFiberAsyncRuntime {
            fibers,
            executor: executor.build()?,
        })
    }

    /// Builds one split current-thread fiber + async bundle from one caller-owned bound slab.
    ///
    /// The async runtime stays deferred so callers can realize it inside a spawned current-thread
    /// fiber while still consuming one combined owning slab.
    ///
    /// # Errors
    ///
    /// Returns any honest sizing, partitioning, or bootstrap failure.
    pub fn from_bound_slab_parts(
        self,
        slab: MemoryResourceHandle,
    ) -> Result<CurrentFiberAsyncParts, CurrentFiberAsyncRuntimeError> {
        let layout = self.backing_plan_for_base_alignment_with_allocator_layout_policy(
            runtime_resource_base_alignment(&slab),
            slab.info().layout,
        )?;
        if slab.view().len() < layout.slab.bytes {
            return Err(CurrentFiberAsyncRuntimeError::Fiber(
                fusion_sys::fiber::FiberError::resource_exhausted(),
            ));
        }
        let fibers = CurrentFiberPool::from_bound_slab(
            self.fibers.config(),
            partition_runtime_bound_resource(&slab, layout.fibers)?,
        )?;
        Ok(CurrentFiberAsyncParts {
            fibers,
            executor: CurrentAsyncRuntimeBuilder {
                config: self.executor,
                slab: Some(partition_runtime_bound_resource(&slab, layout.executor)?),
                owned_backing: None,
            },
        })
    }

    /// Builds one current-thread fiber + async runtime bundle from one caller-owned static slab.
    ///
    /// # Safety
    ///
    /// The caller must guarantee the supplied pointer/length pair names one valid writable static
    /// extent for the whole lifetime of the bundle.
    ///
    /// # Errors
    ///
    /// Returns any honest binding, sizing, partitioning, or bootstrap failure.
    pub unsafe fn from_static_slab(
        self,
        ptr: *mut u8,
        len: usize,
    ) -> Result<CurrentFiberAsyncRuntime, CurrentFiberAsyncRuntimeError> {
        let slab = MemoryResourceHandle::from(
            unsafe { BoundMemoryResource::static_allocatable_bytes(ptr, len) }.map_err(|_| {
                CurrentFiberAsyncRuntimeError::Executor(super::ExecutorError::Unsupported)
            })?,
        );
        self.from_bound_slab(slab)
    }

    /// Builds one split current-thread fiber + async bundle from one caller-owned static slab.
    ///
    /// # Safety
    ///
    /// The caller must guarantee the supplied pointer/length pair names one valid writable static
    /// extent for the whole lifetime of the split bundle.
    ///
    /// # Errors
    ///
    /// Returns any honest binding, sizing, partitioning, or bootstrap failure.
    pub unsafe fn from_static_slab_parts(
        self,
        ptr: *mut u8,
        len: usize,
    ) -> Result<CurrentFiberAsyncParts, CurrentFiberAsyncRuntimeError> {
        let slab = MemoryResourceHandle::from(
            unsafe { BoundMemoryResource::static_allocatable_bytes(ptr, len) }.map_err(|_| {
                CurrentFiberAsyncRuntimeError::Executor(super::ExecutorError::Unsupported)
            })?,
        );
        self.from_bound_slab_parts(slab)
    }
}

pub(super) fn current_runtime_error_from_owned_backing(
    error: RuntimeBackingError,
) -> CurrentFiberAsyncRuntimeError {
    let executor_error = match error.kind() {
        RuntimeBackingErrorKind::Unsupported => super::ExecutorError::Unsupported,
        RuntimeBackingErrorKind::Invalid => {
            super::ExecutorError::Sync(crate::sync::SyncErrorKind::Invalid)
        }
        RuntimeBackingErrorKind::ResourceExhausted => {
            super::ExecutorError::Sync(crate::sync::SyncErrorKind::Overflow)
        }
        RuntimeBackingErrorKind::StateConflict => {
            super::ExecutorError::Sync(crate::sync::SyncErrorKind::Busy)
        }
    };
    CurrentFiberAsyncRuntimeError::Executor(executor_error)
}
