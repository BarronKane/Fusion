/// Public fiber-pool configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FiberPoolConfig<'a> {
    /// Stack backing and growth model.
    pub stack_backing: FiberStackBacking,
    /// Sizing strategy applied to derived stack and backing envelopes.
    pub sizing: RuntimeSizingStrategy,
    /// Optional explicit stack-class provisioning table.
    pub classes: &'a [FiberStackClassConfig],
    /// Hardware guard pages per fiber.
    pub guard_pages: usize,
    /// Legacy number of reservations committed together when one slab-backed pool grows.
    ///
    /// Class-backed pools carry per-class growth chunks in [`FiberStackClassConfig`] instead of
    /// consulting this field.
    pub growth_chunk: usize,
    /// Legacy maximum live fibers admitted per carrier worker.
    ///
    /// When `classes` is non-empty, the effective per-carrier capacity is derived from the class
    /// table instead.
    pub max_fibers_per_carrier: usize,
    /// Scheduling policy across carriers.
    pub scheduling: GreenScheduling,
    /// Locality preference when admitting new work from an already-running carrier.
    pub spawn_locality_policy: CarrierSpawnLocalityPolicy,
    /// Optional cap on virtual waiting-age promotion for strict-priority scheduling.
    pub priority_age_cap: Option<FiberTaskAgeCap>,
    /// Pool population growth policy.
    pub growth: GreenGrowth,
    /// Signal-path stack telemetry policy.
    pub telemetry: FiberTelemetry,
    /// Action to take when an elastic stack reaches capacity.
    pub capacity_policy: CapacityPolicy,
    /// Action to take when one cooperative fiber exceeds its declared run-between-yield budget.
    pub yield_budget_policy: FiberYieldBudgetPolicy,
    /// Whether hosted carrier queues provision readiness/reactor machinery.
    pub reactor_policy: GreenReactorPolicy,
    /// Huge-page preference for large reservations.
    pub huge_pages: HugePagePolicy,
    /// Optional owning courier identity carried by the live runtime for self-query surfaces.
    pub courier_id: Option<CourierId>,
    /// Optional owning context identity carried by the live runtime for self-query surfaces.
    pub context_id: Option<ContextId>,
    /// Optional courier-runtime sink used to publish authoritative lifecycle/runtime truth.
    pub runtime_sink: Option<CourierRuntimeSink>,
    /// Optional launch-control surface used to realize parent/child courier truth at root-fiber
    /// admission time.
    pub launch_control: Option<CourierLaunchControl<'static>>,
    /// Optional child-courier launch request realized on first root-fiber admission.
    pub launch_request: Option<CourierChildLaunchRequest<'static>>,
}

/// Whether one green-fiber pool provisions hosted readiness/reactor machinery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GreenReactorPolicy {
    /// Enable readiness/reactor support when the backend can honestly provide it.
    Automatic,
    /// Do not provision hosted readiness/reactor support; readiness parking will fail honestly.
    Disabled,
}

