/// Scheduling policy for green threads on top of carrier workers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GreenScheduling {
    /// Simple first-come-first-served scheduling across carrier queues.
    Fifo,
    /// Priority-aware scheduling across carriers.
    Priority,
    /// First-come-first-served carrier queues with idle-carrier work stealing.
    WorkStealing,
}

/// Growth policy for the green-thread pool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GreenGrowth {
    /// Fixed-capacity pool with explicit admission control.
    Fixed,
    /// Grow green-thread population on demand up to the configured cap.
    OnDemand,
}

/// Signal-path stack telemetry policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FiberTelemetry {
    /// No per-fiber growth counters.
    Disabled,
    /// Count growth events only.
    GrowthCount,
    /// Count growth events and track committed-page high-water marks.
    Full,
}

/// Response policy when an elastic fiber stack reaches its reservation ceiling.
#[derive(Debug, Clone, Copy)]
pub enum CapacityPolicy {
    /// Hard-fault semantics only. No advisory callback.
    Abort,
    /// Invoke the callback after the running fiber yields or completes.
    Notify(fn(FiberCapacityEvent)),
}

impl PartialEq for CapacityPolicy {
    fn eq(&self, other: &Self) -> bool {
        match (*self, *other) {
            (Self::Abort, Self::Abort) => true,
            (Self::Notify(lhs), Self::Notify(rhs)) => core::ptr::fn_addr_eq(lhs, rhs),
            _ => false,
        }
    }
}

impl Eq for CapacityPolicy {}

impl core::hash::Hash for CapacityPolicy {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        match self {
            Self::Abort => core::hash::Hash::hash(&0_u8, state),
            Self::Notify(_) => core::hash::Hash::hash(&1_u8, state),
        }
    }
}

/// Response policy when one cooperative fiber exceeds its declared run-between-yield budget.
#[derive(Debug, Clone, Copy)]
pub enum FiberYieldBudgetPolicy {
    /// Treat the overrun as one fatal fault and abort the current process.
    Abort,
    /// Invoke the callback when the watchdog or post-run check observes the overrun.
    Notify(fn(FiberYieldBudgetEvent)),
}

impl PartialEq for FiberYieldBudgetPolicy {
    fn eq(&self, other: &Self) -> bool {
        match (*self, *other) {
            (Self::Abort, Self::Abort) => true,
            (Self::Notify(lhs), Self::Notify(rhs)) => core::ptr::fn_addr_eq(lhs, rhs),
            _ => false,
        }
    }
}

impl Eq for FiberYieldBudgetPolicy {}

impl core::hash::Hash for FiberYieldBudgetPolicy {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        match self {
            Self::Abort => core::hash::Hash::hash(&0_u8, state),
            Self::Notify(_) => core::hash::Hash::hash(&1_u8, state),
        }
    }
}

/// Advisory event emitted when a fiber reaches stack capacity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FiberCapacityEvent {
    /// Stable fiber identifier.
    pub fiber_id: u64,
    /// Carrier worker that was executing the fiber.
    pub carrier_id: usize,
    /// Currently committed usable pages.
    pub committed_pages: u32,
    /// Maximum usable pages allowed by the reservation.
    pub reservation_pages: u32,
}

/// Advisory event emitted when one cooperative fiber exceeds its declared run-between-yield
/// budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FiberYieldBudgetEvent {
    /// Stable fiber identifier.
    pub fiber_id: u64,
    /// Carrier worker that was executing the fiber when the overrun was observed.
    pub carrier_id: usize,
    /// Declared maximum run duration before the fiber must yield, park, or complete.
    pub budget: Duration,
    /// Observed run duration when the overrun was detected.
    pub observed: Duration,
}

/// Approximate pool-level stack telemetry snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FiberStackStats {
    /// Total growth events across live fibers in the pool.
    pub total_growth_events: u64,
    /// Maximum exact byte watermark observed across released fixed stacks in this pool.
    pub peak_used_bytes: usize,
    /// Maximum committed-page count observed across live fibers.
    pub peak_committed_pages: u32,
    /// Distribution of live fibers by committed-page count.
    pub committed_distribution: FiberStackDistribution,
    /// Number of live fibers currently at reservation capacity.
    pub at_capacity_count: usize,
}

