#[derive(Debug)]
#[cfg(feature = "std")]
pub struct HostedFiberRuntime {
    carriers: HostedCarrierRuntime,
    fibers: GreenPool,
}

#[cfg(feature = "std")]
impl Drop for HostedFiberRuntime {
    fn drop(&mut self) {
        let _ = self.fibers.shutdown();
        let _ = self.carriers.shutdown();
    }
}

/// Hosted carrier bootstrap model used to realize one hosted fiber runtime.
#[cfg(feature = "std")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HostedCarrierBootstrap {
    /// Spawn direct OS-thread carriers whose thread entry is the green carrier loop itself.
    Direct,
    /// Build one generic carrier thread pool first, then submit carrier loops into it.
    ThreadPool,
}

#[cfg(feature = "std")]
#[derive(Debug)]
pub enum HostedCarrierRuntime {
    /// Direct hosted OS-thread carriers.
    Direct(HostedDirectCarrierSet),
    /// Generic carrier thread pool used as the hosted green substrate.
    ThreadPool(ThreadPool),
}

#[cfg(feature = "std")]
impl HostedCarrierRuntime {
    /// Returns the configured carrier bootstrap model.
    #[must_use]
    pub const fn bootstrap(&self) -> HostedCarrierBootstrap {
        match self {
            Self::Direct(_) => HostedCarrierBootstrap::Direct,
            Self::ThreadPool(_) => HostedCarrierBootstrap::ThreadPool,
        }
    }

    /// Returns the active worker count.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying hosted carrier runtime cannot report its worker count
    /// honestly.
    pub fn worker_count(&self) -> Result<usize, FiberError> {
        match self {
            Self::Direct(carriers) => Ok(carriers.worker_count()),
            Self::ThreadPool(carriers) => carriers
                .worker_count()
                .map_err(fiber_error_from_thread_pool),
        }
    }

    /// Shuts the hosted carrier runtime down.
    ///
    /// # Errors
    ///
    /// Returns an error if the carrier runtime cannot complete shutdown honestly.
    pub fn shutdown(&mut self) -> Result<(), FiberError> {
        match self {
            Self::Direct(carriers) => carriers.shutdown(),
            Self::ThreadPool(carriers) => {
                let carriers = carriers.try_clone().map_err(fiber_error_from_thread_pool)?;
                carriers.shutdown().map_err(fiber_error_from_thread_pool)
            }
        }
    }

    /// Returns the carrier thread pool when this hosted runtime still uses the composed thread-pool
    /// carrier model.
    #[must_use]
    pub const fn thread_pool(&self) -> Option<&ThreadPool> {
        match self {
            Self::Direct(_) => None,
            Self::ThreadPool(carriers) => Some(carriers),
        }
    }
}

#[cfg(feature = "std")]
#[derive(Debug)]
pub struct HostedDirectCarrierSet {
    system: ThreadSystem,
    handles: std::boxed::Box<[Option<ThreadHandle>]>,
}

