impl GreenPool {
    /// Returns the low-level fiber support available on the current backend.
    #[must_use]
    pub fn support() -> FiberSupport {
        FiberSystem::new().support()
    }

    /// Returns the shared automatic hosted fiber pool, creating it on first use.
    ///
    /// The current automatic carrier default prefers HAL-reported visible physical cores, then
    /// falls back to visible logical CPUs, and otherwise uses one carrier.
    ///
    /// # Errors
    ///
    /// Returns an honest bootstrap failure if the automatic carrier or fiber pool cannot be
    /// realized on the current platform.
    #[cfg(feature = "std")]
    pub fn automatic() -> Result<Self, FiberError> {
        let slot = AUTOMATIC_FIBER_RUNTIME
            .get_or_init(|| SyncMutex::new(None))
            .map_err(fiber_error_from_sync)?;
        let mut guard = slot.lock().map_err(fiber_error_from_sync)?;
        if let Some(runtime) = guard.as_ref() {
            return runtime.fibers.try_clone();
        }

        let runtime = build_automatic_fiber_runtime()?;
        let fibers = runtime.fibers.try_clone()?;
        *guard = Some(runtime);
        Ok(fibers)
    }

    /// Creates a green-thread pool on top of the supplied carrier pool.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the selected fiber backend cannot support the requested
    /// scheduling and migration contract, or the configured slab-backed stack pool cannot be
    /// realized.
    #[allow(clippy::too_many_lines)]
    pub fn new(config: &FiberPoolConfig<'_>, carrier: &ThreadPool) -> Result<Self, FiberError> {
        let carrier_workers = carrier
            .worker_count()
            .map_err(fiber_error_from_thread_pool)?;
        fusion_sys::fiber::prime_fiber_runtime_substrate()?;
        if let Some(backing) = green_pool_owned_backing(config, carrier_workers)? {
            return Self::from_backing(config, carrier, carrier_workers, backing);
        }
        let inner = build_hosted_green_inner(config, carrier_workers)?;
        launch_thread_pool_green_carriers(&inner, carrier)?;
        Ok(Self { inner })
    }

    fn from_backing(
        config: &FiberPoolConfig<'_>,
        carrier: &ThreadPool,
        carrier_workers: usize,
        backing: GreenPoolOwnedBacking,
    ) -> Result<Self, FiberError> {
        let support = GreenPool::support();
        if !support.context.caps.contains(ContextCaps::MAKE)
            || !support.context.caps.contains(ContextCaps::SWAP)
        {
            return Err(FiberError::unsupported());
        }
        if support.context.guard_required && config.guard_pages == 0 {
            return Err(FiberError::invalid());
        }

        let task_capacity_per_carrier = config.task_capacity_per_carrier()?;
        if config.growth_chunk == 0 || task_capacity_per_carrier == 0 || carrier_workers == 0 {
            return Err(FiberError::invalid());
        }
        if !config.uses_classes() && config.growth_chunk > config.max_fibers_per_carrier {
            return Err(FiberError::invalid());
        }
        if matches!(config.scheduling, GreenScheduling::Priority) && carrier_workers > 1 {
            return Err(FiberError::unsupported());
        }
        if matches!(config.scheduling, GreenScheduling::WorkStealing)
            && support.context.migration != ContextMigrationSupport::CrossCarrier
        {
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
        let reactor_enabled = EventSystem::new()
            .support()
            .caps
            .contains(EventCaps::READINESS)
            && system_fiber_host().support().wake_signal
            && matches!(config.reactor_policy, GreenReactorPolicy::Automatic);
        let metadata_region = unsafe { backing.runtime_metadata.view().raw_region() };
        let (pool_metadata, tasks, carriers) = GreenPoolMetadata::new_in_region(
            metadata_region,
            carrier_workers,
            task_capacity,
            config.scheduling,
            config.priority_age_cap,
            reactor_enabled,
            false,
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
                yield_budget_runtime: GreenYieldBudgetRuntime::new(carrier_workers),
            },
            pool_metadata,
        )?;
        inner
            .block()
            .metadata
            .initialize_carrier_contexts(inner.ptr)?;
        inner.tasks.initialize_owner(inner.as_ptr());
        launch_thread_pool_green_carriers(&inner, carrier)?;
        Ok(Self { inner })
    }

    #[cfg(feature = "std")]
    fn build_hosted_direct(
        config: &FiberPoolConfig<'_>,
        runtime: HostedFiberRuntimeConfig<'_>,
    ) -> Result<(Self, HostedDirectCarrierSet), FiberError> {
        let inner = build_hosted_green_inner(config, runtime.carrier_count)?;
        let carriers = HostedDirectCarrierSet::new(runtime, &inner)?;
        Ok((Self { inner }, carriers))
    }

    /// Returns the currently configured low-level support surface.
    #[must_use]
    pub fn fiber_support(&self) -> FiberSupport {
        self.inner.support
    }

    /// Returns the number of active green threads currently admitted.
    #[must_use]
    pub fn active_count(&self) -> usize {
        self.inner.active.load(Ordering::Acquire)
    }

    /// Returns the owning courier identity for this carrier-backed fiber lane, when configured.
    #[must_use]
    pub fn courier_id(&self) -> Option<CourierId> {
        self.inner.courier_id
    }

    /// Returns a courier-facing run summary for this carrier-backed fiber lane.
    ///
    /// # Errors
    ///
    /// Returns an error if the task registry cannot be observed honestly.
    pub fn runtime_summary(&self) -> Result<CourierRuntimeSummary, FiberError> {
        self.runtime_summary_with_responsiveness(CourierResponsiveness::Responsive)
    }

    /// Returns a courier-facing run summary for this carrier-backed fiber lane using one
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

    /// Returns whether this live pool can honestly admit the requested task class.
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

    /// Validates one compile-time explicit fiber task against this live pool.
    ///
    /// # Errors
    ///
    /// Returns an error when the task's declared contract is invalid or not provisioned by the
    /// current pool.
    pub fn validate_explicit_task<T: ExplicitFiberTask>(&self) -> Result<(), FiberError> {
        self.validate_task_attributes(T::task_attributes()?)
    }