/// Exact generated-task metadata resolved for one concrete task type.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GeneratedFiberTaskMetadataView {
    /// Exact analyzer-predicted stack budget in bytes before class rounding.
    pub stack_bytes: usize,
    /// Analyzer-resolved task priority.
    pub priority: i8,
    /// Analyzer-resolved execution strategy.
    pub execution: FiberTaskExecution,
}

/// Runtime admission snapshot for one live or completed spawned task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FiberTaskAdmission {
    /// Carrier queue assigned to this task.
    pub carrier: usize,
    /// Admitted stack class for this task.
    ///
    /// Inline `NoYield` work still reports the logical class carried by its task attributes even
    /// though it does not consume one real fiber stack slot.
    pub stack_class: FiberStackClass,
    /// Strict-priority scheduling value for this task.
    pub priority: FiberTaskPriority,
    /// Optional cooperative run budget for this task.
    pub yield_budget: Option<Duration>,
    /// Selected execution strategy for this task.
    pub execution: FiberTaskExecution,
}

/// Planning-time context-switch surface used for exact current-thread fiber backing analysis.
///
/// This stays narrower than the full runtime `FiberSupport` contract on purpose: build-time slab
/// sizing only needs the stack-shape truth that affects backing size, not the whole live runtime
/// capability surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FiberPlanningSupport {
    /// Whether the platform can create fresh contexts honestly.
    pub can_make: bool,
    /// Whether the platform can swap into a saved context honestly.
    pub can_swap: bool,
    /// Backend/runtime bytes that must be added to predicted task stack usage before admission.
    pub structural_stack_overhead_bytes: usize,
    /// Minimum required stack alignment in bytes.
    pub min_stack_alignment: usize,
    /// Architectural red-zone size below the live stack pointer in bytes.
    pub red_zone_bytes: usize,
    /// Architectural stack growth direction.
    pub stack_direction: ContextStackDirection,
    /// Whether the platform requires guard pages or equivalent stack limits.
    pub guard_required: bool,
}

impl FiberPlanningSupport {
    /// Returns one unsupported planning surface.
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            can_make: false,
            can_swap: false,
            structural_stack_overhead_bytes: 0,
            min_stack_alignment: 1,
            red_zone_bytes: 0,
            stack_direction: ContextStackDirection::Unknown,
            guard_required: false,
        }
    }

    /// Returns one supported same-carrier planning surface with explicit stack-shape truth.
    #[must_use]
    pub const fn same_carrier(
        structural_stack_overhead_bytes: usize,
        min_stack_alignment: usize,
        red_zone_bytes: usize,
        stack_direction: ContextStackDirection,
        guard_required: bool,
    ) -> Self {
        Self {
            can_make: true,
            can_swap: true,
            structural_stack_overhead_bytes,
            min_stack_alignment,
            red_zone_bytes,
            stack_direction,
            guard_required,
        }
    }

    /// Returns the default planning surface for the selected runtime lane.
    #[must_use]
    pub fn selected_runtime() -> Self {
        Self::from_fiber_support(FiberSystem::new().support())
    }

    /// Returns one planning surface derived from the live low-level fiber support.
    #[must_use]
    pub const fn from_fiber_support(support: FiberSupport) -> Self {
        Self {
            can_make: support.context.caps.contains(ContextCaps::MAKE),
            can_swap: support.context.caps.contains(ContextCaps::SWAP),
            structural_stack_overhead_bytes: support.context.structural_stack_overhead_bytes,
            min_stack_alignment: support.context.min_stack_alignment,
            red_zone_bytes: support.context.red_zone_bytes,
            stack_direction: support.context.stack_direction,
            guard_required: support.context.guard_required,
        }
    }

    const fn supports_current_thread(self) -> bool {
        self.can_make && self.can_swap
    }
}

/// Memory-footprint summary for the stack-backing side of one fiber pool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FiberStackMemoryFootprint {
    /// Total schedulable stack slots across all fixed/classed stack pools.
    pub total_capacity: usize,
    /// Total reserved stack-region bytes across all stack pools, excluding metadata/control.
    pub reserved_stack_bytes: usize,
    /// Total usable stack bytes across all slots, excluding guards and metadata.
    pub usable_stack_bytes: usize,
    /// Stack-pool metadata bytes, including slab headers and class-pool registries.
    pub metadata_bytes: usize,
}