#[cfg(feature = "std")]
impl HostedDirectCarrierSet {
    fn new(
        runtime: HostedFiberRuntimeConfig<'_>,
        inner: &GreenPoolLease,
    ) -> Result<Self, FiberError> {
        let system = ThreadSystem::new();
        let placement = resolve_hosted_direct_placement(runtime)?;
        let mut handles = std::iter::repeat_with(|| None)
            .take(runtime.carrier_count)
            .collect::<std::vec::Vec<_>>()
            .into_boxed_slice();

        for carrier_index in 0..runtime.carrier_count {
            let context = inner
                .block()
                .metadata
                .carrier_contexts
                .ptr
                .as_ptr()
                .wrapping_add(carrier_index)
                .cast::<()>();
            retain_carrier_loop_context(context.cast_const().cast())?;
            let handle = match placement.as_ref() {
                Some(HostedDirectPlacement::LogicalCpus(cpus)) => {
                    let single = &cpus[carrier_index..=carrier_index];
                    let targets = [ThreadPlacementTarget::LogicalCpus(single)];
                    let placement = ThreadPlacementRequest {
                        targets: &targets,
                        mode: ThreadConstraintMode::Require,
                        phase: ThreadPlacementPhase::PreStartPreferred,
                        migration: ThreadMigrationPolicy::Inherit,
                    };
                    let config = ThreadConfig {
                        join_policy: ThreadJoinPolicy::Joinable,
                        name: runtime.name_prefix,
                        start_mode: ThreadStartMode::PlacementCommitted,
                        placement,
                        scheduler: fusion_sys::thread::ThreadSchedulerRequest::new(),
                        stack: fusion_sys::thread::ThreadStackRequest::new(),
                    };
                    unsafe {
                        system.spawn_raw(
                            &config,
                            run_direct_carrier_thread as RawThreadEntry,
                            context,
                        )
                    }
                }
                Some(HostedDirectPlacement::CoreClasses(classes)) => {
                    let targets = [ThreadPlacementTarget::CoreClasses(classes)];
                    let placement = ThreadPlacementRequest {
                        targets: &targets,
                        mode: ThreadConstraintMode::Prefer,
                        phase: ThreadPlacementPhase::PreStartPreferred,
                        migration: ThreadMigrationPolicy::Inherit,
                    };
                    let config = ThreadConfig {
                        join_policy: ThreadJoinPolicy::Joinable,
                        name: runtime.name_prefix,
                        start_mode: ThreadStartMode::PlacementCommitted,
                        placement,
                        scheduler: fusion_sys::thread::ThreadSchedulerRequest::new(),
                        stack: fusion_sys::thread::ThreadStackRequest::new(),
                    };
                    unsafe {
                        system.spawn_raw(
                            &config,
                            run_direct_carrier_thread as RawThreadEntry,
                            context,
                        )
                    }
                }
                None => {
                    let config = ThreadConfig {
                        join_policy: ThreadJoinPolicy::Joinable,
                        name: runtime.name_prefix,
                        start_mode: ThreadStartMode::Immediate,
                        placement: ThreadPlacementRequest::new(),
                        scheduler: fusion_sys::thread::ThreadSchedulerRequest::new(),
                        stack: fusion_sys::thread::ThreadStackRequest::new(),
                    };
                    unsafe {
                        system.spawn_raw(
                            &config,
                            run_direct_carrier_thread as RawThreadEntry,
                            context,
                        )
                    }
                }
            };
            let handle = match handle {
                Ok(handle) => handle,
                Err(error) => {
                    let _ = unsafe { release_carrier_loop_context(context.cast_const().cast()) };
                    let _ = inner.request_shutdown();
                    let mut carriers = Self { system, handles };
                    let _ = carriers.shutdown();
                    return Err(fiber_error_from_thread_pool(error));
                }
            };
            handles[carrier_index] = Some(handle);
        }

        Ok(Self { system, handles })
    }

    /// Returns the number of active hosted carrier threads.
    #[must_use]
    pub fn worker_count(&self) -> usize {
        self.handles.iter().flatten().count()
    }

    /// Shuts the direct hosted carrier set down by joining all live carrier threads.
    ///
    /// # Errors
    ///
    /// Returns the first honest thread shutdown failure, if any.
    pub fn shutdown(&mut self) -> Result<(), FiberError> {
        let mut first_error = None;
        for handle in &mut *self.handles {
            let Some(handle) = handle.take() else {
                continue;
            };
            if let Err(error) = self.system.join(handle)
                && first_error.is_none()
            {
                first_error = Some(fiber_error_from_thread_pool(error));
            }
        }
        if let Some(error) = first_error {
            return Err(error);
        }
        Ok(())
    }
}