impl FiberPoolConfig<'static> {
    /// Returns an automatic hosted default.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            stack_backing: FiberStackBacking::Elastic {
                initial_size: non_zero_usize(4 * 1024),
                max_size: non_zero_usize(256 * 1024),
            },
            sizing: default_runtime_sizing_strategy(),
            classes: &[],
            guard_pages: 1,
            growth_chunk: 32,
            max_fibers_per_carrier: 64,
            scheduling: GreenScheduling::Fifo,
            spawn_locality_policy: CarrierSpawnLocalityPolicy::SameCore,
            priority_age_cap: None,
            growth: GreenGrowth::OnDemand,
            telemetry: FiberTelemetry::Disabled,
            capacity_policy: CapacityPolicy::Abort,
            yield_budget_policy: FiberYieldBudgetPolicy::Abort,
            reactor_policy: GreenReactorPolicy::Automatic,
            huge_pages: HugePagePolicy::Disabled,
            courier_id: None,
            context_id: None,
            runtime_sink: None,
            launch_control: None,
            launch_request: None,
        }
    }

    /// Returns an explicit fixed-capacity deterministic configuration.
    #[must_use]
    pub const fn fixed(stack_size: NonZeroUsize, max_fibers_per_carrier: usize) -> Self {
        Self {
            stack_backing: FiberStackBacking::Fixed { stack_size },
            sizing: default_runtime_sizing_strategy(),
            classes: &[],
            guard_pages: 1,
            growth_chunk: max_fibers_per_carrier,
            max_fibers_per_carrier,
            scheduling: GreenScheduling::Fifo,
            spawn_locality_policy: CarrierSpawnLocalityPolicy::SameCore,
            priority_age_cap: None,
            growth: GreenGrowth::Fixed,
            telemetry: FiberTelemetry::Disabled,
            capacity_policy: CapacityPolicy::Abort,
            yield_budget_policy: FiberYieldBudgetPolicy::Abort,
            reactor_policy: GreenReactorPolicy::Automatic,
            huge_pages: HugePagePolicy::Disabled,
            courier_id: None,
            context_id: None,
            runtime_sink: None,
            launch_control: None,
            launch_request: None,
        }
    }

    /// Returns one deterministic fixed-stack configuration that commits slots on demand.
    ///
    /// This keeps one fixed stack envelope per task while avoiding full up-front slot
    /// initialization for every admitted capacity slot.
    ///
    /// # Errors
    ///
    /// Returns an error when `max_fibers_per_carrier` is zero, `growth_chunk` is zero, or
    /// `growth_chunk` exceeds `max_fibers_per_carrier`.
    pub const fn fixed_growing(
        stack_size: NonZeroUsize,
        max_fibers_per_carrier: usize,
        growth_chunk: usize,
    ) -> Result<Self, FiberError> {
        if max_fibers_per_carrier == 0 || growth_chunk == 0 || growth_chunk > max_fibers_per_carrier
        {
            return Err(FiberError::invalid());
        }
        Ok(Self {
            stack_backing: FiberStackBacking::Fixed { stack_size },
            sizing: default_runtime_sizing_strategy(),
            classes: &[],
            guard_pages: 1,
            growth_chunk,
            max_fibers_per_carrier,
            scheduling: GreenScheduling::Fifo,
            spawn_locality_policy: CarrierSpawnLocalityPolicy::SameCore,
            priority_age_cap: None,
            growth: GreenGrowth::OnDemand,
            telemetry: FiberTelemetry::Disabled,
            capacity_policy: CapacityPolicy::Abort,
            yield_budget_policy: FiberYieldBudgetPolicy::Abort,
            reactor_policy: GreenReactorPolicy::Automatic,
            huge_pages: HugePagePolicy::Disabled,
            courier_id: None,
            context_id: None,
            runtime_sink: None,
            launch_control: None,
            launch_request: None,
        })
    }
}

const fn non_zero_usize(value: usize) -> NonZeroUsize {
    match NonZeroUsize::new(value) {
        Some(value) => value,
        None => panic!("fiber stack sizes must be non-zero"),
    }
}

impl<'a> FiberPoolConfig<'a> {
    const fn validate_class_layout(
        classes: &'a [FiberStackClassConfig],
    ) -> Result<(FiberStackClass, usize), FiberError> {
        if classes.is_empty() {
            return Err(FiberError::invalid());
        }

        let mut total = 0usize;
        let mut previous: Option<FiberStackClass> = None;
        let mut largest: Option<FiberStackClass> = None;
        let mut index = 0usize;
        while index < classes.len() {
            let class = match classes[index].validate() {
                Ok(class) => class,
                Err(error) => return Err(error),
            };
            if let Some(previous_class) = previous
                && previous_class.size_bytes().get() >= class.class.size_bytes().get()
            {
                return Err(FiberError::invalid());
            }
            let Some(next_total) = total.checked_add(class.slots_per_carrier) else {
                return Err(FiberError::resource_exhausted());
            };
            total = next_total;
            previous = Some(class.class);
            largest = Some(class.class);
            index += 1;
        }

        match largest {
            Some(largest_class) => Ok((largest_class, total)),
            None => Err(FiberError::invalid()),
        }
    }