impl FiberStackMemoryFootprint {
    /// Returns the total bytes reserved by stack backing plus stack metadata.
    #[must_use]
    pub const fn total_bytes(self) -> usize {
        self.reserved_stack_bytes + self.metadata_bytes
    }
}

/// Memory-footprint summary for one live fiber pool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FiberPoolMemoryFootprint {
    /// Number of carrier queues provisioned for this pool.
    pub carrier_count: usize,
    /// Total schedulable task slots across the pool.
    pub task_capacity: usize,
    /// Stack-backing footprint for the pool.
    pub stack: FiberStackMemoryFootprint,
    /// Scheduler/task metadata bytes outside the stack backing itself.
    pub runtime_metadata_bytes: usize,
    /// Control-block bytes used to own the shared pool root.
    pub control_bytes: usize,
}

impl FiberPoolMemoryFootprint {
    /// Returns the total bytes reserved by this pool across stack, metadata, and control state.
    #[must_use]
    pub const fn total_bytes(self) -> usize {
        self.stack.total_bytes() + self.runtime_metadata_bytes + self.control_bytes
    }
}

/// Huge-page preference for large fiber stack reservations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HugePagePolicy {
    /// Small-page treatment only.
    Disabled,
    /// Prefer huge-page treatment for large reservations when the backend supports advice.
    Enabled {
        /// Target huge-page granule used as the advisory threshold.
        size: HugePageSize,
    },
}

/// Huge-page granule used for advisory thresholds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HugePageSize {
    /// 2 MiB huge pages.
    TwoMiB,
    /// 1 GiB huge pages.
    OneGiB,
}

impl HugePageSize {
    const fn bytes(self) -> usize {
        match self {
            Self::TwoMiB => 2 * 1024 * 1024,
            Self::OneGiB => 1024 * 1024 * 1024,
        }
    }
}

/// Stack-backing strategy for one fiber reservation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FiberStackBacking {
    /// Fully committed fixed-capacity stacks with hardware guard pages only.
    Fixed {
        /// Total usable stack size per fiber.
        stack_size: NonZeroUsize,
    },
    /// Reservation-backed elastic stacks with MMU-driven page promotion.
    Elastic {
        /// Initially committed usable bytes at fiber creation.
        initial_size: NonZeroUsize,
        /// Maximum usable bytes the fiber may grow to.
        max_size: NonZeroUsize,
    },
}

/// One power-of-two stack class used for fiber admission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FiberStackClass {
    size_bytes: NonZeroUsize,
}

impl FiberStackClass {
    /// Smallest supported class size for explicit stack-class admission.
    pub const MIN: Self = Self {
        size_bytes: NonZeroUsize::new(4 * 1024).unwrap(),
    };

    /// Creates one explicit stack class from a power-of-two byte size.
    ///
    /// # Errors
    ///
    /// Returns an error when the supplied byte size is smaller than the minimum class or is not a
    /// power of two.
    pub const fn new(size_bytes: NonZeroUsize) -> Result<Self, FiberError> {
        if size_bytes.get() < Self::MIN.size_bytes.get() || !size_bytes.get().is_power_of_two() {
            return Err(FiberError::invalid());
        }
        Ok(Self { size_bytes })
    }

    /// Rounds one required stack byte budget up to the next valid class.
    ///
    /// # Errors
    ///
    /// Returns an error when rounding would overflow the platform word size.
    pub const fn from_stack_bytes(size_bytes: NonZeroUsize) -> Result<Self, FiberError> {
        let requested = size_bytes.get();
        let minimum = Self::MIN.size_bytes.get();
        let target = if requested < minimum {
            minimum
        } else {
            requested
        };
        let Some(rounded) = target.checked_next_power_of_two() else {
            return Err(FiberError::resource_exhausted());
        };
        let Some(non_zero) = NonZeroUsize::new(rounded) else {
            return Err(FiberError::invalid());
        };
        Self::new(non_zero)
    }

    /// Returns the class size in bytes.
    #[must_use]
    pub const fn size_bytes(self) -> NonZeroUsize {
        self.size_bytes
    }
}