    /// Validates one build-generated explicit fiber task against this live pool.
    ///
    /// # Errors
    ///
    /// Returns an error when generated metadata is missing or invalid for the task, or when the
    /// resulting stack class is not provisioned by the current pool.
    #[cfg(not(feature = "critical-safe"))]
    pub fn validate_generated_task<T: GeneratedExplicitFiberTask>(&self) -> Result<(), FiberError> {
        self.validate_task_attributes(T::task_attributes()?)
    }

    /// Validates one build-generated explicit fiber task against this live pool using its
    /// compile-time generated contract directly.
    ///
    /// This is the cross-crate contract-first path for ordinary builds that want compile-time
    /// generated contracts without depending on runtime metadata lookup.
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

    /// Validates one build-generated explicit fiber task against this live pool.
    ///
    /// In strict generated-contract builds, admission must come from a compile-time generated
    /// contract instead of the runtime metadata lookup table.
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

    /// Returns an approximate stack-telemetry snapshot for live fibers.
    #[must_use]
    pub fn stack_stats(&self) -> Option<FiberStackStats> {
        self.inner.stacks.stack_stats()
    }

    /// Returns the exact live memory footprint of this carrier-backed pool.
    #[must_use]
    pub fn memory_footprint(&self) -> FiberPoolMemoryFootprint {
        self.inner.memory_footprint()
    }

    /// Spawns one green-thread job onto the carrier-backed scheduler.
    ///
    /// # Errors
    ///
    /// Returns an error when the pool is shut down, capacity is exhausted, the inline task
    /// storage cannot contain the submitted closure, or a new fiber cannot be constructed on the
    /// slab-backed stack store.
    pub fn spawn<F, T>(&self, job: F) -> Result<GreenHandle<T>, FiberError>
    where
        F: FnOnce() -> T + Send + 'static,
        T: 'static,
    {
        let task = closure_spawn_task_attributes::<F>(self.inner.stacks.default_task_class()?)?;
        self.spawn_with_attrs(task, job)
    }

    /// Spawns one green-thread job with an explicit stack-byte contract.
    ///
    /// # Errors
    ///
    /// Returns an error when the declared stack bytes cannot be mapped to a supported class.
    pub fn spawn_with_stack<const STACK_BYTES: usize, F, T>(
        &self,
        job: F,
    ) -> Result<GreenHandle<T>, FiberError>
    where
        F: FnOnce() -> T + Send + 'static,
        T: 'static,
    {
        self.spawn_with_attrs(task_attributes_from_stack_bytes::<STACK_BYTES>()?, job)
    }

    /// Spawns one explicit fiber task carrying compile-time stack metadata.
    ///
    /// This is the initial bridge between the public runtime and the planned build-time
    /// stack-budget tooling.
    ///
    /// # Errors
    ///
    /// Returns an error when the task's declared stack contract cannot be mapped to a supported
    /// class, or when ordinary green-task admission fails.
    pub fn spawn_planned<T>(&self, task: T) -> Result<GreenHandle<T::Output>, FiberError>
    where
        T: ExplicitFiberTask,
    {
        self.spawn_explicit(task)
    }

    /// Spawns one explicit fiber task carrying compile-time stack metadata.
    ///
    /// This is the initial bridge between the public runtime and the planned build-time
    /// stack-budget tooling.
    ///
    /// # Errors
    ///
    /// Returns an error when the task's declared stack contract cannot be mapped to a supported
    /// class, or when ordinary green-task admission fails.
    pub fn spawn_explicit<T>(&self, task: T) -> Result<GreenHandle<T::Output>, FiberError>
    where
        T: ExplicitFiberTask,
    {
        let attributes = T::task_attributes()?;
        self.validate_task_attributes(attributes)?;
        spawn_explicit_task_on_lease(
            &self.inner,
            attributes,
            task,
            fusion_sys::courier::CourierFiberClass::Planned,
            true,
            GreenHandleDriveMode::CarrierPool,
        )
    }

    /// Spawns one explicit fiber task using build-generated stack metadata.
    ///
    /// # Errors
    ///
    /// Returns an error when generated metadata is missing or invalid for the task type, or when
    /// ordinary green-task admission fails.
    #[cfg(not(feature = "critical-safe"))]
    pub fn spawn_generated<T>(&self, task: T) -> Result<GreenHandle<T::Output>, FiberError>
    where
        T: GeneratedExplicitFiberTask,
    {
        let attributes = T::task_attributes()?;
        self.validate_task_attributes(attributes)?;
        spawn_generated_task_on_lease(
            &self.inner,
            attributes,
            task,
            fusion_sys::courier::CourierFiberClass::Planned,
            true,
            GreenHandleDriveMode::CarrierPool,
        )
    }

    /// Spawns one explicit fiber task using a compile-time generated contract.
    ///
    /// In strict generated-contract builds, admission must come from a compile-time generated
    /// contract instead of the runtime metadata lookup table.
    ///
    /// # Errors
    ///
    /// Returns an error when the generated contract is not provisioned by the current pool, or
    /// when ordinary green-task admission fails.
    #[cfg(feature = "critical-safe")]
    pub fn spawn_generated<T>(&self, task: T) -> Result<GreenHandle<T::Output>, FiberError>
    where
        T: GeneratedExplicitFiberTask + GeneratedExplicitFiberTaskContract,
    {
        let attributes = generated_explicit_task_contract_attributes::<T>()
            .with_optional_yield_budget(T::YIELD_BUDGET);
        self.validate_task_attributes(attributes)?;
        spawn_generated_task_on_lease(
            &self.inner,
            attributes,
            task,
            fusion_sys::courier::CourierFiberClass::Planned,
            true,
            GreenHandleDriveMode::CarrierPool,
        )
    }

    /// Spawns one explicit fiber task using a compile-time generated contract directly.
    ///
    /// This is the cross-crate contract-first path for ordinary builds that want compile-time
    /// generated contracts without depending on runtime metadata lookup.
    ///
    /// # Errors
    ///
    /// Returns an error when the generated contract is not provisioned by the current pool, or
    /// when ordinary green-task admission fails.
    pub fn spawn_generated_contract<T>(&self, task: T) -> Result<GreenHandle<T::Output>, FiberError>
    where
        T: GeneratedExplicitFiberTask + GeneratedExplicitFiberTaskContract,
    {
        let attributes = generated_explicit_task_contract_attributes::<T>()
            .with_optional_yield_budget(T::YIELD_BUDGET);
        self.validate_task_attributes(attributes)?;
        spawn_generated_task_on_lease(
            &self.inner,
            attributes,
            task,
            fusion_sys::courier::CourierFiberClass::Planned,
            true,
            GreenHandleDriveMode::CarrierPool,
        )
    }