    /// Returns one checked class-first configuration.
    ///
    /// The effective per-carrier capacity is derived from the class table, and the legacy fixed
    /// backing fields are normalized to the largest configured class so the public shape stays
    /// honest.
    ///
    /// # Errors
    ///
    /// Returns an error when the class table is empty, unsorted, contains zero-slot classes, or
    /// overflows capacity accounting.
    pub const fn classed(classes: &'a [FiberStackClassConfig]) -> Result<Self, FiberError> {
        let (largest, total_capacity) = match Self::validate_class_layout(classes) {
            Ok(layout) => layout,
            Err(error) => return Err(error),
        };
        Ok(Self {
            stack_backing: FiberStackBacking::Fixed {
                stack_size: largest.size_bytes(),
            },
            sizing: default_runtime_sizing_strategy(),
            classes,
            guard_pages: 1,
            growth_chunk: total_capacity,
            max_fibers_per_carrier: total_capacity,
            scheduling: GreenScheduling::Fifo,
            spawn_locality_policy: CarrierSpawnLocalityPolicy::SameCore,
            priority_age_cap: None,
            growth: GreenGrowth::Fixed,
            telemetry: FiberTelemetry::Disabled,
            capacity_policy: CapacityPolicy::Abort,
            yield_budget_policy: FiberYieldBudgetPolicy::Abort,
            reactor_policy: GreenReactorPolicy::Automatic,
            huge_pages: HugePagePolicy::Disabled,
            courier_id: None,
            context_id: None,
            runtime_sink: None,
            launch_control: None,
            launch_request: None,
        })
    }

    /// Returns one copy of this configuration with explicit class-based provisioning.
    ///
    /// This is a low-level override that does not normalize the legacy single-slab fields or
    /// validate per-class growth semantics. Prefer [`FiberPoolConfig::classed`] for public
    /// class-first configuration.
    #[must_use]
    pub const fn with_classes(mut self, classes: &'a [FiberStackClassConfig]) -> Self {
        self.classes = classes;
        self
    }

    /// Returns one copy of this configuration with an explicit sizing strategy.
    #[must_use]
    pub const fn with_sizing_strategy(mut self, sizing: RuntimeSizingStrategy) -> Self {
        self.sizing = sizing;
        self
    }

    /// Returns one copy of this configuration with an explicit guard-page count.
    #[must_use]
    pub const fn with_guard_pages(mut self, guard_pages: usize) -> Self {
        self.guard_pages = guard_pages;
        self
    }

    /// Returns one copy of this configuration with an explicit scheduling policy.
    #[must_use]
    pub const fn with_scheduling(mut self, scheduling: GreenScheduling) -> Self {
        self.scheduling = scheduling;
        self
    }

    /// Returns one copy of this configuration with an explicit carrier spawn-locality policy.
    #[must_use]
    pub const fn with_spawn_locality_policy(
        mut self,
        spawn_locality_policy: CarrierSpawnLocalityPolicy,
    ) -> Self {
        self.spawn_locality_policy = spawn_locality_policy;
        self
    }

    /// Returns one copy configured for first-come-first-served multicarrier execution with
    /// sticky locality and idle-carrier stealing.
    ///
    /// This is the small-SMP default Fusion should want first: run fibers in admission order on
    /// whichever carrier dequeued them, prefer keeping follow-on spawns local, and let idle
    /// carriers steal when that helps more than purity theater.
    #[must_use]
    pub const fn with_fcfs_steal_locality(
        mut self,
        spawn_locality_policy: CarrierSpawnLocalityPolicy,
    ) -> Self {
        self.scheduling = GreenScheduling::WorkStealing;
        self.spawn_locality_policy = spawn_locality_policy;
        self
    }

    /// Returns one copy of this configuration with an explicit strict-priority virtual age cap.
    #[must_use]
    pub const fn with_priority_age_cap(mut self, priority_age_cap: FiberTaskAgeCap) -> Self {
        self.priority_age_cap = Some(priority_age_cap);
        self
    }

    /// Returns one copy of this configuration with an explicit growth policy.
    #[must_use]
    pub const fn with_growth(mut self, growth: GreenGrowth) -> Self {
        self.growth = growth;
        self
    }