/// Provisioning for one fiber stack class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FiberStackClassConfig {
    /// Power-of-two class size in bytes.
    pub class: FiberStackClass,
    /// Number of stack slots provisioned per carrier for this class.
    pub slots_per_carrier: usize,
    /// Number of slots committed together when this class-backed pool grows on demand.
    pub growth_chunk: usize,
}

impl FiberStackClassConfig {
    /// Creates one checked class-backed pool entry.
    ///
    /// By default, the growth chunk matches the full slot count for the class. Use
    /// [`FiberStackClassConfig::with_growth_chunk`] to tighten on-demand commit size without
    /// dragging the legacy pool-wide knob back into the class-backed model.
    ///
    /// # Errors
    ///
    /// Returns an error when the class would provision zero slots.
    pub const fn new(class: FiberStackClass, slots_per_carrier: usize) -> Result<Self, FiberError> {
        if slots_per_carrier == 0 {
            return Err(FiberError::invalid());
        }
        Ok(Self {
            class,
            slots_per_carrier,
            growth_chunk: slots_per_carrier,
        })
    }

    /// Returns one copy of this class entry with an explicit on-demand growth chunk.
    ///
    /// # Errors
    ///
    /// Returns an error when the chunk is zero or larger than the slot count for this class.
    pub const fn with_growth_chunk(mut self, growth_chunk: usize) -> Result<Self, FiberError> {
        if growth_chunk == 0 || growth_chunk > self.slots_per_carrier {
            return Err(FiberError::invalid());
        }
        self.growth_chunk = growth_chunk;
        Ok(self)
    }

    const fn validate(self) -> Result<Self, FiberError> {
        if self.slots_per_carrier == 0
            || self.growth_chunk == 0
            || self.growth_chunk > self.slots_per_carrier
        {
            return Err(FiberError::invalid());
        }
        Ok(self)
    }
}

/// Hosted-runtime class provisioning expressed as one total budget across all automatic carriers.
#[cfg(feature = "std")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HostedFiberClassConfig {
    /// Power-of-two class size in bytes.
    pub class: FiberStackClass,
    /// Total stack slots requested for this class across the whole hosted runtime.
    pub total_slots: usize,
    /// Total slot count committed together when this class-backed pool grows on demand.
    pub growth_chunk: usize,
}

#[cfg(feature = "std")]
impl HostedFiberClassConfig {
    /// Creates one checked hosted class-budget entry.
    ///
    /// # Errors
    ///
    /// Returns an error when the class would provision zero total slots.
    pub const fn new(class: FiberStackClass, total_slots: usize) -> Result<Self, FiberError> {
        if total_slots == 0 {
            return Err(FiberError::invalid());
        }
        Ok(Self {
            class,
            total_slots,
            growth_chunk: total_slots,
        })
    }

    /// Returns one copy of this hosted class-budget entry with an explicit growth chunk.
    ///
    /// # Errors
    ///
    /// Returns an error when the chunk is zero or larger than the total slot count.
    pub const fn with_growth_chunk(mut self, growth_chunk: usize) -> Result<Self, FiberError> {
        if growth_chunk == 0 || growth_chunk > self.total_slots {
            return Err(FiberError::invalid());
        }
        self.growth_chunk = growth_chunk;
        Ok(self)
    }

    const fn validate(self) -> Result<Self, FiberError> {
        if self.total_slots == 0 || self.growth_chunk == 0 || self.growth_chunk > self.total_slots {
            return Err(FiberError::invalid());
        }
        Ok(self)
    }
}

/// Strict-priority value attached to one fiber task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FiberTaskPriority(i8);

impl FiberTaskPriority {
    /// Lowest priority value.
    pub const MIN: Self = Self(i8::MIN);
    /// Default neutral priority.
    pub const DEFAULT: Self = Self(0);
    /// Highest priority value.
    pub const MAX: Self = Self(i8::MAX);

    /// Creates one explicit priority value.
    #[must_use]
    pub const fn new(value: i8) -> Self {
        Self(value)
    }

    /// Returns the raw priority value.
    #[must_use]
    pub const fn get(self) -> i8 {
        self.0
    }

    #[must_use]
    #[allow(clippy::cast_lossless)]
    const fn queue_index(self) -> usize {
        u8::from_ne_bytes(self.0.wrapping_sub(i8::MIN).to_ne_bytes()) as usize
    }
}