    /// Spawns one green-thread job with explicit stack-class and priority metadata.
    ///
    /// This is a transitional admission API. The current substrate still has one global backing
    /// slab, so the requested stack class is validated against the pool envelope and stored with
    /// the task record, but it does not yet select among class-specific stack pools.
    ///
    /// # Errors
    ///
    /// Returns an error when the task requests a stack class the current pool cannot satisfy, the
    /// pool is shut down, capacity is exhausted, the inline task storage cannot contain the
    /// submitted closure, or a new fiber cannot be constructed on the slab-backed stack store.
    pub fn spawn_with_attrs<F, T>(
        &self,
        task: FiberTaskAttributes,
        job: F,
    ) -> Result<GreenHandle<T>, FiberError>
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
    ) -> Result<GreenHandle<T>, FiberError>
    where
        F: FnOnce() -> T + Send + 'static,
        T: 'static,
    {
        spawn_on_lease(
            &self.inner,
            task,
            job,
            class,
            true,
            GreenHandleDriveMode::CarrierPool,
            true,
        )
    }

    /// Requests scheduler shutdown and wakes every carrier loop.
    ///
    /// # Errors
    ///
    /// Returns an error if the wakeup path cannot be signaled honestly.
    pub fn shutdown(&self) -> Result<(), FiberError> {
        self.inner.request_shutdown()
    }
}

#[derive(Debug)]
struct GreenPoolOwnedBacking {
    control: MemoryResourceHandle,
    runtime_metadata: MemoryResourceHandle,
    stack_metadata: MemoryResourceHandle,
    stacks: MemoryResourceHandle,
    slab_owner: Option<fusion_sys::alloc::ExtentLease>,
}

fn green_pool_owned_backing(
    config: &FiberPoolConfig<'_>,
    carrier_workers: usize,
) -> Result<Option<GreenPoolOwnedBacking>, FiberError> {
    if !uses_explicit_bound_runtime_backing() {
        return Ok(None);
    }
    if carrier_workers == 0 || !config.classes.is_empty() || config.guard_pages != 0 {
        return Ok(None);
    }

    let support = FiberSystem::new().support();
    if !support.context.caps.contains(ContextCaps::MAKE)
        || !support.context.caps.contains(ContextCaps::SWAP)
    {
        return Err(FiberError::unsupported());
    }
    if matches!(
        apply_fiber_sizing_strategy_backing(config.stack_backing, config.sizing)?,
        FiberStackBacking::Elastic { .. }
    ) {
        return Ok(None);
    }

    let alignment = support.context.min_stack_alignment.max(16);
    let (_, backing) =
        FiberStackSlab::build_backing(config.stack_backing, 0, 1, alignment, support.context.stack_direction)?;
    if matches!(backing, FiberStackBackingState::Elastic { .. }) {
        return Ok(None);
    }

    let task_capacity = config.task_capacity_per_carrier()?;
    let reactor_enabled = EventSystem::new()
        .support()
        .caps
        .contains(EventCaps::READINESS)
        && system_fiber_host().support().wake_signal
        && matches!(config.reactor_policy, GreenReactorPolicy::Automatic);
    let stacks = apply_fiber_backing_request(
        FiberPoolBackingRequest {
            bytes: config
                .max_fibers_per_carrier
                .checked_mul(FiberStackSlab::build_backing(
                    config.stack_backing,
                    0,
                    1,
                    alignment,
                    support.context.stack_direction,
                )?
                .0)
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
                carrier_workers,
                task_capacity,
                config.scheduling,
                reactor_enabled,
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

    let plan = CurrentFiberPoolBackingPlan {
        control,
        runtime_metadata,
        stack_metadata,
        stacks,
    }
    .combined()?;
    let Some(slab) = allocate_owned_runtime_slab(plan.slab.bytes, plan.slab.align)
        .map_err(fiber_error_from_current_runtime_backing)?
    else {
        return Ok(None);
    };
    Ok(Some(GreenPoolOwnedBacking {
        control: partition_bound_resource(&slab.handle, plan.control)?,
        runtime_metadata: partition_bound_resource(&slab.handle, plan.runtime_metadata)?,
        stack_metadata: partition_bound_resource(&slab.handle, plan.stack_metadata)?,
        stacks: partition_bound_resource(&slab.handle, plan.stacks)?,
        slab_owner: Some(slab.lease),
    }))
}

impl GreenPool {
    /// Attempts to clone one green-thread pool handle.
    ///
    /// # Errors
    ///
    /// Returns an error when the shared pool root cannot be retained honestly.
    pub fn try_clone(&self) -> Result<Self, FiberError> {
        let inner = self.inner.try_clone()?;
        inner.client_refs.fetch_add(1, Ordering::AcqRel);
        Ok(Self { inner })
    }
}

impl Drop for GreenPool {
    fn drop(&mut self) {
        if self.inner.client_refs.fetch_sub(1, Ordering::AcqRel) == 1 {
            let _ = self.inner.request_shutdown();
        }
    }
}

#[cfg(feature = "std")]
fn build_automatic_fiber_runtime() -> Result<HostedFiberRuntime, FiberError> {
    HostedFiberRuntime::from_bootstrap_with(
        FiberPoolBootstrap::from_config(automatic_fiber_config()),
        HostedFiberRuntimeConfig::automatic(),
    )
}

#[cfg(feature = "std")]
fn automatic_carrier_count() -> usize {
    hal_visible_carrier_count()
        .filter(|count| *count != 0)
        .unwrap_or(1)
}

#[cfg(feature = "std")]
const fn automatic_pool_placement(carrier_count: usize) -> PoolPlacement<'static> {
    if carrier_count > 1 {
        PoolPlacement::PerCore
    } else {
        PoolPlacement::Inherit
    }
}

#[cfg(feature = "std")]
fn per_carrier_capacity_for_total(
    total_fibers: usize,
    carrier_count: usize,
) -> Result<usize, FiberError> {
    if total_fibers == 0 || carrier_count == 0 {
        return Err(FiberError::invalid());
    }
    let adjusted = total_fibers
        .checked_add(carrier_count - 1)
        .ok_or_else(FiberError::resource_exhausted)?;
    Ok(adjusted / carrier_count)
}

#[cfg(feature = "std")]
fn distribute_hosted_class_configs(
    classes: &[HostedFiberClassConfig],
    carrier_count: usize,
) -> Result<std::vec::Vec<FiberStackClassConfig>, FiberError> {
    if classes.is_empty() || carrier_count == 0 {
        return Err(FiberError::invalid());
    }

    let mut distributed = std::vec::Vec::with_capacity(classes.len());
    for class in classes {
        let class = class.validate()?;
        let slots_per_carrier = per_carrier_capacity_for_total(class.total_slots, carrier_count)?;
        let growth_chunk = per_carrier_capacity_for_total(class.growth_chunk, carrier_count)?;
        distributed.push(
            FiberStackClassConfig::new(class.class, slots_per_carrier)?
                .with_growth_chunk(growth_chunk)?,
        );
    }
    Ok(distributed)
}

#[cfg(feature = "std")]
fn hal_visible_carrier_count() -> Option<usize> {
    system_cpu()
        .topology_summary()
        .ok()
        .and_then(select_automatic_carrier_count)
        .filter(|count| *count != 0)
}

#[cfg(feature = "std")]
const fn select_automatic_carrier_count(summary: HardwareTopologySummary) -> Option<usize> {
    carrier_count_for_profile(summary, CarrierWorkloadProfile::DedicatedCore)
}

#[cfg(feature = "std")]
fn automatic_fiber_config() -> FiberPoolConfig<'static> {
    let mut config = FiberPoolConfig {
        max_fibers_per_carrier: 1024,
        growth_chunk: 32,
        ..FiberPoolConfig::new()
    };
    config.huge_pages = automatic_huge_page_policy(config.stack_backing);
    config
}