    /// Returns one copy of this configuration with an explicit telemetry policy.
    #[must_use]
    pub const fn with_telemetry(mut self, telemetry: FiberTelemetry) -> Self {
        self.telemetry = telemetry;
        self
    }

    /// Returns one copy of this configuration with an explicit capacity-response policy.
    #[must_use]
    pub const fn with_capacity_policy(mut self, capacity_policy: CapacityPolicy) -> Self {
        self.capacity_policy = capacity_policy;
        self
    }

    /// Returns one copy of this configuration with an explicit run-between-yield overrun policy.
    #[must_use]
    pub const fn with_yield_budget_policy(
        mut self,
        yield_budget_policy: FiberYieldBudgetPolicy,
    ) -> Self {
        self.yield_budget_policy = yield_budget_policy;
        self
    }

    /// Returns one copy of this configuration with explicit hosted readiness/reactor policy.
    #[must_use]
    pub const fn with_reactor_policy(mut self, reactor_policy: GreenReactorPolicy) -> Self {
        self.reactor_policy = reactor_policy;
        self
    }

    /// Returns one copy of this configuration with an explicit huge-page preference.
    #[must_use]
    pub const fn with_huge_pages(mut self, huge_pages: HugePagePolicy) -> Self {
        self.huge_pages = huge_pages;
        self
    }

    /// Returns one copy of this configuration with an explicit owning courier identity.
    #[must_use]
    pub const fn with_courier_id(mut self, courier_id: CourierId) -> Self {
        self.courier_id = Some(courier_id);
        self
    }

    /// Returns one copy of this configuration with an explicit owning context identity.
    #[must_use]
    pub const fn with_context_id(mut self, context_id: ContextId) -> Self {
        self.context_id = Some(context_id);
        self
    }

    /// Returns one copy of this configuration with one explicit runtime-to-courier sink.
    #[must_use]
    pub const fn with_runtime_sink(mut self, runtime_sink: CourierRuntimeSink) -> Self {
        self.runtime_sink = Some(runtime_sink);
        self
    }

    /// Returns one copy of this configuration with one explicit child-courier launch-control
    /// surface.
    #[must_use]
    pub const fn with_launch_control(
        mut self,
        launch_control: CourierLaunchControl<'static>,
    ) -> Self {
        self.launch_control = Some(launch_control);
        self
    }

    /// Returns one copy of this configuration with one explicit child-courier launch request.
    #[must_use]
    pub const fn with_child_launch(
        mut self,
        launch_request: CourierChildLaunchRequest<'static>,
    ) -> Self {
        self.launch_request = Some(launch_request);
        self
    }

    /// Returns one copy of this configuration with an explicit legacy chunk size.
    ///
    /// This only affects legacy single-envelope pools. Class-backed pools carry their own
    /// `growth_chunk` in [`FiberStackClassConfig`].
    #[must_use]
    pub const fn with_legacy_growth_chunk(mut self, growth_chunk: usize) -> Self {
        self.growth_chunk = growth_chunk;
        self
    }

    /// Returns one copy of this configuration with an explicit legacy capacity.
    ///
    /// This only affects legacy single-envelope pools. Class-backed pools derive total capacity
    /// from their class table.
    #[must_use]
    pub const fn with_legacy_capacity(mut self, max_fibers_per_carrier: usize) -> Self {
        self.max_fibers_per_carrier = max_fibers_per_carrier;
        self
    }

    /// Returns whether this configuration uses explicit stack classes.
    #[must_use]
    pub const fn uses_classes(&self) -> bool {
        !self.classes.is_empty()
    }

    /// Returns whether this configuration still relies on the legacy single-envelope capacity
    /// model instead of explicit class provisioning.
    #[must_use]
    pub const fn uses_legacy_capacity_model(&self) -> bool {
        self.classes.is_empty()
    }