impl Default for FiberTaskPriority {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Saturating ready-queue age attached to one waiting fiber task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
struct FiberTaskAge(u8);

impl FiberTaskAge {
    const ZERO: Self = Self(0);
    const MAX: Self = Self(u8::MAX);

    #[must_use]
    const fn get(self) -> u8 {
        self.0
    }

    #[must_use]
    fn from_epoch_delta(delta: u64) -> Self {
        if delta == 0 {
            Self::ZERO
        } else if delta >= u64::from(u8::MAX) {
            Self::MAX
        } else {
            Self(u8::try_from(delta).unwrap_or(u8::MAX))
        }
    }
}

/// Optional cap on how much virtual waiting age may promote one strict-priority task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FiberTaskAgeCap(u8);

impl FiberTaskAgeCap {
    /// Creates one explicit age cap.
    #[must_use]
    pub const fn new(age: u8) -> Self {
        Self(age)
    }

    #[must_use]
    const fn as_age(self) -> FiberTaskAge {
        FiberTaskAge(self.0)
    }
}

/// One named cooperative exclusion span tracked by the current running green context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CooperativeExclusionSpan(NonZeroU16);

impl CooperativeExclusionSpan {
    /// Creates one explicit exclusion span identifier.
    ///
    /// # Errors
    ///
    /// Returns an error when the supplied identifier is zero.
    pub const fn new(span: u16) -> Result<Self, SyncError> {
        match NonZeroU16::new(span) {
            Some(span) => Ok(Self(span)),
            None => Err(SyncError::invalid()),
        }
    }

    /// Returns the concrete numeric span identifier.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0.get()
    }
}

/// One compile-time cooperative exclusion summary tree over named span bits.
///
/// `summary_levels` are ordered from the leaf parent upward to the root. Each bit in one summary
/// word says “at least one child word below this index is non-zero”.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CooperativeExclusionSummaryTree {
    /// Leaf words containing the actual exclusion bits.
    pub leaf_words: &'static [u32],
    /// Parent summary levels ordered from the leaf parent upward to the root.
    pub summary_levels: &'static [&'static [u32]],
}

impl CooperativeExclusionSummaryTree {
    /// Creates one summary tree from explicit leaf and parent levels.
    #[must_use]
    pub const fn new(
        leaf_words: &'static [u32],
        summary_levels: &'static [&'static [u32]],
    ) -> Self {
        Self {
            leaf_words,
            summary_levels,
        }
    }

    /// Returns the total span-id capacity of the leaf layer.
    #[must_use]
    pub const fn span_capacity(self) -> usize {
        self.leaf_words.len() * COOPERATIVE_EXCLUSION_TREE_WORD_BITS
    }

    #[must_use]
    fn contains(self, span: CooperativeExclusionSpan) -> bool {
        let span_index = usize::from(span.get() - 1);
        let leaf_word_index = span_index / COOPERATIVE_EXCLUSION_TREE_WORD_BITS;
        if leaf_word_index >= self.leaf_words.len() {
            return false;
        }

        let mut child_word_index = leaf_word_index;
        let mut level = 0;
        while level < self.summary_levels.len() {
            let words = self.summary_levels[level];
            let summary_word_index = child_word_index / COOPERATIVE_EXCLUSION_TREE_WORD_BITS;
            if summary_word_index >= words.len() {
                return false;
            }
            let bit = 1_u32 << (child_word_index % COOPERATIVE_EXCLUSION_TREE_WORD_BITS);
            if words[summary_word_index] & bit == 0 {
                return false;
            }
            child_word_index = summary_word_index;
            level += 1;
        }

        let bit = 1_u32 << (span_index % COOPERATIVE_EXCLUSION_TREE_WORD_BITS);
        self.leaf_words[leaf_word_index] & bit != 0
    }
}

impl FiberTaskPriority {
    #[must_use]
    const fn effective_with_age(self, age: FiberTaskAge) -> Self {
        Self(self.0.saturating_add_unsigned(age.get()))
    }
}

/// Execution strategy selected for one admitted task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FiberTaskExecution {
    /// Execute the task on one dedicated fiber stack.
    Fiber,
    /// Execute the task inline on the current carrier stack because it is proven not to yield.
    InlineNoYield,
}

impl FiberTaskExecution {
    #[must_use]
    const fn requires_fiber(self) -> bool {
        matches!(self, Self::Fiber)
    }