#[cfg(feature = "std")]
fn automatic_huge_page_policy(backing: FiberStackBacking) -> HugePagePolicy {
    let FiberStackBacking::Elastic { max_size, .. } = backing else {
        return HugePagePolicy::Disabled;
    };
    if max_size.get() < HugePageSize::TwoMiB.bytes() {
        return HugePagePolicy::Disabled;
    }
    if !system_mem()
        .support()
        .advice
        .contains(MemAdviceCaps::HUGE_PAGE)
    {
        return HugePagePolicy::Disabled;
    }
    HugePagePolicy::Enabled {
        size: HugePageSize::TwoMiB,
    }
}

const fn initial_steal_seed(carrier_index: usize) -> usize {
    let seed = carrier_index.wrapping_add(1).wrapping_mul(STEAL_SEED_MIX);
    if seed == 0 { 1 } else { seed }
}

const fn xorshift_word(mut state: usize) -> usize {
    #[cfg(target_pointer_width = "64")]
    {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
    }

    #[cfg(target_pointer_width = "32")]
    {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
    }

    #[cfg(not(any(target_pointer_width = "32", target_pointer_width = "64")))]
    {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
    }

    if state == 0 { 1 } else { state }
}

const fn xorshift64(state: usize) -> usize {
    xorshift_word(state)
}

fn saturating_duration_to_nanos_u64(duration: Duration) -> u64 {
    let nanos = duration.as_nanos();
    u64::try_from(nanos).unwrap_or(u64::MAX)
}

fn current_monotonic_nanos() -> Result<u64, FiberError> {
    let now = ThreadSystem::new()
        .monotonic_now()
        .map_err(fiber_error_from_thread_pool)?;
    Ok(saturating_duration_to_nanos_u64(now))
}

fn yield_budget_enforcement_supported() -> bool {
    ThreadSystem::new()
        .support()
        .scheduler
        .caps
        .contains(ThreadSchedulerCaps::MONOTONIC_NOW)
}

unsafe fn green_task_entry(context: *mut ()) -> FiberReturn {
    FUSION_GREEN_TASK_ENTRY_PHASE.store(1, Ordering::Release);
    let slot = unsafe { &*context.cast::<GreenTaskSlot>() };
    let Ok(id) = slot.current_id() else {
        FUSION_GREEN_TASK_ENTRY_FAILURE_KIND.store(1, Ordering::Release);
        return FiberReturn::new(usize::MAX);
    };
    FUSION_GREEN_TASK_ENTRY_PHASE.store(2, Ordering::Release);

    let runner = match slot.take_job_runner(id) {
        Ok(runner) => runner,
        Err(error) => {
            let code = match error.kind() {
                FiberErrorKind::Unsupported => 10,
                FiberErrorKind::Invalid => 11,
                FiberErrorKind::ResourceExhausted => 12,
                FiberErrorKind::DeadlineExceeded => 13,
                FiberErrorKind::StateConflict => 14,
                FiberErrorKind::Context(kind) => match kind {
                    ContextErrorKind::Unsupported => 100,
                    ContextErrorKind::Invalid => 101,
                    ContextErrorKind::Busy => 102,
                    ContextErrorKind::PermissionDenied => 103,
                    ContextErrorKind::ResourceExhausted => 104,
                    ContextErrorKind::StateConflict => 105,
                    ContextErrorKind::Platform(_) => 106,
                },
            };
            FUSION_GREEN_TASK_ENTRY_FAILURE_KIND.store(code, Ordering::Release);
            let _ = slot.set_state(id, GreenTaskState::Failed(error));
            return FiberReturn::new(usize::MAX);
        }
    };
    FUSION_GREEN_TASK_ENTRY_PHASE.store(3, Ordering::Release);

    #[cfg(feature = "std")]
    if run_green_job_contained(runner).is_err() {
        FUSION_GREEN_TASK_ENTRY_FAILURE_KIND.store(2, Ordering::Release);
        let _ = slot.set_state(id, GreenTaskState::Failed(FiberError::state_conflict()));
        return FiberReturn::new(usize::MAX);
    }

    #[cfg(not(feature = "std"))]
    {
        FUSION_GREEN_TASK_ENTRY_PHASE.store(4, Ordering::Release);
        run_green_job_contained(runner);
    }

    FUSION_GREEN_TASK_ENTRY_PHASE.store(5, Ordering::Release);
    FiberReturn::new(0)
}