    /// Returns the effective per-carrier task capacity.
    ///
    /// # Errors
    ///
    /// Returns an error when the class table overflows capacity accounting.
    pub const fn task_capacity_per_carrier(&self) -> Result<usize, FiberError> {
        if self.classes.is_empty() {
            return Ok(self.max_fibers_per_carrier);
        }

        let mut total = 0usize;
        let mut index = 0usize;
        while index < self.classes.len() {
            let Some(next_total) = total.checked_add(self.classes[index].slots_per_carrier) else {
                return Err(FiberError::resource_exhausted());
            };
            total = next_total;
            index += 1;
        }
        Ok(total)
    }

    const fn max_stack_bytes(&self) -> usize {
        match self.stack_backing {
            FiberStackBacking::Fixed { stack_size } => stack_size.get(),
            FiberStackBacking::Elastic { max_size, .. } => max_size.get(),
        }
    }

    /// Returns whether this configuration can honestly admit the requested task class.
    #[must_use]
    pub const fn supports_task_class(&self, class: FiberStackClass) -> bool {
        if self.classes.is_empty() {
            return class.size_bytes().get() <= self.max_stack_bytes();
        }

        let mut index = 0usize;
        while index < self.classes.len() {
            if self.classes[index].class.size_bytes().get() >= class.size_bytes().get() {
                return true;
            }
            index += 1;
        }
        false
    }

    /// Returns whether this configuration can honestly admit the requested task attributes.
    #[must_use]
    pub const fn supports_task_attributes(&self, task: FiberTaskAttributes) -> bool {
        !task.execution.requires_fiber() || self.supports_task_class(task.stack_class)
    }

    /// Validates one explicit task-attribute bundle against this pool configuration.
    ///
    /// # Errors
    ///
    /// Returns an error when the requested task class is not provisioned by this configuration.
    pub const fn validate_task_attributes(
        &self,
        task: FiberTaskAttributes,
    ) -> Result<(), FiberError> {
        if !task.execution.requires_fiber() && task.yield_budget.is_some() {
            return Err(FiberError::unsupported());
        }
        if !self.supports_task_attributes(task) {
            return Err(FiberError::unsupported());
        }
        Ok(())
    }

    /// Asserts in const/static contexts that one raw task-attribute bundle is supported by this
    /// configuration.
    ///
    /// # Panics
    ///
    /// Panics during const evaluation when the requested task class is not provisioned by this
    /// configuration.
    pub const fn assert_task_attributes_supported(&self, task: FiberTaskAttributes) {
        assert!(
            self.validate_task_attributes(task).is_ok(),
            "fiber task attributes are not supported by this pool configuration",
        );
    }

    /// Validates one compile-time explicit fiber task against this configuration.
    ///
    /// # Errors
    ///
    /// Returns an error when the task's declared contract is invalid or not provisioned by this
    /// configuration.
    pub const fn validate_explicit_task<T: ExplicitFiberTask>(&self) -> Result<(), FiberError> {
        self.validate_task_attributes(T::ATTRIBUTES)
    }

    /// Asserts in const/static contexts that one explicit task is supported by this configuration.
    ///
    /// # Panics
    ///
    /// Panics during const evaluation when the task's declared stack class is not provisioned by
    /// this configuration.
    pub const fn assert_explicit_task_supported<T: ExplicitFiberTask>(&self) {
        assert!(
            self.validate_explicit_task::<T>().is_ok(),
            "explicit fiber task is not supported by this pool configuration",
        );
    }

    /// Validates one build-generated compile-time task contract against this configuration.
    ///
    /// # Errors
    ///
    /// Returns an error when the generated task's declared stack class is not provisioned by this
    /// configuration.
    pub const fn validate_generated_task_contract<T: GeneratedExplicitFiberTaskContract>(
        &self,
    ) -> Result<(), FiberError> {
        self.validate_task_attributes(T::ATTRIBUTES)
    }

    /// Asserts in const/static contexts that one build-generated task contract is supported by
    /// this configuration.
    ///
    /// # Panics
    ///
    /// Panics during const evaluation when the generated task's declared stack class is not
    /// provisioned by this configuration.
    pub const fn assert_generated_task_supported<T: GeneratedExplicitFiberTaskContract>(&self) {
        assert!(
            self.validate_generated_task_contract::<T>().is_ok(),
            "generated fiber task is not supported by this pool configuration",
        );
    }