    /// Returns whether this execution strategy runs on one real fiber stack.
    #[must_use]
    pub const fn is_fiber(self) -> bool {
        self.requires_fiber()
    }

    /// Returns whether this execution strategy runs inline without one dedicated fiber stack.
    #[must_use]
    pub const fn is_inline(self) -> bool {
        !self.requires_fiber()
    }
}

/// Transitional task-side admission metadata for stack class and priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FiberTaskAttributes {
    /// Required stack class for this task admission.
    pub stack_class: FiberStackClass,
    /// Strict-priority scheduling value for this task.
    pub priority: FiberTaskPriority,
    /// Optional maximum cooperative run duration between explicit handoff points.
    pub yield_budget: Option<Duration>,
    /// Selected execution strategy for this task.
    pub execution: FiberTaskExecution,
}

impl FiberTaskAttributes {
    /// Creates one task attribute set with default priority.
    #[must_use]
    pub const fn new(stack_class: FiberStackClass) -> Self {
        Self {
            stack_class,
            priority: FiberTaskPriority::DEFAULT,
            yield_budget: None,
            execution: FiberTaskExecution::Fiber,
        }
    }

    /// Returns one copy of these attributes with an explicit priority value.
    #[must_use]
    pub const fn with_priority(mut self, priority: FiberTaskPriority) -> Self {
        self.priority = priority;
        self
    }

    /// Returns one copy of these attributes with an explicit cooperative run-between-yield budget.
    #[must_use]
    pub const fn with_yield_budget(mut self, yield_budget: Duration) -> Self {
        self.yield_budget = Some(yield_budget);
        self
    }

    /// Returns one copy of these attributes with an explicit optional cooperative run budget.
    #[must_use]
    pub const fn with_optional_yield_budget(mut self, yield_budget: Option<Duration>) -> Self {
        self.yield_budget = yield_budget;
        self
    }

    /// Returns one copy of these attributes with an explicit execution strategy.
    #[must_use]
    pub const fn with_execution(mut self, execution: FiberTaskExecution) -> Self {
        self.execution = execution;
        self
    }

    /// Builds one explicit task-attribute set from a compile-time stack budget and priority.
    ///
    /// # Errors
    ///
    /// Returns an error when the supplied byte budget cannot be mapped to a supported stack
    /// class.
    pub const fn from_stack_bytes(
        stack_bytes: NonZeroUsize,
        priority: FiberTaskPriority,
    ) -> Result<Self, FiberError> {
        match FiberStackClass::from_stack_bytes(stack_bytes) {
            Ok(class) => Ok(Self::new(class).with_priority(priority)),
            Err(error) => Err(error),
        }
    }
}

/// Explicit fiber-task contract carrying compile-time stack and priority metadata.
///
/// This is the first runtime-facing hook for the planned build-time stack analysis pipeline. The
/// current contract still relies on developer-supplied constants; later tooling can generate or
/// validate them from emitted stack-size metadata.
pub trait ExplicitFiberTask: Send + 'static {
    /// Result type produced by this task.
    type Output: 'static;

    /// Required stack byte budget for this task.
    const STACK_BYTES: NonZeroUsize;

    /// Strict-priority value for this task.
    const PRIORITY: FiberTaskPriority = FiberTaskPriority::DEFAULT;
    /// Optional maximum cooperative run duration between explicit yields or completion.
    const YIELD_BUDGET: Option<Duration> = None;
    /// Execution strategy for this task.
    const EXECUTION: FiberTaskExecution = FiberTaskExecution::Fiber;

    /// Compile-time task attributes derived from the explicit stack and priority contract.
    ///
    /// Invalid explicit task contracts fail when this constant is evaluated, which lets
    /// critical-safe configurations reject unsupported declarations in a const context instead of
    /// politely waiting for runtime.
    const ATTRIBUTES: FiberTaskAttributes =
        match FiberTaskAttributes::from_stack_bytes(Self::STACK_BYTES, Self::PRIORITY) {
            Ok(attributes) => attributes
                .with_optional_yield_budget(Self::YIELD_BUDGET)
                .with_execution(Self::EXECUTION),
            Err(_) => panic!("invalid explicit fiber task contract"),
        };

    /// Runs the explicit task to completion.
    fn run(self) -> Self::Output;

    /// Derives one runtime task-attribute bundle from the compile-time contract.
    ///
    /// # Errors
    ///
    /// Returns an error when the declared byte budget cannot be mapped to a supported stack class.
    fn task_attributes() -> Result<FiberTaskAttributes, FiberError> {
        Ok(Self::ATTRIBUTES)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct GeneratedExplicitFiberTaskMetadata {
    type_name: &'static str,
    stack_bytes: usize,
    priority: i8,
    execution: FiberTaskExecution,
}

#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GeneratedExplicitFiberTaskRoot {
    pub type_name: &'static str,
    pub symbol: &'static str,
    pub priority: i8,
}