fn run_carrier_loop(
    inner: &GreenPoolInner,
    context: &CarrierLoopContext,
) -> Result<(), FiberError> {
    let carrier_index = context.carrier_index;
    FUSION_GREEN_CARRIER_PHASE.store(1, Ordering::Release);
    if inner.carriers[carrier_index].reactor.is_some() {
        return run_reactor_carrier_loop(inner, context);
    }

    let _alt_stack = if inner.stacks.requires_signal_handler() {
        Some(install_carrier_signal_stack()?)
    } else {
        None
    };
    loop {
        #[cfg(feature = "std")]
        context.publish_current_observation();
        while let Some(slot_index) = dequeue_ready(inner, carrier_index)? {
            FUSION_GREEN_CARRIER_READY_COUNT.fetch_add(1, Ordering::AcqRel);
            FUSION_GREEN_CARRIER_PHASE.store(2, Ordering::Release);
            if let Err(error) = run_ready_task(inner, carrier_index, slot_index) {
                trace_carrier_failure("run_carrier_loop.run_ready_task", carrier_index, &error);
                return Err(error);
            }
        }
        if let Some(slot_index) = inner.try_steal_ready(carrier_index)? {
            FUSION_GREEN_CARRIER_READY_COUNT.fetch_add(1, Ordering::AcqRel);
            FUSION_GREEN_CARRIER_PHASE.store(3, Ordering::Release);
            if let Err(error) = run_ready_task(inner, carrier_index, slot_index) {
                trace_carrier_failure("run_carrier_loop.run_stolen_task", carrier_index, &error);
                return Err(error);
            }
            continue;
        }
        if inner.shutdown.load(Ordering::Acquire) {
            break;
        }
        #[cfg(feature = "std")]
        {
            let carrier = &inner.carriers[carrier_index];
            FUSION_GREEN_CARRIER_PHASE.store(4, Ordering::Release);
            if let Err(error) = carrier.ready.acquire().map_err(fiber_error_from_sync) {
                trace_carrier_failure("run_carrier_loop.ready.acquire", carrier_index, &error);
                return Err(error);
            }
        }
        #[cfg(not(feature = "std"))]
        {
            FUSION_GREEN_CARRIER_PHASE.store(5, Ordering::Release);
            let _ = system_thread().yield_now();
        }
    }
    FUSION_GREEN_CARRIER_PHASE.store(6, Ordering::Release);
    Ok(())
}

fn run_reactor_carrier_loop(
    inner: &GreenPoolInner,
    context: &CarrierLoopContext,
) -> Result<(), FiberError> {
    let carrier_index = context.carrier_index;
    let _alt_stack = if inner.stacks.requires_signal_handler() {
        Some(install_carrier_signal_stack()?)
    } else {
        None
    };
    let reactor = inner.carriers[carrier_index]
        .reactor
        .as_ref()
        .ok_or_else(FiberError::unsupported)?;

    loop {
        #[cfg(feature = "std")]
        context.publish_current_observation();
        while let Some(slot_index) = dequeue_ready(inner, carrier_index)? {
            if let Err(error) = run_ready_task(inner, carrier_index, slot_index) {
                trace_carrier_failure(
                    "run_reactor_carrier_loop.run_ready_task",
                    carrier_index,
                    &error,
                );
                return Err(error);
            }
        }
        if let Some(slot_index) = inner.try_steal_ready(carrier_index)? {
            if let Err(error) = run_ready_task(inner, carrier_index, slot_index) {
                trace_carrier_failure(
                    "run_reactor_carrier_loop.run_stolen_task",
                    carrier_index,
                    &error,
                );
                return Err(error);
            }
            continue;
        }

        if inner.shutdown.load(Ordering::Acquire) {
            while let Some(waiter) = reactor.cancel_one_waiter()? {
                inner.finish_task(
                    waiter.slot_index,
                    waiter.task_id,
                    GreenTaskState::Failed(FiberError::state_conflict()),
                )?;
            }
            if reactor.waiter_count()? == 0 {
                break;
            }
            continue;
        }

        let mut ready = [None; CARRIER_EVENT_BATCH];
        let poll_result = match reactor.poll_ready(None, &mut ready) {
            Ok(poll_result) => poll_result,
            Err(error) => {
                trace_carrier_failure("run_reactor_carrier_loop.poll_ready", carrier_index, &error);
                return Err(error);
            }
        };
        if poll_result.capacity_signaled {
            inner.dispatch_capacity_for_carrier(carrier_index)?;
        }
        for waiter in ready.into_iter().take(poll_result.ready_count).flatten() {
            inner
                .tasks
                .set_state(waiter.slot_index, waiter.task_id, GreenTaskState::Yielded)?;
            inner.enqueue_with_signal(carrier_index, waiter.slot_index, false)?;
        }
    }
    Ok(())
}

fn install_carrier_signal_stack() -> Result<PlatformFiberSignalStack, FiberError> {
    let host = system_fiber_host();
    host.ensure_elastic_fault_handler(elastic_stack_fault_handler)
        .map_err(fiber_error_from_host)?;
    host.install_signal_stack().map_err(fiber_error_from_host)
}

fn dequeue_ready(
    inner: &GreenPoolInner,
    carrier_index: usize,
) -> Result<Option<usize>, FiberError> {
    let carrier = inner
        .carriers
        .get(carrier_index)
        .ok_or_else(FiberError::invalid)?;
    let slot_index = carrier.queue.with(CarrierReadyQueue::dequeue)?;
    Ok(slot_index)
}