#[cfg(feature = "std")]
impl Drop for HostedDirectCarrierSet {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

/// Hosted carrier-pool shape used to build one hosted fiber runtime.
#[cfg(feature = "std")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HostedFiberRuntimeConfig<'a> {
    /// Number of carrier workers to provision.
    pub carrier_count: usize,
    /// Hosted carrier bootstrap model.
    pub bootstrap: HostedCarrierBootstrap,
    /// Placement policy for the carrier workers.
    pub placement: PoolPlacement<'a>,
    /// Optional worker-name prefix for the carrier pool.
    pub name_prefix: Option<&'a str>,
}

#[cfg(feature = "std")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum HostedCarrierCountPolicy {
    Automatic,
    VisibleLogicalCpus,
    VisibleCores,
    VisiblePackages,
}

#[cfg(feature = "std")]
impl<'a> HostedFiberRuntimeConfig<'a> {
    /// Returns one explicit hosted runtime config with the supplied carrier count.
    #[must_use]
    pub const fn new(carrier_count: usize) -> Self {
        Self {
            carrier_count,
            bootstrap: HostedCarrierBootstrap::Direct,
            placement: PoolPlacement::Inherit,
            name_prefix: Some("fusion-fiber"),
        }
    }

    /// Returns one automatic hosted runtime config derived from visible hardware topology.
    #[must_use]
    pub fn automatic() -> Self {
        let carrier_count = automatic_carrier_count();
        Self {
            carrier_count,
            bootstrap: HostedCarrierBootstrap::Direct,
            placement: automatic_pool_placement(carrier_count),
            name_prefix: Some("fusion-fiber"),
        }
    }

    /// Returns one hosted runtime config sized to the visible logical CPU count.
    ///
    /// # Errors
    ///
    /// Returns one honest runtime error when the platform cannot truthfully report the visible
    /// logical CPU count.
    pub fn visible_logical_cpus() -> Result<Self, FiberError> {
        let summary = system_cpu()
            .topology_summary()
            .map_err(|_| FiberError::unsupported())?;
        let carrier_count = hosted_carrier_count_from_summary(
            summary,
            HostedCarrierCountPolicy::VisibleLogicalCpus,
        )
        .ok_or_else(FiberError::unsupported)?;
        Ok(Self {
            carrier_count,
            bootstrap: HostedCarrierBootstrap::Direct,
            placement: PoolPlacement::PerCore,
            name_prefix: Some("fusion-fiber"),
        })
    }

    /// Returns one hosted runtime config sized to the visible physical or topology-defined core
    /// count.
    ///
    /// This constructor only derives the carrier count from the visible core count. Hosted thread
    /// pools do not yet expose a separate physical-core affinity mode, so the default placement
    /// stays inherited until that story is truthful.
    ///
    /// # Errors
    ///
    /// Returns one honest runtime error when the platform cannot truthfully report the visible
    /// core count.
    pub fn visible_cores() -> Result<Self, FiberError> {
        let summary = system_cpu()
            .topology_summary()
            .map_err(|_| FiberError::unsupported())?;
        let carrier_count =
            hosted_carrier_count_from_summary(summary, HostedCarrierCountPolicy::VisibleCores)
                .ok_or_else(FiberError::unsupported)?;
        Ok(Self {
            carrier_count,
            bootstrap: HostedCarrierBootstrap::Direct,
            placement: PoolPlacement::Inherit,
            name_prefix: Some("fusion-fiber"),
        })
    }

    /// Returns one hosted runtime config sized to the visible package/socket count.
    ///
    /// This constructor only derives the carrier count from the visible package count. Hosted
    /// thread pools do not yet expose truthful package affinity, so placement stays inherited
    /// until the backend can actually honor package-level binding.
    ///
    /// # Errors
    ///
    /// Returns one honest runtime error when the platform cannot truthfully report the visible
    /// package count.
    pub fn visible_packages() -> Result<Self, FiberError> {
        let summary = system_cpu()
            .topology_summary()
            .map_err(|_| FiberError::unsupported())?;
        let carrier_count =
            hosted_carrier_count_from_summary(summary, HostedCarrierCountPolicy::VisiblePackages)
                .ok_or_else(FiberError::unsupported)?;
        Ok(Self {
            carrier_count,
            bootstrap: HostedCarrierBootstrap::Direct,
            placement: PoolPlacement::Inherit,
            name_prefix: Some("fusion-fiber"),
        })
    }