/// Hidden compile-time contract emitted by the build-generated metadata pipeline for task types
/// known to this crate.
#[doc(hidden)]
pub trait GeneratedExplicitFiberTaskContract {
    const ATTRIBUTES: FiberTaskAttributes;
}

include!(concat!(env!("OUT_DIR"), "/fiber_task_generated.rs"));

#[doc(hidden)]
pub const GENERATED_EXPLICIT_FIBER_TASK_ROOTS: &[GeneratedExplicitFiberTaskRoot] =
    &[GeneratedExplicitFiberTaskRoot {
        type_name: "fusion_std::thread::fiber::GeneratedFiberTaskMetadataAnchorTask",
        symbol: "generated_fiber_task_metadata_anchor",
        priority: 5,
    }];

/// Returns the compile-time generated-task attributes for one task type with a declared contract.
#[must_use]
pub const fn generated_explicit_task_contract_attributes<T: GeneratedExplicitFiberTaskContract>()
-> FiberTaskAttributes {
    T::ATTRIBUTES
}

/// Includes one generated Rust contract sidecar emitted by the fiber-task analyzer pipeline.
///
/// Downstream crates can use this to pull generated
/// `declare_generated_fiber_task_contract!(...)` entries into scope directly from a build-step
/// output file instead of retyping them by hand.
#[macro_export]
macro_rules! include_generated_fiber_task_contracts {
    ($path:expr $(,)?) => {
        include!($path);
    };
}

const CLOSURE_METADATA_MISS_CACHE_SIZE: usize = 32;

const fn generated_stack_structural_overhead_bytes() -> usize {
    fusion_sys::fiber::system_context_support().structural_stack_overhead_bytes
}

pub const fn admit_generated_fiber_task_stack_bytes(
    stack_bytes: NonZeroUsize,
) -> Result<NonZeroUsize, FiberError> {
    let adjusted = adjust_generated_stack_bytes_for_runtime(stack_bytes.get());
    let Some(adjusted) = NonZeroUsize::new(adjusted) else {
        return Err(FiberError::invalid());
    };
    Ok(adjusted)
}

#[derive(Debug)]
struct ClosureMetadataMissCacheEntry {
    ptr: AtomicUsize,
    len: AtomicUsize,
}

static GENERATED_CLOSURE_METADATA_MISS_CACHE: [ClosureMetadataMissCacheEntry;
    CLOSURE_METADATA_MISS_CACHE_SIZE] = [const {
    ClosureMetadataMissCacheEntry {
        ptr: AtomicUsize::new(0),
        len: AtomicUsize::new(0),
    }
}; CLOSURE_METADATA_MISS_CACHE_SIZE];
static GENERATED_CLOSURE_METADATA_MISS_NEXT: AtomicUsize = AtomicUsize::new(0);

const fn adjust_generated_stack_bytes_for_runtime(stack_bytes: usize) -> usize {
    stack_bytes.saturating_add(generated_stack_structural_overhead_bytes())
}

fn generated_closure_metadata_miss_cached(type_name: &'static str) -> bool {
    let ptr = type_name.as_ptr() as usize;
    let len = type_name.len();
    GENERATED_CLOSURE_METADATA_MISS_CACHE.iter().any(|entry| {
        entry.ptr.load(Ordering::Acquire) == ptr && entry.len.load(Ordering::Acquire) == len
    })
}