#[allow(clippy::too_many_lines)]
fn run_ready_task(
    inner: &GreenPoolInner,
    carrier_index: usize,
    slot_index: usize,
) -> Result<(), FiberError> {
    let slot = inner.tasks.slot(slot_index)?;
    FUSION_GREEN_RESUME_PHASE.store(5, Ordering::Release);
    let (task_id, yield_budget, execution) = match slot.begin_run() {
        Ok(values) => values,
        Err(error) => {
            FUSION_GREEN_RESUME_ERROR_KIND.store(
                match error.kind() {
                    FiberErrorKind::Unsupported => 1,
                    FiberErrorKind::Invalid => 2,
                    FiberErrorKind::ResourceExhausted => 3,
                    FiberErrorKind::DeadlineExceeded => 4,
                    FiberErrorKind::StateConflict => 5,
                    FiberErrorKind::Context(kind) => match kind {
                        ContextErrorKind::Unsupported => 100,
                        ContextErrorKind::Invalid => 101,
                        ContextErrorKind::Busy => 102,
                        ContextErrorKind::PermissionDenied => 103,
                        ContextErrorKind::ResourceExhausted => 104,
                        ContextErrorKind::StateConflict => 105,
                        ContextErrorKind::Platform(_) => 106,
                    },
                },
                Ordering::Release,
            );
            FUSION_GREEN_RESUME_PHASE.store(6, Ordering::Release);
            trace_carrier_failure("run_ready_task.begin_run", carrier_index, &error);
            return Err(error);
        }
    };
    FUSION_GREEN_RESUME_PHASE.store(7, Ordering::Release);
    FUSION_GREEN_RESUME_PHASE.store(8, Ordering::Release);
    let runtime_fiber_id = match inner.tasks.current_fiber_id(slot_index) {
        Ok(id) => id,
        Err(error) => {
            FUSION_GREEN_RESUME_ERROR_KIND.store(
                match error.kind() {
                    FiberErrorKind::Unsupported => 1,
                    FiberErrorKind::Invalid => 2,
                    FiberErrorKind::ResourceExhausted => 3,
                    FiberErrorKind::DeadlineExceeded => 4,
                    FiberErrorKind::StateConflict => 5,
                    FiberErrorKind::Context(kind) => match kind {
                        ContextErrorKind::Unsupported => 100,
                        ContextErrorKind::Invalid => 101,
                        ContextErrorKind::Busy => 102,
                        ContextErrorKind::PermissionDenied => 103,
                        ContextErrorKind::ResourceExhausted => 104,
                        ContextErrorKind::StateConflict => 105,
                        ContextErrorKind::Platform(_) => 106,
                    },
                },
                Ordering::Release,
            );
            FUSION_GREEN_RESUME_PHASE.store(9, Ordering::Release);
            trace_carrier_failure("run_ready_task.current_fiber_id", carrier_index, &error);
            return Err(error);
        }
    };
    FUSION_GREEN_RESUME_PHASE.store(16, Ordering::Release);
    if let Err(error) = inner.update_runtime_fiber(
        runtime_fiber_id,
        fusion_sys::fiber::FiberState::Running,
        true,
    ) {
        FUSION_GREEN_RESUME_ERROR_KIND.store(
            match error.kind() {
                FiberErrorKind::Unsupported => 1,
                FiberErrorKind::Invalid => 2,
                FiberErrorKind::ResourceExhausted => 3,
                FiberErrorKind::DeadlineExceeded => 4,
                FiberErrorKind::StateConflict => 5,
                FiberErrorKind::Context(kind) => match kind {
                    ContextErrorKind::Unsupported => 100,
                    ContextErrorKind::Invalid => 101,
                    ContextErrorKind::Busy => 102,
                    ContextErrorKind::PermissionDenied => 103,
                    ContextErrorKind::ResourceExhausted => 104,
                    ContextErrorKind::StateConflict => 105,
                    ContextErrorKind::Platform(_) => 106,
                },
            },
            Ordering::Release,
        );
        FUSION_GREEN_RESUME_PHASE.store(17, Ordering::Release);
        trace_carrier_failure("run_ready_task.update_runtime_fiber", carrier_index, &error);
        return Err(error);
    }
    FUSION_GREEN_RESUME_PHASE.store(18, Ordering::Release);
    if !execution.requires_fiber() {
        let runner = match slot.take_job_runner(task_id) {
            Ok(runner) => runner,
            Err(error) => {
                trace_carrier_failure("run_ready_task.take_job_runner", carrier_index, &error);
                inner.finish_task(slot_index, task_id, GreenTaskState::Failed(error))?;
                return Ok(());
            }
        };

        #[cfg(feature = "std")]
        let run_result = run_green_job_contained(runner);
        #[cfg(not(feature = "std"))]
        let run_result = {
            run_green_job_contained(runner);
            Ok(())
        };

        match run_result {
            Ok(()) => inner.finish_task(slot_index, task_id, GreenTaskState::Completed)?,
            Err(()) => inner.finish_task(
                slot_index,
                task_id,
                GreenTaskState::Failed(FiberError::state_conflict()),
            )?,
        }
        return Ok(());
    }
    if let Err(error) = slot.set_yield_action(CurrentGreenYieldAction::Requeue) {
        FUSION_GREEN_RESUME_ERROR_KIND.store(
            match error.kind() {
                FiberErrorKind::Unsupported => 1,
                FiberErrorKind::Invalid => 2,
                FiberErrorKind::ResourceExhausted => 3,
                FiberErrorKind::DeadlineExceeded => 4,
                FiberErrorKind::StateConflict => 5,
                FiberErrorKind::Context(kind) => match kind {
                    ContextErrorKind::Unsupported => 100,
                    ContextErrorKind::Invalid => 101,
                    ContextErrorKind::Busy => 102,
                    ContextErrorKind::PermissionDenied => 103,
                    ContextErrorKind::ResourceExhausted => 104,
                    ContextErrorKind::StateConflict => 105,
                    ContextErrorKind::Platform(_) => 106,
                },
            },
            Ordering::Release,
        );
        FUSION_GREEN_RESUME_PHASE.store(10, Ordering::Release);
        trace_carrier_failure("run_ready_task.set_yield_action", carrier_index, &error);
        return Err(error);
    }
    FUSION_GREEN_RESUME_PHASE.store(11, Ordering::Release);
    if inner.support.context.migration != ContextMigrationSupport::CrossCarrier {
        let context = match inner.tasks.slot_context(slot_index) {
            Ok(context) => context,
            Err(error) => {
                FUSION_GREEN_RESUME_ERROR_KIND.store(
                    match error.kind() {
                        FiberErrorKind::Unsupported => 1,
                        FiberErrorKind::Invalid => 2,
                        FiberErrorKind::ResourceExhausted => 3,
                        FiberErrorKind::DeadlineExceeded => 4,
                        FiberErrorKind::StateConflict => 5,
                        FiberErrorKind::Context(kind) => match kind {
                            ContextErrorKind::Unsupported => 100,
                            ContextErrorKind::Invalid => 101,
                            ContextErrorKind::Busy => 102,
                            ContextErrorKind::PermissionDenied => 103,
                            ContextErrorKind::ResourceExhausted => 104,
                            ContextErrorKind::StateConflict => 105,
                            ContextErrorKind::Platform(_) => 106,
                        },
                    },
                    Ordering::Release,
                );
                FUSION_GREEN_RESUME_PHASE.store(12, Ordering::Release);
                trace_carrier_failure("run_ready_task.slot_context", carrier_index, &error);
                return Err(error);
            }
        };
        FUSION_GREEN_RESUME_PHASE.store(13, Ordering::Release);
        if let Err(error) = inner
            .tasks
            .materialize_fiber(slot_index, task_id, green_task_entry, context)
        {
            FUSION_GREEN_RESUME_ERROR_KIND.store(
                match error.kind() {
                    FiberErrorKind::Unsupported => 1,
                    FiberErrorKind::Invalid => 2,
                    FiberErrorKind::ResourceExhausted => 3,
                    FiberErrorKind::DeadlineExceeded => 4,
                    FiberErrorKind::StateConflict => 5,
                    FiberErrorKind::Context(kind) => match kind {
                        ContextErrorKind::Unsupported => 100,
                        ContextErrorKind::Invalid => 101,
                        ContextErrorKind::Busy => 102,
                        ContextErrorKind::PermissionDenied => 103,
                        ContextErrorKind::ResourceExhausted => 104,
                        ContextErrorKind::StateConflict => 105,
                        ContextErrorKind::Platform(_) => 106,
                    },
                },
                Ordering::Release,
            );
            FUSION_GREEN_RESUME_PHASE.store(14, Ordering::Release);
            trace_carrier_failure("run_ready_task.materialize_fiber", carrier_index, &error);
            return Err(error);
        }
        FUSION_GREEN_RESUME_PHASE.store(15, Ordering::Release);
    }

    let run_started = yield_budget
        .map(|_| current_monotonic_nanos())
        .transpose()?;
    #[cfg(feature = "std")]
    if let Some(start_nanos) = run_started {
        inner.begin_yield_budget_segment(
            carrier_index,
            slot_index,
            task_id,
            yield_budget,
            start_nanos,
        );
    }
    FUSION_GREEN_RESUME_PHASE.store(1, Ordering::Release);
    let resume = match inner.tasks.resume(slot_index, task_id) {
        Ok(resume) => Ok(resume),
        Err(error) => {
            let code = match error.kind() {
                FiberErrorKind::Unsupported => 1,
                FiberErrorKind::Invalid => 2,
                FiberErrorKind::ResourceExhausted => 3,
                FiberErrorKind::DeadlineExceeded => 4,
                FiberErrorKind::StateConflict => 5,
                FiberErrorKind::Context(kind) => match kind {
                    ContextErrorKind::Unsupported => 100,
                    ContextErrorKind::Invalid => 101,
                    ContextErrorKind::Busy => 102,
                    ContextErrorKind::PermissionDenied => 103,
                    ContextErrorKind::ResourceExhausted => 104,
                    ContextErrorKind::StateConflict => 105,
                    ContextErrorKind::Platform(_) => 106,
                },
            };
            FUSION_GREEN_RESUME_ERROR_KIND.store(code, Ordering::Release);
            FUSION_GREEN_RESUME_PHASE.store(4, Ordering::Release);
            trace_carrier_failure("run_ready_task.resume", carrier_index, &error);
            Err(error)
        }
    };
    let observed_budget_runtime = match (yield_budget, run_started) {
        (Some(_budget), Some(start_nanos)) => {
            Duration::from_nanos(current_monotonic_nanos()?.saturating_sub(start_nanos))
        }
        _ => Duration::ZERO,
    };
    let budget_faulted = inner.finish_yield_budget_segment(
        carrier_index,
        task_id,
        yield_budget,
        observed_budget_runtime,
    );

    if budget_faulted {
        inner.dispatch_capacity_for_task(slot_index, task_id)?;
        inner.finish_task(
            slot_index,
            task_id,
            GreenTaskState::Failed(FiberError::deadline_exceeded()),
        )?;
        return Ok(());
    }

    match resume {
        Ok(FiberYield::Yielded) => {
            FUSION_GREEN_RESUME_PHASE.store(2, Ordering::Release);
            match take_current_green_yield_action(inner, slot_index)
            .inspect_err(|error| {
                trace_carrier_failure(
                    "run_ready_task.take_current_green_yield_action",
                    carrier_index,
                    error,
                );
            })? {
            CurrentGreenYieldAction::Requeue => {
                inner
                    .tasks
                    .set_state(slot_index, task_id, GreenTaskState::Yielded)?;
                inner.update_runtime_fiber(
                    runtime_fiber_id,
                    fusion_sys::fiber::FiberState::Suspended,
                    true,
                )?;
                inner.dispatch_capacity_for_task(slot_index, task_id)?;
                inner.enqueue_with_signal(carrier_index, slot_index, false)?;
            }
            CurrentGreenYieldAction::WaitReadiness { source, interest } => {
                inner.update_runtime_fiber(
                    runtime_fiber_id,
                    fusion_sys::fiber::FiberState::Suspended,
                    true,
                )?;
                inner.dispatch_capacity_for_task(slot_index, task_id)?;
                if let Err(error) =
                    inner.park_on_readiness(carrier_index, slot_index, task_id, source, interest)
                {
                    inner.finish_task(slot_index, task_id, GreenTaskState::Failed(error))?;
                }
            }
        }
        }
        Ok(FiberYield::Completed(_)) => {
            FUSION_GREEN_RESUME_PHASE.store(3, Ordering::Release);
            inner.dispatch_capacity_for_task(slot_index, task_id)?;
            inner.finish_task(slot_index, task_id, GreenTaskState::Completed)?;
        }
        Err(error) => {
            inner.dispatch_capacity_for_task(slot_index, task_id)?;
            inner.finish_task(slot_index, task_id, GreenTaskState::Failed(error))?;
        }
    }
    Ok(())
}