    /// Returns one copy of this hosted runtime config with an explicit carrier bootstrap model.
    #[must_use]
    pub const fn with_bootstrap(mut self, bootstrap: HostedCarrierBootstrap) -> Self {
        self.bootstrap = bootstrap;
        self
    }

    /// Returns one copy of this hosted runtime config with an explicit placement policy.
    #[must_use]
    pub const fn with_placement(mut self, placement: PoolPlacement<'a>) -> Self {
        self.placement = placement;
        self
    }

    /// Returns one copy of this hosted runtime config with an explicit carrier name prefix.
    #[must_use]
    pub const fn with_name_prefix(mut self, name_prefix: Option<&'a str>) -> Self {
        self.name_prefix = name_prefix;
        self
    }

    const fn to_thread_pool_config(self) -> Result<ThreadPoolConfig<'a>, FiberError> {
        if self.carrier_count == 0 {
            return Err(FiberError::invalid());
        }
        Ok(ThreadPoolConfig {
            min_threads: self.carrier_count,
            max_threads: self.carrier_count,
            placement: self.placement,
            name_prefix: self.name_prefix,
            ..ThreadPoolConfig::new()
        })
    }
}

#[cfg(feature = "std")]
#[derive(Debug, Clone, Copy)]
enum HostedDirectPlacement<'a> {
    LogicalCpus([ThreadLogicalCpuId; 32]),
    CoreClasses(&'a [fusion_sys::thread::ThreadCoreClassId]),
}

#[cfg(feature = "std")]
fn resolve_hosted_direct_placement(
    runtime: HostedFiberRuntimeConfig<'_>,
) -> Result<Option<HostedDirectPlacement<'_>>, FiberError> {
    if runtime.carrier_count == 0 {
        return Err(FiberError::invalid());
    }
    if runtime.carrier_count > 32 {
        return Err(FiberError::unsupported());
    }
    match runtime.placement {
        PoolPlacement::Inherit => Ok(None),
        PoolPlacement::Static(cpus) => {
            if cpus.len() < runtime.carrier_count {
                return Err(FiberError::invalid());
            }
            let mut resolved = [ZERO_LOGICAL_CPU; 32];
            resolved[..runtime.carrier_count].copy_from_slice(&cpus[..runtime.carrier_count]);
            Ok(Some(HostedDirectPlacement::LogicalCpus(resolved)))
        }
        PoolPlacement::PerCore => {
            let mut resolved = [ZERO_LOGICAL_CPU; 32];
            let summary = system_cpu()
                .write_logical_cpus(&mut resolved[..runtime.carrier_count])
                .map_err(|_| FiberError::unsupported())?;
            if summary.total < runtime.carrier_count {
                return Err(FiberError::resource_exhausted());
            }
            Ok(Some(HostedDirectPlacement::LogicalCpus(resolved)))
        }
        PoolPlacement::CoreClasses(classes) => {
            Ok(Some(HostedDirectPlacement::CoreClasses(classes)))
        }
        PoolPlacement::PerPackage | PoolPlacement::Dynamic => Err(FiberError::unsupported()),
    }
}

#[cfg(feature = "std")]
static AUTOMATIC_FIBER_RUNTIME: OnceLock<SyncMutex<Option<HostedFiberRuntime>>> = OnceLock::new();
static GREEN_RUNTIME_REGION_CACHE: OnceLock<
    SyncMutex<[Option<Region>; GREEN_RUNTIME_REGION_CACHE_SLOTS]>,
> = OnceLock::new();