fn remember_generated_closure_metadata_miss(type_name: &'static str) {
    let index = GENERATED_CLOSURE_METADATA_MISS_NEXT.fetch_add(1, Ordering::AcqRel)
        % CLOSURE_METADATA_MISS_CACHE_SIZE;
    GENERATED_CLOSURE_METADATA_MISS_CACHE[index]
        .len
        .store(type_name.len(), Ordering::Release);
    GENERATED_CLOSURE_METADATA_MISS_CACHE[index]
        .ptr
        .store(type_name.as_ptr() as usize, Ordering::Release);
}

fn generated_task_attributes_by_type_name(
    type_name: &str,
) -> Result<FiberTaskAttributes, FiberError> {
    let metadata = generated_task_metadata_by_type_name(type_name)?;
    let stack_bytes = NonZeroUsize::new(adjust_generated_stack_bytes_for_runtime(
        metadata.stack_bytes,
    ))
    .ok_or_else(FiberError::unsupported)?;
    Ok(
        FiberTaskAttributes::new(FiberStackClass::from_stack_bytes(stack_bytes)?)
            .with_priority(FiberTaskPriority::new(metadata.priority))
            .with_execution(metadata.execution),
    )
}

fn generated_task_metadata_by_type_name(
    type_name: &str,
) -> Result<GeneratedFiberTaskMetadataView, FiberError> {
    let metadata = GENERATED_EXPLICIT_FIBER_TASKS
        .iter()
        .find(|entry| entry.type_name == type_name)
        .ok_or_else(FiberError::unsupported)?;
    Ok(GeneratedFiberTaskMetadataView {
        stack_bytes: metadata.stack_bytes,
        priority: metadata.priority,
        execution: metadata.execution,
    })
}

/// Returns the exact generated-task metadata for one runtime type name when it exists.
///
/// This is the measurement-facing view of the build-generated manifest: it preserves the exact
/// predicted stack bytes before class rounding so external probes can compare runtime watermarks
/// against the analyzer output instead of against a rounded admission class.
///
/// # Errors
///
/// Returns an error when the generated manifest does not contain the supplied type name.
#[doc(hidden)]
pub fn generated_fiber_task_metadata_by_type_name(
    type_name: &str,
) -> Result<GeneratedFiberTaskMetadataView, FiberError> {
    generated_task_metadata_by_type_name(type_name)
}

/// Returns the admission-adjusted generated stack bytes for one runtime task type when it exists.
///
/// This is the runtime-facing view used for class selection after platform wrapper overhead is
/// accounted for.
#[doc(hidden)]
pub fn generated_fiber_task_admitted_stack_bytes_by_type_name(
    type_name: &str,
) -> Result<usize, FiberError> {
    Ok(adjust_generated_stack_bytes_for_runtime(
        generated_task_metadata_by_type_name(type_name)?.stack_bytes,
    ))
}

const fn generated_max_fiber_task_stack_bytes() -> Option<usize> {
    let mut index = 0;
    let mut max = 0usize;
    while index < GENERATED_EXPLICIT_FIBER_TASKS.len() {
        let candidate = adjust_generated_stack_bytes_for_runtime(
            GENERATED_EXPLICIT_FIBER_TASKS[index].stack_bytes,
        );
        if candidate > max {
            max = candidate;
        }
        index += 1;
    }
    if max == 0 { None } else { Some(max) }
}

fn generated_default_fiber_stack_size() -> Result<NonZeroUsize, FiberError> {
    let Some(bytes) = generated_max_fiber_task_stack_bytes() else {
        return Err(FiberError::unsupported());
    };
    let bytes = apply_fiber_sizing_strategy_bytes(bytes, default_runtime_sizing_strategy())?;
    let Some(bytes) = NonZeroUsize::new(bytes) else {
        return Err(FiberError::invalid());
    };
    Ok(FiberStackClass::from_stack_bytes(bytes)?.size_bytes())
}

/// Returns the default generated fiber stack size admitted for the current crate.
///
/// This is the rounded stack-class size selected from the largest generated fiber-task contract
/// visible to the current build.
///
/// # Errors
///
/// Returns an error when generated fiber-task metadata is unavailable.
pub fn generated_default_fiber_stack_bytes() -> Result<usize, FiberError> {
    Ok(generated_default_fiber_stack_size()?.get())
}

fn generated_task_attributes<T: 'static>() -> Result<FiberTaskAttributes, FiberError> {
    generated_task_attributes_by_type_name(type_name::<T>())
}