/// Yields the current green thread cooperatively.
///
/// # Errors
///
/// Returns an honest error when no active green fiber exists on the current carrier.
pub fn yield_now() -> Result<(), FiberError> {
    ensure_current_green_handoff_unlocked()?;
    set_current_green_yield_action(CurrentGreenYieldAction::Requeue);
    system_yield_now()
}

#[doc(hidden)]
pub fn wait_for_readiness(
    source: EventSourceHandle,
    interest: EventInterest,
) -> Result<(), FiberError> {
    if current_green_context().is_none() {
        return Err(FiberError::state_conflict());
    }
    ensure_current_green_handoff_unlocked()?;
    set_current_green_yield_action(CurrentGreenYieldAction::WaitReadiness { source, interest });
    if let Err(error) = system_yield_now() {
        set_current_green_yield_action(CurrentGreenYieldAction::Requeue);
        return Err(error);
    }
    Ok(())
}

#[doc(hidden)]
pub fn wait_blocking_for_readiness(
    source: EventSourceHandle,
    interest: EventInterest,
) -> Result<(), FiberError> {
    let reactor = EventSystem::new();
    let mut poller = reactor.create().map_err(fiber_error_from_event)?;
    let key = reactor
        .register(
            &mut poller,
            source,
            interest | EventInterest::ERROR | EventInterest::HANGUP,
        )
        .map_err(fiber_error_from_event)?;
    let mut events = [EMPTY_EVENT_RECORD; 1];
    let poll_result = reactor
        .poll(&mut poller, &mut events, None)
        .map_err(fiber_error_from_event);
    let deregister_result = reactor.deregister(&mut poller, key);
    poll_result?;
    deregister_result.map_err(fiber_error_from_event)?;
    Ok(())
}