#[cfg(feature = "std")]
impl HostedFiberRuntime {
    /// Builds one fixed-stack hosted runtime with a total requested fiber budget spread across the
    /// automatically selected carrier count.
    ///
    /// # Errors
    ///
    /// Returns one honest runtime error when the total budget is invalid or the selected runtime
    /// cannot be realized on the current platform.
    pub fn fixed(total_fibers: usize) -> Result<Self, FiberError> {
        Self::fixed_with_stack(FiberStackClass::MIN.size_bytes(), total_fibers)
    }

    /// Builds one fixed-stack hosted runtime with an explicit carrier-pool shape.
    ///
    /// # Errors
    ///
    /// Returns one honest runtime error when the total budget is invalid or the selected runtime
    /// cannot be realized on the current platform.
    pub fn fixed_with_config(
        total_fibers: usize,
        runtime: HostedFiberRuntimeConfig<'_>,
    ) -> Result<Self, FiberError> {
        Self::fixed_with_stack_and_config(FiberStackClass::MIN.size_bytes(), total_fibers, runtime)
    }

    /// Builds one fixed-stack hosted runtime with an explicit stack size and a total requested
    /// fiber budget spread across the automatically selected carrier count.
    ///
    /// # Errors
    ///
    /// Returns one honest runtime error when the total budget is invalid or the selected runtime
    /// cannot be realized on the current platform.
    pub fn fixed_with_stack(
        stack_size: NonZeroUsize,
        total_fibers: usize,
    ) -> Result<Self, FiberError> {
        Self::fixed_with_stack_and_config(
            stack_size,
            total_fibers,
            HostedFiberRuntimeConfig::automatic(),
        )
    }

    /// Builds one fixed-stack hosted runtime with an explicit stack size and carrier-pool shape.
    ///
    /// # Errors
    ///
    /// Returns one honest runtime error when the total budget is invalid or the selected runtime
    /// cannot be realized on the current platform.
    pub fn fixed_with_stack_and_config(
        stack_size: NonZeroUsize,
        total_fibers: usize,
        runtime: HostedFiberRuntimeConfig<'_>,
    ) -> Result<Self, FiberError> {
        let per_carrier = per_carrier_capacity_for_total(total_fibers, runtime.carrier_count)?;
        FiberPoolBootstrap::fixed_with_stack(stack_size, per_carrier).build_hosted_with(runtime)
    }

    /// Builds one fixed-stack hosted runtime with on-demand slot growth and a total requested
    /// fiber budget spread across the automatically selected carrier count.
    ///
    /// # Errors
    ///
    /// Returns one honest runtime error when the total budget or total growth chunk is invalid, or
    /// the selected runtime cannot be realized on the current platform.
    pub fn fixed_growing(
        total_fibers: usize,
        total_growth_chunk: usize,
    ) -> Result<Self, FiberError> {
        Self::fixed_growing_with_stack_and_config(
            FiberStackClass::MIN.size_bytes(),
            total_fibers,
            total_growth_chunk,
            HostedFiberRuntimeConfig::automatic(),
        )
    }

    /// Builds one fixed-stack hosted runtime with on-demand slot growth and an explicit carrier
    /// pool shape.
    ///
    /// # Errors
    ///
    /// Returns one honest runtime error when the total budget or total growth chunk is invalid, or
    /// the selected runtime cannot be realized on the current platform.
    pub fn fixed_growing_with_config(
        total_fibers: usize,
        total_growth_chunk: usize,
        runtime: HostedFiberRuntimeConfig<'_>,
    ) -> Result<Self, FiberError> {
        Self::fixed_growing_with_stack_and_config(
            FiberStackClass::MIN.size_bytes(),
            total_fibers,
            total_growth_chunk,
            runtime,
        )
    }

    /// Builds one fixed-stack hosted runtime with an explicit stack size, on-demand slot growth,
    /// and a total requested fiber budget spread across the automatically selected carrier count.
    ///
    /// # Errors
    ///
    /// Returns one honest runtime error when the total budget or total growth chunk is invalid, or
    /// the selected runtime cannot be realized on the current platform.
    pub fn fixed_growing_with_stack(
        stack_size: NonZeroUsize,
        total_fibers: usize,
        total_growth_chunk: usize,
    ) -> Result<Self, FiberError> {
        Self::fixed_growing_with_stack_and_config(
            stack_size,
            total_fibers,
            total_growth_chunk,
            HostedFiberRuntimeConfig::automatic(),
        )
    }