    /// Validates one build-generated explicit fiber task against this configuration.
    ///
    /// # Errors
    ///
    /// Returns an error when generated metadata is missing or invalid for the task, or when the
    /// resulting stack class is not provisioned by this configuration.
    #[cfg(not(feature = "critical-safe"))]
    pub fn validate_generated_task<T: GeneratedExplicitFiberTask>(&self) -> Result<(), FiberError> {
        self.validate_task_attributes(T::task_attributes()?)
    }

    /// Validates one build-generated explicit fiber task against this configuration using its
    /// compile-time generated contract directly.
    ///
    /// This is the cross-crate contract-first path for ordinary builds that want compile-time
    /// generated contracts without depending on runtime metadata lookup.
    ///
    /// # Errors
    ///
    /// Returns an error when the generated contract is not provisioned by this configuration.
    pub const fn validate_generated_task_contract_path<T>(&self) -> Result<(), FiberError>
    where
        T: GeneratedExplicitFiberTask + GeneratedExplicitFiberTaskContract,
    {
        self.validate_generated_task_contract::<T>()
    }

    /// Validates one build-generated explicit fiber task against this configuration.
    ///
    /// In strict generated-contract builds, admission must come from a compile-time generated
    /// contract instead of the runtime metadata lookup table.
    ///
    /// # Errors
    ///
    /// Returns an error when the generated contract is not provisioned by this configuration.
    #[cfg(feature = "critical-safe")]
    pub const fn validate_generated_task<T>(&self) -> Result<(), FiberError>
    where
        T: GeneratedExplicitFiberTask + GeneratedExplicitFiberTaskContract,
    {
        self.validate_generated_task_contract::<T>()
    }
}

impl Default for FiberPoolConfig<'static> {
    fn default() -> Self {
        Self::new()
    }
}

/// Ergonomic fiber-pool bootstrap surface above raw `FiberPoolConfig`.
#[derive(Debug, Clone, Copy)]
pub struct FiberPoolBootstrap<'a> {
    config: FiberPoolConfig<'a>,
}

impl FiberPoolBootstrap<'static> {
    /// Returns one deterministic fixed-stack bootstrap using the largest generated fiber-task
    /// contract visible to the current crate.
    ///
    /// # Errors
    ///
    /// Returns an error when no generated stack metadata is available.
    pub fn auto(max_fibers: usize) -> Result<Self, FiberError> {
        Ok(Self::uniform(
            max_fibers,
            generated_default_fiber_stack_size()?,
        ))
    }

    /// Returns one deterministic fixed-stack bootstrap using the largest generated fiber-task
    /// contract visible to the current crate and one explicit growth chunk.
    ///
    /// # Errors
    ///
    /// Returns an error when generated stack metadata is unavailable or the requested growth
    /// chunk is invalid.
    pub fn auto_growing(max_fibers: usize, growth_chunk: usize) -> Result<Self, FiberError> {
        Self::uniform_growing(
            generated_default_fiber_stack_size()?,
            max_fibers,
            growth_chunk,
        )
    }

    /// Returns one deterministic fixed-stack bootstrap with one explicit uniform stack size.
    #[must_use]
    pub const fn uniform(max_fibers: usize, stack_size: NonZeroUsize) -> Self {
        Self::fixed_with_stack(stack_size, max_fibers)
    }

    /// Returns one deterministic fixed-stack bootstrap with one explicit uniform stack size and
    /// one explicit growth chunk.
    ///
    /// # Errors
    ///
    /// Returns an error when the requested growth chunk is invalid for the requested capacity.
    pub const fn uniform_growing(
        stack_size: NonZeroUsize,
        max_fibers: usize,
        growth_chunk: usize,
    ) -> Result<Self, FiberError> {
        Self::fixed_growing_with_stack(stack_size, max_fibers, growth_chunk)
    }

    /// Returns one deterministic fixed-stack bootstrap with the minimum supported stack size.
    #[must_use]
    pub const fn fixed(max_fibers: usize) -> Self {
        Self::fixed_with_stack(FiberStackClass::MIN.size_bytes(), max_fibers)
    }

    /// Returns one deterministic fixed-stack bootstrap with an explicit stack size.
    #[must_use]
    pub const fn fixed_with_stack(stack_size: NonZeroUsize, max_fibers: usize) -> Self {
        Self {
            config: FiberPoolConfig::fixed(stack_size, max_fibers),
        }
    }

    /// Returns one deterministic fixed-stack bootstrap with on-demand slot growth.
    ///
    /// # Errors
    ///
    /// Returns an error when the requested growth chunk is invalid for the requested capacity.
    pub const fn fixed_growing(max_fibers: usize, growth_chunk: usize) -> Result<Self, FiberError> {
        Self::fixed_growing_with_stack(FiberStackClass::MIN.size_bytes(), max_fibers, growth_chunk)
    }

    /// Returns one deterministic fixed-stack bootstrap with an explicit stack size and on-demand
    /// slot growth.
    ///
    /// # Errors
    ///
    /// Returns an error when the requested growth chunk is invalid for the requested capacity.
    pub const fn fixed_growing_with_stack(
        stack_size: NonZeroUsize,
        max_fibers: usize,
        growth_chunk: usize,
    ) -> Result<Self, FiberError> {
        match FiberPoolConfig::fixed_growing(stack_size, max_fibers, growth_chunk) {
            Ok(config) => Ok(Self { config }),
            Err(error) => Err(error),
        }
    }

    /// Returns one hosted-default bootstrap with explicit task capacity.
    #[must_use]
    pub const fn hosted_default(max_fibers: usize) -> Self {
        Self {
            config: FiberPoolConfig::new()
                .with_legacy_growth_chunk(max_fibers)
                .with_legacy_capacity(max_fibers),
        }
    }
}