fn run_capacity_callback_contained(callback: fn(FiberCapacityEvent), event: FiberCapacityEvent) {
    #[cfg(feature = "std")]
    {
        use std::panic::{AssertUnwindSafe, catch_unwind};

        let _ = catch_unwind(AssertUnwindSafe(|| callback(event)));
    }

    #[cfg(not(feature = "std"))]
    {
        callback(event);
    }
}

#[cfg(feature = "std")]
fn run_yield_budget_callback_contained(
    callback: fn(FiberYieldBudgetEvent),
    event: FiberYieldBudgetEvent,
) {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    let _ = catch_unwind(AssertUnwindSafe(|| callback(event)));
}

#[cfg(feature = "std")]
#[allow(clippy::needless_pass_by_value)]
fn run_yield_budget_watchdog(inner: GreenPoolLease) {
    while !inner.shutdown.load(Ordering::Acquire) {
        if inner.scan_yield_budget_overruns().is_err() {
            let _ = inner.request_shutdown();
            break;
        }
        std::thread::sleep(FIBER_YIELD_WATCHDOG_POLL_INTERVAL);
    }
}

#[cfg(feature = "std")]
fn run_green_job_contained(runner: InlineGreenJobRunner) -> Result<(), ()> {
    #[cfg(feature = "std")]
    {
        use std::panic::{AssertUnwindSafe, catch_unwind};

        catch_unwind(AssertUnwindSafe(|| runner.run())).map_err(|_| ())
    }
}

#[cfg(not(feature = "std"))]
fn run_green_job_contained(runner: InlineGreenJobRunner) {
    runner.run();
}

#[cfg(feature = "std")]
unsafe fn run_direct_carrier_thread(context: *mut ()) -> ThreadEntryReturn {
    unsafe { run_carrier_loop_job(context) };
    ThreadEntryReturn::new(0)
}

unsafe fn run_carrier_loop_job(context: *mut ()) {
    FUSION_GREEN_CARRIER_PHASE.store(7, Ordering::Release);
    let context = unsafe { &*context.cast::<CarrierLoopContext>() };
    let inner = unsafe { &context.control.as_ref().inner };
    #[cfg(feature = "std")]
    context.publish_current_observation();
    FUSION_GREEN_CARRIER_PHASE.store(8, Ordering::Release);
    if let Err(_error) = run_carrier_loop(inner, context) {
        #[cfg(feature = "std")]
        {
            if std::env::var_os("FUSION_TRACE_CARRIER_ERRORS").is_some() {
                std::eprintln!(
                    "fusion-std carrier loop error: carrier_index={} kind={:?}",
                    context.carrier_index,
                    _error.kind()
                );
            }
        }
        let _ = inner.request_shutdown();
    }
    let _ = unsafe { release_carrier_loop_context(context) };
}

unsafe fn cancel_carrier_loop_job(context: *mut ()) {
    let context = unsafe { &*context.cast::<CarrierLoopContext>() };
    let inner = unsafe { &context.control.as_ref().inner };
    let _ = inner.request_shutdown();
    let _ = unsafe { release_carrier_loop_context(context) };
}

fn retain_carrier_loop_context(context: *const CarrierLoopContext) -> Result<(), FiberError> {
    let context = unsafe { context.as_ref().ok_or_else(FiberError::invalid)? };
    let block = unsafe { context.control.as_ref() };
    block.header.try_retain().map_err(fiber_error_from_sync)
}

unsafe fn release_carrier_loop_context(
    context: *const CarrierLoopContext,
) -> Result<(), FiberError> {
    let context = unsafe { context.as_ref().ok_or_else(FiberError::invalid)? };
    let block = unsafe { context.control.as_ref() };
    let release = block.header.release().map_err(fiber_error_from_sync)?;
    if release != SharedRelease::Last {
        return Ok(());
    }
    unsafe { destroy_green_pool_block(context.control.as_ptr()) };
    Ok(())
}

/// Public alias for the carrier-backed stackful scheduler surface.
pub type FiberPool = GreenPool;
/// Public alias for one spawned fiber handle.
pub type FiberHandle<T = ()> = GreenHandle<T>;