    /// Builds one fixed-stack hosted runtime with an explicit stack size, on-demand slot growth,
    /// and an explicit carrier-pool shape.
    ///
    /// # Errors
    ///
    /// Returns one honest runtime error when the total budget or total growth chunk is invalid, or
    /// the selected runtime cannot be realized on the current platform.
    pub fn fixed_growing_with_stack_and_config(
        stack_size: NonZeroUsize,
        total_fibers: usize,
        total_growth_chunk: usize,
        runtime: HostedFiberRuntimeConfig<'_>,
    ) -> Result<Self, FiberError> {
        let per_carrier = per_carrier_capacity_for_total(total_fibers, runtime.carrier_count)?;
        let per_carrier_growth =
            per_carrier_capacity_for_total(total_growth_chunk, runtime.carrier_count)?;
        FiberPoolBootstrap::fixed_growing_with_stack(stack_size, per_carrier, per_carrier_growth)?
            .build_hosted_with(runtime)
    }

    /// Builds one hosted-default runtime with a total requested fiber budget spread across the
    /// automatically selected carrier count.
    ///
    /// # Errors
    ///
    /// Returns one honest runtime error when the total budget is invalid or the selected runtime
    /// cannot be realized on the current platform.
    pub fn hosted_default(total_fibers: usize) -> Result<Self, FiberError> {
        Self::hosted_default_with_config(total_fibers, HostedFiberRuntimeConfig::automatic())
    }

    /// Builds one hosted-default runtime with an explicit carrier-pool shape.
    ///
    /// # Errors
    ///
    /// Returns one honest runtime error when the total budget is invalid or the selected runtime
    /// cannot be realized on the current platform.
    pub fn hosted_default_with_config(
        total_fibers: usize,
        runtime: HostedFiberRuntimeConfig<'_>,
    ) -> Result<Self, FiberError> {
        let per_carrier = per_carrier_capacity_for_total(total_fibers, runtime.carrier_count)?;
        FiberPoolBootstrap::hosted_default(per_carrier).build_hosted_with(runtime)
    }

    /// Builds one class-backed hosted runtime from total per-class budgets spread across the
    /// automatically selected carrier count.
    ///
    /// # Errors
    ///
    /// Returns one honest runtime error when the supplied class table is invalid or the selected
    /// runtime cannot be realized on the current platform.
    pub fn classed(classes: &[HostedFiberClassConfig]) -> Result<Self, FiberError> {
        Self::classed_with_config(classes, HostedFiberRuntimeConfig::automatic())
    }

    /// Builds one class-backed hosted runtime from total per-class budgets and an explicit
    /// carrier-pool shape.
    ///
    /// # Errors
    ///
    /// Returns one honest runtime error when the supplied class table is invalid or the selected
    /// runtime cannot be realized on the current platform.
    pub fn classed_with_config(
        classes: &[HostedFiberClassConfig],
        runtime: HostedFiberRuntimeConfig<'_>,
    ) -> Result<Self, FiberError> {
        let distributed = distribute_hosted_class_configs(classes, runtime.carrier_count)?;
        FiberPoolBootstrap::classed(distributed.as_slice())?.build_hosted_with(runtime)
    }

    /// Builds one hosted carrier-backed runtime from an explicit bootstrap surface.
    ///
    /// # Errors
    ///
    /// Returns one honest runtime error when the selected runtime cannot be realized on the
    /// current platform.
    pub fn from_bootstrap(bootstrap: FiberPoolBootstrap<'_>) -> Result<Self, FiberError> {
        Self::from_bootstrap_with(bootstrap, HostedFiberRuntimeConfig::automatic())
    }