impl<'a> FiberPoolBootstrap<'a> {
    /// Returns one class-backed bootstrap surface.
    ///
    /// # Errors
    ///
    /// Returns an error when the supplied class table is invalid.
    pub const fn classed(classes: &'a [FiberStackClassConfig]) -> Result<Self, FiberError> {
        match FiberPoolConfig::classed(classes) {
            Ok(config) => Ok(Self { config }),
            Err(error) => Err(error),
        }
    }

    /// Returns one bootstrap surface from an already-built low-level config.
    #[must_use]
    pub const fn from_config(config: FiberPoolConfig<'a>) -> Self {
        Self { config }
    }

    /// Returns one copy of this bootstrap with explicit telemetry.
    #[must_use]
    pub const fn with_telemetry(mut self, telemetry: FiberTelemetry) -> Self {
        self.config = self.config.with_telemetry(telemetry);
        self
    }

    /// Returns one copy of this bootstrap with explicit guard-page count.
    #[must_use]
    pub const fn with_guard_pages(mut self, guard_pages: usize) -> Self {
        self.config = self.config.with_guard_pages(guard_pages);
        self
    }

    /// Returns one copy of this bootstrap with explicit sizing strategy.
    #[must_use]
    pub const fn with_sizing_strategy(mut self, sizing: RuntimeSizingStrategy) -> Self {
        self.config = self.config.with_sizing_strategy(sizing);
        self
    }

    /// Returns one copy of this bootstrap with explicit scheduling.
    #[must_use]
    pub const fn with_scheduling(mut self, scheduling: GreenScheduling) -> Self {
        self.config = self.config.with_scheduling(scheduling);
        self
    }

    /// Returns one copy of this bootstrap with an explicit owning courier identity.
    #[must_use]
    pub const fn with_courier_id(mut self, courier_id: CourierId) -> Self {
        self.config = self.config.with_courier_id(courier_id);
        self
    }

    /// Returns one copy of this bootstrap with an explicit owning context identity.
    #[must_use]
    pub const fn with_context_id(mut self, context_id: ContextId) -> Self {
        self.config = self.config.with_context_id(context_id);
        self
    }

    /// Returns one copy of this bootstrap with one explicit runtime-to-courier sink.
    #[must_use]
    pub const fn with_runtime_sink(mut self, runtime_sink: CourierRuntimeSink) -> Self {
        self.config = self.config.with_runtime_sink(runtime_sink);
        self
    }

    /// Returns one copy of this bootstrap with one explicit child-courier launch-control
    /// surface.
    #[must_use]
    pub const fn with_launch_control(
        mut self,
        launch_control: CourierLaunchControl<'static>,
    ) -> Self {
        self.config = self.config.with_launch_control(launch_control);
        self
    }

    /// Returns one copy of this bootstrap with one explicit child-courier launch request.
    #[must_use]
    pub const fn with_child_launch(
        mut self,
        launch_request: CourierChildLaunchRequest<'static>,
    ) -> Self {
        self.config = self.config.with_child_launch(launch_request);
        self
    }

    /// Returns the underlying low-level configuration.
    #[must_use]
    pub const fn config(&self) -> &FiberPoolConfig<'a> {
        &self.config
    }

    /// Builds one manually-driven current-thread pool from this bootstrap.
    ///
    /// # Errors
    ///
    /// Returns one honest runtime error when the selected backend cannot realize the pool.
    pub fn build_current(self) -> Result<CurrentFiberPool, FiberError> {
        CurrentFiberPool::new(&self.config)
    }

    /// Builds one current-thread fiber pool from one caller-owned bound slab.
    ///
    /// # Errors
    ///
    /// Returns any honest sizing, partitioning, or bootstrap failure.
    pub fn from_bound_slab(
        self,
        slab: MemoryResourceHandle,
    ) -> Result<CurrentFiberPool, FiberError> {
        CurrentFiberPool::from_bound_slab(&self.config, slab)
    }

    /// Builds one current-thread fiber pool from one caller-owned static byte slab.
    ///
    /// # Safety
    ///
    /// The caller must guarantee the supplied pointer/length pair names one valid writable static
    /// extent for the whole lifetime of the pool.
    ///
    /// # Errors
    ///
    /// Returns any honest binding, sizing, partitioning, or bootstrap failure.
    pub unsafe fn from_static_slab(
        self,
        ptr: *mut u8,
        len: usize,
    ) -> Result<CurrentFiberPool, FiberError> {
        unsafe { CurrentFiberPool::from_static_slab(&self.config, ptr, len) }
    }

    /// Builds one carrier-backed green pool from this bootstrap.
    ///
    /// # Errors
    ///
    /// Returns one honest runtime error when the selected backend cannot realize the pool.
    pub fn build_green(self, carriers: &ThreadPool) -> Result<GreenPool, FiberError> {
        GreenPool::new(&self.config, carriers)
    }

    /// Builds one hosted carrier-backed runtime using the platform's automatic carrier selection.
    ///
    /// # Errors
    ///
    /// Returns one honest runtime error when either the carrier pool or fiber pool cannot be
    /// realized on the current platform.
    #[cfg(feature = "std")]
    pub fn build_hosted(self) -> Result<HostedFiberRuntime, FiberError> {
        HostedFiberRuntime::from_bootstrap(self)
    }

    /// Builds one hosted carrier-backed runtime using an explicit carrier-pool shape.
    ///
    /// # Errors
    ///
    /// Returns one honest runtime error when either the carrier pool or fiber pool cannot be
    /// realized on the current platform.
    #[cfg(feature = "std")]
    pub fn build_hosted_with(
        self,
        runtime: HostedFiberRuntimeConfig<'_>,
    ) -> Result<HostedFiberRuntime, FiberError> {
        HostedFiberRuntime::from_bootstrap_with(self, runtime)
    }
}

/// Backward-compatible alias for the older green-pool naming.
pub type GreenPoolConfig<'a> = FiberPoolConfig<'a>;

#[cfg(test)]
mod config_policy_tests {
    use super::*;

    #[test]
    fn fcfs_steal_locality_sets_work_stealing_and_requested_locality() {
        let config = FiberPoolConfig::new()
            .with_scheduling(GreenScheduling::Priority)
            .with_fcfs_steal_locality(CarrierSpawnLocalityPolicy::SameCore);
        assert_eq!(config.scheduling, GreenScheduling::WorkStealing);
        assert_eq!(
            config.spawn_locality_policy,
            CarrierSpawnLocalityPolicy::SameCore
        );
    }
}