    /// Builds one hosted carrier-backed runtime from an explicit bootstrap surface and carrier
    /// configuration.
    ///
    /// # Errors
    ///
    /// Returns one honest runtime error when the selected runtime cannot be realized on the
    /// current platform.
    pub fn from_bootstrap_with(
        bootstrap: FiberPoolBootstrap<'_>,
        runtime: HostedFiberRuntimeConfig<'_>,
    ) -> Result<Self, FiberError> {
        match runtime.bootstrap {
            HostedCarrierBootstrap::Direct => {
                let (fibers, carriers) =
                    GreenPool::build_hosted_direct(bootstrap.config(), runtime)?;
                Ok(Self {
                    carriers: HostedCarrierRuntime::Direct(carriers),
                    fibers,
                })
            }
            HostedCarrierBootstrap::ThreadPool => {
                let carrier_config = runtime.to_thread_pool_config()?;
                let carriers =
                    ThreadPool::new(&carrier_config).map_err(fiber_error_from_thread_pool)?;
                let fibers = GreenPool::new(bootstrap.config(), &carriers)?;
                Ok(Self {
                    carriers: HostedCarrierRuntime::ThreadPool(carriers),
                    fibers,
                })
            }
        }
    }

    /// Returns the owned hosted carrier runtime backing this hosted fiber runtime.
    #[must_use]
    pub const fn carriers(&self) -> &HostedCarrierRuntime {
        &self.carriers
    }

    /// Returns the carrier-backed green-fiber pool exposed by this hosted runtime.
    #[must_use]
    pub const fn fibers(&self) -> &GreenPool {
        &self.fibers
    }

    /// Releases the owned carrier pool and green-fiber pool back to the caller.
    #[must_use]
    pub fn into_parts(self) -> (HostedCarrierRuntime, GreenPool) {
        let this = ManuallyDrop::new(self);
        // SAFETY: `this` will not run `Drop`; we move both owned fields out exactly once.
        unsafe { (ptr::read(&this.carriers), ptr::read(&this.fibers)) }
    }
}

fn build_hosted_green_inner(
    config: &FiberPoolConfig<'_>,
    carrier_workers: usize,
) -> Result<GreenPoolLease, FiberError> {
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
    let alignment = support.context.min_stack_alignment.max(16);
    let stacks = FiberStackStore::new(config, alignment, support.context.stack_direction)?;
    let reactor_enabled = EventSystem::new()
        .support()
        .caps
        .contains(EventCaps::READINESS)
        && system_fiber_host().support().wake_signal
        && matches!(config.reactor_policy, GreenReactorPolicy::Automatic);
    let task_capacity = stacks.total_capacity();
    let (runtime_region, metadata_region) = green_pool_runtime_regions(
        carrier_workers,
        task_capacity,
        config.scheduling,
        reactor_enabled,
        config.sizing,
    )?;
    let (pool_metadata, tasks, carriers) = match GreenPoolMetadata::new_in_region(
        metadata_region,
        carrier_workers,
        task_capacity,
        config.scheduling,
        config.priority_age_cap,
        reactor_enabled,
        false,
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
    Ok(inner)
}

fn launch_thread_pool_green_carriers(
    inner: &GreenPoolLease,
    carrier: &ThreadPool,
) -> Result<(), FiberError> {
    for carrier_index in 0..inner.carriers.len() {
        let context = inner
            .block()
            .metadata
            .carrier_contexts
            .ptr
            .as_ptr()
            .wrapping_add(carrier_index)
            .cast::<()>();
        retain_carrier_loop_context(context.cast_const().cast())?;
        let work =
            SystemWorkItem::with_cancel(run_carrier_loop_job, context, cancel_carrier_loop_job);
        if let Err(error) = carrier
            .submit_raw(work)
            .map_err(fiber_error_from_thread_pool)
        {
            let _ = unsafe { release_carrier_loop_context(context.cast_const().cast()) };
            let _ = inner.request_shutdown();
            return Err(error);
        }
    }
    Ok(())
}
