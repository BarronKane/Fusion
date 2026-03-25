//! Domain 2: public green-thread and fiber orchestration surface.

use core::any::{TypeId, type_name};
use core::cell::UnsafeCell;
use core::fmt;
use core::marker::PhantomData;
use core::mem::{ManuallyDrop, MaybeUninit, align_of, size_of};
use core::num::NonZeroU16;
use core::num::NonZeroUsize;
use core::ops::{Deref, DerefMut};
use core::ptr::{self, NonNull, addr_of_mut};
use core::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, AtomicUsize, Ordering};
use core::time::Duration;

use crate::sync::{
    Mutex as SyncMutex,
    OnceLock,
    Semaphore,
    SharedHeader,
    SharedRelease,
    SyncError,
    SyncErrorKind,
};
use fusion_pal::sys::fiber::{
    FiberHostError,
    FiberHostErrorKind,
    PlatformFiberSignalStack,
    PlatformFiberWakeSignal,
    PlatformWakeToken,
    system_fiber_host,
};
use fusion_pal::sys::mem::{
    Advise,
    Backing,
    CachePolicy,
    MapFlags,
    MapRequest,
    MemAdviceCaps,
    MemAdvise,
    MemBase,
    MemMap,
    MemProtect,
    Placement,
    Protect,
    Region,
    RegionAttrs,
    system_mem,
};
use fusion_sys::event::{
    EventCaps,
    EventInterest,
    EventKey,
    EventNotification,
    EventPoller,
    EventReadiness,
    EventRecord,
    EventSourceHandle,
    EventSystem,
};
use fusion_sys::fiber::{
    ContextCaps,
    ContextMigrationSupport,
    ContextStackDirection,
    Fiber,
    FiberError,
    FiberReturn,
    FiberStack,
    FiberSupport,
    FiberSystem,
    FiberYield,
    current_context as system_fiber_context,
    yield_now as system_yield_now,
};
use fusion_sys::mem::resource::{
    BoundMemoryResource,
    BoundResourceSpec,
    MemoryResource,
    MemoryResourceHandle,
    ResourceBackingKind,
    ResourceError,
    ResourceErrorKind,
    ResourceRange,
};
use fusion_sys::sync::Mutex as SysMutex;
#[cfg(feature = "std")]
use fusion_sys::thread::{
    RawThreadEntry,
    ThreadConfig,
    ThreadConstraintMode,
    ThreadEntryReturn,
    ThreadHandle,
    ThreadJoinPolicy,
    ThreadLogicalCpuId,
    ThreadMigrationPolicy,
    ThreadPlacementPhase,
    ThreadPlacementRequest,
    ThreadPlacementTarget,
    ThreadProcessorGroupId,
    ThreadStartMode,
};
use fusion_sys::thread::{SystemWorkItem, ThreadSchedulerCaps, ThreadSystem};

use super::ThreadPool;
#[cfg(feature = "std")]
use super::{PoolPlacement, ThreadPoolConfig};
use super::{RuntimeSizingStrategy, default_runtime_sizing_strategy};
#[cfg(feature = "std")]
use core::sync::atomic::AtomicU64;
#[cfg(feature = "std")]
use fusion_pal::hal::{HardwareTopologyQuery as _, system_hardware};

const INLINE_GREEN_JOB_BYTES: usize = 256;
const INLINE_GREEN_RESULT_BYTES: usize = 256;
const CARRIER_EVENT_BATCH: usize = 64;
const FIBER_PRIORITY_LEVELS: usize = u8::MAX as usize + 1;
const FIBER_PRIORITY_WORDS: usize = FIBER_PRIORITY_LEVELS / usize::BITS as usize;
const EMPTY_QUEUE_SLOT: usize = usize::MAX;
const MAX_COOPERATIVE_LOCK_NESTING: usize = 16;
const COOPERATIVE_EXCLUSION_TREE_WORD_BITS: usize = u32::BITS as usize;
const ACTIVE_COOPERATIVE_EXCLUSION_FAST_SPAN_CAPACITY: usize = 1024;
const ACTIVE_COOPERATIVE_EXCLUSION_FAST_LEAF_WORDS: usize =
    ACTIVE_COOPERATIVE_EXCLUSION_FAST_SPAN_CAPACITY / COOPERATIVE_EXCLUSION_TREE_WORD_BITS;
const UNRANKED_COOPERATIVE_LOCK: u16 = 0;
const NO_COOPERATIVE_EXCLUSION_SPAN: u16 = 0;
const FIXED_STACK_WATERMARK_SENTINEL: u8 = 0xA5;
#[cfg(feature = "std")]
const FIBER_YIELD_WATCHDOG_POLL_INTERVAL: Duration = Duration::from_millis(1);
const GREEN_RUNTIME_REGION_CACHE_SLOTS: usize = 4;
#[cfg(target_pointer_width = "64")]
const STEAL_SEED_MIX: usize = 0x9e37_79b9_7f4a_7c15;
#[cfg(not(target_pointer_width = "64"))]
const STEAL_SEED_MIX: usize = 0x7f4a_7c15;
#[cfg(feature = "std")]
const ZERO_LOGICAL_CPU: ThreadLogicalCpuId = ThreadLogicalCpuId {
    group: ThreadProcessorGroupId(0),
    index: 0,
};

#[allow(clippy::cast_possible_truncation)]
const fn wake_token_to_word(token: PlatformWakeToken) -> usize {
    let raw = token.into_raw();
    if raw > usize::MAX as u64 {
        0
    } else {
        raw as usize
    }
}

const fn word_to_wake_token(raw: usize) -> PlatformWakeToken {
    PlatformWakeToken::from_raw(raw as u64)
}

/// Scheduling policy for green threads on top of carrier workers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GreenScheduling {
    /// Simple FIFO scheduling across carriers.
    Fifo,
    /// Priority-aware scheduling across carriers.
    Priority,
    /// Per-carrier deque scheduling with work stealing.
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
#[derive(Debug, PartialEq, Eq)]
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
            min_stack_alignment: 1,
            red_zone_bytes: 0,
            stack_direction: ContextStackDirection::Unknown,
            guard_required: false,
        }
    }

    /// Returns one supported same-carrier planning surface with explicit stack-shape truth.
    #[must_use]
    pub const fn same_carrier(
        min_stack_alignment: usize,
        red_zone_bytes: usize,
        stack_direction: ContextStackDirection,
        guard_required: bool,
    ) -> Self {
        Self {
            can_make: true,
            can_swap: true,
            min_stack_alignment,
            red_zone_bytes,
            stack_direction,
            guard_required,
        }
    }

    /// Returns the truthful Cortex-M same-carrier planning surface.
    #[must_use]
    pub const fn cortex_m() -> Self {
        Self::same_carrier(8, 0, ContextStackDirection::Down, false)
    }

    /// Returns one planning surface derived from the live low-level fiber support.
    #[must_use]
    pub const fn from_fiber_support(support: FiberSupport) -> Self {
        Self {
            can_make: support.context.caps.contains(ContextCaps::MAKE),
            can_swap: support.context.caps.contains(ContextCaps::SWAP),
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
#[cfg(all(feature = "std", target_os = "linux"))]
const HOSTED_LINUX_GENERATED_STACK_FLOOR_BYTES: usize = 3304;
#[cfg(all(feature = "std", target_os = "linux"))]
const HOSTED_LINUX_GENERATED_STACK_OVERHEAD_BYTES: usize = 352;

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

#[cfg(all(feature = "std", target_os = "linux"))]
const fn adjust_generated_stack_bytes_for_runtime(stack_bytes: usize) -> usize {
    let adjusted = stack_bytes.saturating_add(HOSTED_LINUX_GENERATED_STACK_OVERHEAD_BYTES);
    if adjusted < HOSTED_LINUX_GENERATED_STACK_FLOOR_BYTES {
        HOSTED_LINUX_GENERATED_STACK_FLOOR_BYTES
    } else {
        adjusted
    }
}

#[cfg(not(all(feature = "std", target_os = "linux")))]
const fn adjust_generated_stack_bytes_for_runtime(stack_bytes: usize) -> usize {
    stack_bytes
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

fn generated_task_attributes<T: 'static>() -> Result<FiberTaskAttributes, FiberError> {
    generated_task_attributes_by_type_name(type_name::<T>())
}

fn closure_spawn_task_attributes<F: 'static>(
    default_class: FiberStackClass,
) -> FiberTaskAttributes {
    let type_name = type_name::<F>();
    if generated_closure_metadata_miss_cached(type_name) {
        return FiberTaskAttributes::new(default_class);
    }
    generated_task_attributes_by_type_name(type_name).unwrap_or_else(|_| {
        remember_generated_closure_metadata_miss(type_name);
        FiberTaskAttributes::new(default_class)
    })
}

/// Explicit fiber-task contract resolved through build-generated metadata.
///
/// The current generated path is backed by a build-script registry. A later stack-analysis tool
/// can replace the manifest source without changing the runtime-facing contract.
pub trait GeneratedExplicitFiberTask: Send + 'static {
    /// Result type produced by this task.
    type Output: 'static;

    /// Optional maximum cooperative run duration between explicit yields or completion.
    const YIELD_BUDGET: Option<Duration> = None;

    /// Runs the explicit task to completion.
    fn run(self) -> Self::Output;

    /// Resolves one runtime task-attribute bundle from generated metadata.
    ///
    /// # Errors
    ///
    /// Returns an error when the build metadata does not contain an entry for this task type, or
    /// when the generated stack budget cannot be mapped to a supported stack class.
    fn task_attributes() -> Result<FiberTaskAttributes, FiberError>
    where
        Self: Sized,
    {
        Ok(generated_task_attributes::<Self>()?.with_optional_yield_budget(Self::YIELD_BUDGET))
    }
}

/// Asserts in one const/static context that an explicit fiber task is supported by one pool
/// configuration.
///
/// This is the critical-safe admission hook for explicit tasks. It intentionally works only with
/// explicit task contracts for now; generated-task compile-time rejection will stay deferred until
/// the generated path stops depending on runtime type-name lookup.
#[macro_export]
macro_rules! assert_explicit_fiber_task_supported {
    ($config:expr, $task:ty $(,)?) => {
        const _: () = {
            ($config).assert_explicit_task_supported::<$task>();
        };
    };
}

/// Asserts in one const/static context that raw fiber-task attributes are supported by one pool
/// configuration.
///
/// This is the lowest-common-denominator compile-time admission hook. It is especially useful for
/// downstream build-generated code that can emit `FiberTaskAttributes` directly without requiring
/// the current crate to know the task type.
#[macro_export]
macro_rules! assert_fiber_task_attributes_supported {
    ($config:expr, $task:expr $(,)?) => {
        const _: () = {
            ($config).assert_task_attributes_supported($task);
        };
    };
}

/// Declares one build-generated fiber-task contract for use in downstream crates.
///
/// This is the cross-crate bridge for generated-task admission. The task still needs a
/// `GeneratedExplicitFiberTask` impl, but that impl can delegate `task_attributes()` to
/// [`generated_explicit_task_contract_attributes()`].
#[macro_export]
macro_rules! declare_generated_fiber_task_contract {
    ($task:ty, $stack_bytes:expr $(,)?) => {
        $crate::declare_generated_fiber_task_contract!(
            $task,
            $stack_bytes,
            $crate::thread::FiberTaskPriority::DEFAULT,
            $crate::thread::FiberTaskExecution::Fiber
        );
    };
    ($task:ty, $stack_bytes:expr, $priority:expr $(,)?) => {
        $crate::declare_generated_fiber_task_contract!(
            $task,
            $stack_bytes,
            $priority,
            $crate::thread::FiberTaskExecution::Fiber
        );
    };
    ($task:ty, $stack_bytes:expr, $priority:expr, $execution:expr $(,)?) => {
        impl $crate::thread::GeneratedExplicitFiberTaskContract for $task {
            const ATTRIBUTES: $crate::thread::FiberTaskAttributes =
                match $crate::thread::FiberTaskAttributes::from_stack_bytes($stack_bytes, $priority)
                {
                    Ok(attributes) => attributes.with_execution($execution),
                    Err(_) => panic!("invalid generated fiber task contract"),
                };
        }
    };
}

/// Asserts in one const/static context that a build-generated fiber task contract is supported by
/// one pool configuration.
///
/// This currently works only for generated tasks with compile-time contracts emitted into the
/// current crate. Runtime type-name lookup still exists as a compatibility path for the broader
/// generated-task API.
#[macro_export]
macro_rules! assert_generated_fiber_task_supported {
    ($config:expr, $task:ty $(,)?) => {
        const _: () = {
            ($config).assert_generated_task_supported::<$task>();
        };
    };
}

/// Hidden generated-task anchor used to exercise the build-generated metadata pipeline in normal
/// library artifacts.
#[doc(hidden)]
pub struct GeneratedFiberTaskMetadataAnchorTask(u32);

#[doc(hidden)]
#[unsafe(no_mangle)]
pub const extern "Rust" fn generated_fiber_task_metadata_anchor(bytes: u32) -> u32 {
    generated_fiber_task_metadata_anchor_leaf(bytes)
}

#[inline(never)]
const fn generated_fiber_task_metadata_anchor_leaf(bytes: u32) -> u32 {
    bytes.saturating_add(1)
}

#[inline(never)]
fn generated_closure_task_root<F, T>(job: F) -> T
where
    F: FnOnce() -> T,
{
    job()
}

/// Hidden closure-root anchor used to exercise closure-task metadata generation in ordinary
/// library artifacts.
#[doc(hidden)]
#[unsafe(no_mangle)]
pub extern "Rust" fn generated_closure_task_metadata_anchor(bytes: u32) -> u32 {
    generated_closure_task_root(|| generated_fiber_task_metadata_anchor_leaf(bytes))
}

impl GeneratedExplicitFiberTask for GeneratedFiberTaskMetadataAnchorTask {
    type Output = u32;

    fn run(self) -> Self::Output {
        generated_fiber_task_metadata_anchor(self.0)
    }
}

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
                initial_size: unsafe { NonZeroUsize::new_unchecked(4 * 1024) },
                max_size: unsafe { NonZeroUsize::new_unchecked(256 * 1024) },
            },
            sizing: default_runtime_sizing_strategy(),
            classes: &[],
            guard_pages: 1,
            growth_chunk: 32,
            max_fibers_per_carrier: 64,
            scheduling: GreenScheduling::Fifo,
            priority_age_cap: None,
            growth: GreenGrowth::OnDemand,
            telemetry: FiberTelemetry::Disabled,
            capacity_policy: CapacityPolicy::Abort,
            yield_budget_policy: FiberYieldBudgetPolicy::Abort,
            reactor_policy: GreenReactorPolicy::Automatic,
            huge_pages: HugePagePolicy::Disabled,
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
            priority_age_cap: None,
            growth: GreenGrowth::Fixed,
            telemetry: FiberTelemetry::Disabled,
            capacity_policy: CapacityPolicy::Abort,
            yield_budget_policy: FiberYieldBudgetPolicy::Abort,
            reactor_policy: GreenReactorPolicy::Automatic,
            huge_pages: HugePagePolicy::Disabled,
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
            priority_age_cap: None,
            growth: GreenGrowth::OnDemand,
            telemetry: FiberTelemetry::Disabled,
            capacity_policy: CapacityPolicy::Abort,
            yield_budget_policy: FiberYieldBudgetPolicy::Abort,
            reactor_policy: GreenReactorPolicy::Automatic,
            huge_pages: HugePagePolicy::Disabled,
        })
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
            priority_age_cap: None,
            growth: GreenGrowth::Fixed,
            telemetry: FiberTelemetry::Disabled,
            capacity_policy: CapacityPolicy::Abort,
            yield_budget_policy: FiberYieldBudgetPolicy::Abort,
            reactor_policy: GreenReactorPolicy::Automatic,
            huge_pages: HugePagePolicy::Disabled,
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
    #[cfg(not(feature = "critical-safe-generated-contracts"))]
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
    #[cfg(feature = "critical-safe-generated-contracts")]
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

    /// Returns one copy of this bootstrap with explicit scheduling.
    #[must_use]
    pub const fn with_scheduling(mut self, scheduling: GreenScheduling) -> Self {
        self.config = self.config.with_scheduling(scheduling);
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GreenTaskState {
    Queued,
    Running,
    Yielded,
    Waiting,
    Finishing,
    Completed,
    Failed(FiberError),
}

const fn is_terminal_task_state(state: GreenTaskState) -> bool {
    matches!(state, GreenTaskState::Completed | GreenTaskState::Failed(_))
}

const EMPTY_EVENT_RECORD: EventRecord = EventRecord {
    key: EventKey(0),
    notification: EventNotification::Readiness(EventReadiness::empty()),
};

struct MetadataSlice<T> {
    ptr: core::ptr::NonNull<T>,
    len: usize,
}

impl<T> Copy for MetadataSlice<T> {}

impl<T> Clone for MetadataSlice<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> MetadataSlice<T> {
    const fn empty() -> Self {
        Self {
            ptr: core::ptr::NonNull::dangling(),
            len: 0,
        }
    }

    const fn len(&self) -> usize {
        self.len
    }

    const fn is_empty(&self) -> bool {
        self.len == 0
    }

    unsafe fn write(&self, index: usize, value: T) -> Result<(), FiberError> {
        if index >= self.len {
            return Err(FiberError::invalid());
        }
        // SAFETY: the metadata slice owner reserved a contiguous region for `len` elements and is
        // responsible for initialization discipline before exposing shared references.
        unsafe {
            self.ptr.as_ptr().add(index).write(value);
        }
        Ok(())
    }

    const fn as_slice(&self) -> &[T] {
        // SAFETY: callers construct `MetadataSlice<T>` only after reserving enough contiguous
        // space, and all public readers are used only after initialization is complete.
        unsafe { core::slice::from_raw_parts(self.ptr.as_ptr(), self.len) }
    }

    const fn as_mut_slice(&mut self) -> &mut [T] {
        // SAFETY: the owner provides unique mutable access before any aliasing references are
        // handed out.
        unsafe { core::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.len) }
    }

    fn get(&self, index: usize) -> Option<&T> {
        self.as_slice().get(index)
    }
}

impl<T> fmt::Debug for MetadataSlice<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MetadataSlice")
            .field("ptr", &self.ptr)
            .field("len", &self.len)
            .finish_non_exhaustive()
    }
}

impl<T> Deref for MetadataSlice<T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl<T> DerefMut for MetadataSlice<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_mut_slice()
    }
}

// SAFETY: `MetadataSlice<T>` is just a pointer/length view over allocator-owned memory. Sending
// or sharing it is sound when the underlying element type already satisfies the corresponding
// thread-safety contract.
unsafe impl<T: Send> Send for MetadataSlice<T> {}
// SAFETY: see above.
unsafe impl<T: Sync> Sync for MetadataSlice<T> {}

struct MappedVec<T> {
    region: Option<Region>,
    ptr: core::ptr::NonNull<T>,
    len: usize,
    capacity: usize,
}

impl<T: Copy> MappedVec<T> {
    const fn new() -> Self {
        Self {
            region: None,
            ptr: core::ptr::NonNull::dangling(),
            len: 0,
            capacity: 0,
        }
    }

    const fn len(&self) -> usize {
        self.len
    }

    const fn is_empty(&self) -> bool {
        self.len == 0
    }

    const fn as_slice(&self) -> &[T] {
        // SAFETY: `ptr` references `len` initialized elements while the owned mapping stays live.
        unsafe { core::slice::from_raw_parts(self.ptr.as_ptr(), self.len) }
    }

    const fn as_mut_slice(&mut self) -> &mut [T] {
        // SAFETY: the owned mapping stays live and mutable access is unique through `&mut self`.
        unsafe { core::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.len) }
    }

    fn truncate(&mut self, len: usize) {
        self.len = self.len.min(len);
    }

    fn grow_for(&mut self, min_capacity: usize) -> Result<(), FiberError> {
        let mut target = self.capacity.max(4);
        while target < min_capacity {
            target = target
                .checked_mul(2)
                .ok_or_else(FiberError::resource_exhausted)?;
        }

        let mut next = Self::with_capacity(target)?;
        for item in self.as_slice() {
            next.push_copy(item)?;
        }
        *self = next;
        Ok(())
    }

    fn with_capacity(capacity: usize) -> Result<Self, FiberError> {
        if capacity == 0 {
            return Ok(Self::new());
        }
        if size_of::<T>() == 0 {
            return Err(FiberError::unsupported());
        }

        let memory = system_mem();
        let page = memory.page_info().alloc_granule.get();
        let align = page.max(align_of::<T>());
        let bytes = size_of::<T>()
            .checked_mul(capacity)
            .ok_or_else(FiberError::resource_exhausted)?;
        let len = fiber_align_up(bytes, page)?;
        let region = unsafe {
            memory.map(&MapRequest {
                len,
                align,
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

        Ok(Self {
            region: Some(region),
            ptr: region
                .base
                .as_non_null::<T>()
                .ok_or_else(FiberError::invalid)?,
            len: 0,
            capacity,
        })
    }

    fn push_copy(&mut self, value: &T) -> Result<(), FiberError> {
        if self.len == self.capacity {
            self.grow_for(
                self.len
                    .checked_add(1)
                    .ok_or_else(FiberError::resource_exhausted)?,
            )?;
        }
        // SAFETY: growth above guarantees spare initialized storage for exactly one `T`.
        unsafe {
            self.ptr.as_ptr().add(self.len).write(*value);
        }
        self.len += 1;
        Ok(())
    }

    fn push(&mut self, value: T) -> Result<(), FiberError> {
        if self.len == self.capacity {
            self.grow_for(
                self.len
                    .checked_add(1)
                    .ok_or_else(FiberError::resource_exhausted)?,
            )?;
        }
        // SAFETY: growth above guarantees spare initialized storage for exactly one `T`.
        unsafe {
            self.ptr.as_ptr().add(self.len).write(value);
        }
        self.len += 1;
        Ok(())
    }

    fn retain<F>(&mut self, mut keep: F)
    where
        F: FnMut(&T) -> bool,
    {
        let mut write = 0;
        for read in 0..self.len {
            let value = self.as_slice()[read];
            if keep(&value) {
                // SAFETY: `write <= read < len` always addresses initialized storage.
                unsafe {
                    self.ptr.as_ptr().add(write).write(value);
                }
                write += 1;
            }
        }
        self.len = write;
    }

    fn sort_by_key<K, F>(&mut self, mut f: F)
    where
        K: Ord,
        F: FnMut(&T) -> K,
    {
        let slice = self.as_mut_slice();
        for i in 1..slice.len() {
            let key = f(&slice[i]);
            let value = slice[i];
            let mut j = i;
            while j > 0 && f(&slice[j - 1]) > key {
                slice[j] = slice[j - 1];
                j -= 1;
            }
            slice[j] = value;
        }
    }
}

impl<T> Drop for MappedVec<T> {
    fn drop(&mut self) {
        if let Some(region) = self.region.take() {
            let _ = unsafe { system_mem().unmap(region) };
        }
    }
}

impl<T: Copy> Deref for MappedVec<T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl<T: Copy> DerefMut for MappedVec<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_mut_slice()
    }
}

impl<T: Copy + fmt::Debug> fmt::Debug for MappedVec<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_slice().fmt(f)
    }
}

impl<T: Copy + PartialEq> PartialEq for MappedVec<T> {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl<T: Copy + Eq> Eq for MappedVec<T> {}

// SAFETY: `MappedVec<T>` owns its mapping and only exposes shared/mutable access according to `T`.
unsafe impl<T: Copy + Send> Send for MappedVec<T> {}
// SAFETY: see above.
unsafe impl<T: Copy + Sync> Sync for MappedVec<T> {}

#[derive(Debug, PartialEq, Eq)]
pub struct FiberStackDistribution {
    entries: MappedVec<(u32, usize)>,
}

impl FiberStackDistribution {
    const fn new() -> Self {
        Self {
            entries: MappedVec::new(),
        }
    }

    fn increment(&mut self, committed_pages: u32) -> Result<(), FiberError> {
        if let Some((_, count)) = self
            .entries
            .as_mut_slice()
            .iter_mut()
            .find(|(pages, _)| *pages == committed_pages)
        {
            *count += 1;
            return Ok(());
        }
        self.entries.push((committed_pages, 1))
    }

    fn sort(&mut self) {
        self.entries
            .sort_by_key(|(committed_pages, _)| *committed_pages);
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    #[must_use]
    pub const fn as_slice(&self) -> &[(u32, usize)] {
        self.entries.as_slice()
    }
}

impl Default for FiberStackDistribution {
    fn default() -> Self {
        Self::new()
    }
}

impl Deref for FiberStackDistribution {
    type Target = [(u32, usize)];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

struct MetadataCursor {
    region: Region,
    offset: usize,
}

impl MetadataCursor {
    const fn new(region: Region) -> Self {
        Self { region, offset: 0 }
    }

    fn reserve_slice<T>(&mut self, len: usize) -> Result<MetadataSlice<T>, FiberError> {
        if len == 0 || size_of::<T>() == 0 {
            return Err(FiberError::invalid());
        }

        let base = self.region.base.get();
        let start = fiber_align_up(
            base.checked_add(self.offset)
                .ok_or_else(FiberError::resource_exhausted)?,
            align_of::<T>(),
        )?;
        let offset = start
            .checked_sub(base)
            .ok_or_else(FiberError::resource_exhausted)?;
        let bytes = size_of::<T>()
            .checked_mul(len)
            .ok_or_else(FiberError::resource_exhausted)?;
        let end = offset
            .checked_add(bytes)
            .ok_or_else(FiberError::resource_exhausted)?;
        if end > self.region.len {
            return Err(FiberError::resource_exhausted());
        }

        self.offset = end;
        Ok(MetadataSlice {
            ptr: core::ptr::NonNull::new(start as *mut T).ok_or_else(FiberError::invalid)?,
            len,
        })
    }
}

fn fiber_align_up(value: usize, align: usize) -> Result<usize, FiberError> {
    if align == 0 || !align.is_power_of_two() {
        return Err(FiberError::invalid());
    }
    let mask = align - 1;
    value
        .checked_add(mask)
        .map(|rounded| rounded & !mask)
        .ok_or_else(FiberError::resource_exhausted)
}

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
struct FiberStackSlabHeader {
    metadata_len: usize,
    payload_offset: usize,
    capacity: usize,
    slot_stride: usize,
    elastic: bool,
}

#[derive(Debug)]
struct MetadataIndexStack {
    entries: MetadataSlice<usize>,
    len: usize,
}

impl MetadataIndexStack {
    fn with_prefix(entries: MetadataSlice<usize>, len: usize) -> Result<Self, FiberError> {
        if len > entries.len() {
            return Err(FiberError::invalid());
        }
        for index in 0..entries.len() {
            unsafe {
                entries.write(index, 0)?;
            }
        }
        for index in 0..len {
            unsafe {
                entries.write(index, index)?;
            }
        }
        Ok(Self { entries, len })
    }

    fn pop(&mut self) -> Option<usize> {
        if self.len == 0 {
            return None;
        }
        self.len -= 1;
        Some(self.entries[self.len])
    }

    fn push(&mut self, value: usize) -> Result<(), FiberError> {
        if self.len == self.entries.len() {
            return Err(FiberError::state_conflict());
        }
        self.entries[self.len] = value;
        self.len += 1;
        Ok(())
    }

    fn retain_less_than(&mut self, limit: usize) {
        let mut write = 0;
        for read in 0..self.len {
            let value = self.entries[read];
            if value < limit {
                self.entries[write] = value;
                write += 1;
            }
        }
        self.len = write;
    }
}

struct RuntimeCell<T> {
    fast: bool,
    value: UnsafeCell<T>,
    lock: SysMutex<()>,
}

unsafe impl<T: Send> Send for RuntimeCell<T> {}
unsafe impl<T: Send> Sync for RuntimeCell<T> {}

impl<T: fmt::Debug> fmt::Debug for RuntimeCell<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RuntimeCell")
            .field("fast", &self.fast)
            .finish_non_exhaustive()
    }
}

impl<T> RuntimeCell<T> {
    const fn new(fast: bool, value: T) -> Self {
        Self {
            fast,
            value: UnsafeCell::new(value),
            lock: SysMutex::new(()),
        }
    }

    fn with<R>(&self, f: impl FnOnce(&mut T) -> R) -> Result<R, FiberError> {
        if self.fast {
            // SAFETY: fast-mode cells are only used by the thread-affine current-thread runtime.
            return Ok(unsafe { f(&mut *self.value.get()) });
        }
        let _guard = self.lock.lock().map_err(fiber_error_from_sync)?;
        // SAFETY: the lock serializes mutable access in the shared runtime.
        Ok(unsafe { f(&mut *self.value.get()) })
    }

    fn with_ref<R>(&self, f: impl FnOnce(&T) -> R) -> Result<R, FiberError> {
        if self.fast {
            // SAFETY: fast-mode cells are only used by the thread-affine current-thread runtime.
            return Ok(unsafe { f(&*self.value.get()) });
        }
        let _guard = self.lock.lock().map_err(fiber_error_from_sync)?;
        // SAFETY: the lock serializes shared access in the shared runtime.
        Ok(unsafe { f(&*self.value.get()) })
    }
}

#[derive(Debug)]
struct MetadataIndexQueue {
    entries: MetadataSlice<usize>,
    head: usize,
    tail: usize,
    len: usize,
}

impl MetadataIndexQueue {
    fn new(entries: MetadataSlice<usize>) -> Result<Self, FiberError> {
        if entries.is_empty() {
            return Err(FiberError::invalid());
        }
        for index in 0..entries.len() {
            unsafe {
                entries.write(index, 0)?;
            }
        }
        Ok(Self {
            entries,
            head: 0,
            tail: 0,
            len: 0,
        })
    }

    fn enqueue(&mut self, value: usize) -> Result<(), FiberError> {
        if self.len == self.entries.len() {
            return Err(FiberError::resource_exhausted());
        }
        self.entries[self.tail] = value;
        self.tail = (self.tail + 1) % self.entries.len();
        self.len += 1;
        Ok(())
    }

    fn dequeue(&mut self) -> Option<usize> {
        if self.len == 0 {
            return None;
        }
        let value = self.entries[self.head];
        self.head = (self.head + 1) % self.entries.len();
        self.len -= 1;
        Some(value)
    }

    fn steal(&mut self) -> Option<usize> {
        if self.len == 0 {
            return None;
        }
        self.tail = if self.tail == 0 {
            self.entries.len() - 1
        } else {
            self.tail - 1
        };
        let value = self.entries[self.tail];
        self.len -= 1;
        Some(value)
    }
}

#[derive(Debug, Clone, Copy)]
struct PriorityBucket {
    head: usize,
    tail: usize,
}

impl PriorityBucket {
    const fn empty() -> Self {
        Self {
            head: EMPTY_QUEUE_SLOT,
            tail: EMPTY_QUEUE_SLOT,
        }
    }

    const fn is_empty(self) -> bool {
        self.head == EMPTY_QUEUE_SLOT
    }
}

#[derive(Debug)]
struct MetadataPriorityQueue {
    buckets: MetadataSlice<PriorityBucket>,
    next: MetadataSlice<usize>,
    base_priorities: MetadataSlice<i8>,
    enqueue_epochs: MetadataSlice<u64>,
    age_cap: Option<FiberTaskAgeCap>,
    len: usize,
    epoch: u64,
    non_empty: [usize; FIBER_PRIORITY_WORDS],
}

impl MetadataPriorityQueue {
    fn new(
        buckets: MetadataSlice<PriorityBucket>,
        next: MetadataSlice<usize>,
        base_priorities: MetadataSlice<i8>,
        enqueue_epochs: MetadataSlice<u64>,
        age_cap: Option<FiberTaskAgeCap>,
    ) -> Result<Self, FiberError> {
        if next.is_empty()
            || buckets.len() != FIBER_PRIORITY_LEVELS
            || base_priorities.len() != next.len()
            || enqueue_epochs.len() != next.len()
        {
            return Err(FiberError::invalid());
        }
        for index in 0..buckets.len() {
            unsafe {
                buckets.write(index, PriorityBucket::empty())?;
            }
        }
        for index in 0..next.len() {
            unsafe {
                next.write(index, EMPTY_QUEUE_SLOT)?;
                base_priorities.write(index, FiberTaskPriority::DEFAULT.get())?;
                enqueue_epochs.write(index, 0)?;
            }
        }
        Ok(Self {
            buckets,
            next,
            base_priorities,
            enqueue_epochs,
            age_cap,
            len: 0,
            epoch: 0,
            non_empty: [0; FIBER_PRIORITY_WORDS],
        })
    }

    fn enqueue(&mut self, value: usize, priority: FiberTaskPriority) -> Result<(), FiberError> {
        if value >= self.next.len() || self.len == self.next.len() {
            return Err(FiberError::resource_exhausted());
        }

        let index = priority.queue_index();
        let bucket = self
            .buckets
            .get(index)
            .copied()
            .ok_or_else(FiberError::invalid)?;
        self.next[value] = EMPTY_QUEUE_SLOT;
        self.base_priorities[value] = priority.get();
        self.enqueue_epochs[value] = self.epoch;

        if bucket.is_empty() {
            self.buckets[index] = PriorityBucket {
                head: value,
                tail: value,
            };
            self.non_empty[index / usize::BITS as usize] |=
                1usize << (index % usize::BITS as usize);
        } else {
            self.next[bucket.tail] = value;
            self.buckets[index].tail = value;
        }

        self.len += 1;
        Ok(())
    }

    fn dequeue(&mut self) -> Option<usize> {
        if self.len == 0 {
            return None;
        }

        let mut selected = None::<(usize, FiberTaskPriority)>;
        let bits_per_word = usize::BITS as usize;

        for word_index in (0..self.non_empty.len()).rev() {
            let mut word = self.non_empty[word_index];
            while word != 0 {
                let highest_bit = usize::BITS as usize - 1 - word.leading_zeros() as usize;
                let bucket_index = word_index * bits_per_word + highest_bit;
                let bucket = self.buckets[bucket_index];
                let candidate = bucket.head;
                let effective = self.effective_priority(candidate);
                if selected
                    .as_ref()
                    .is_none_or(|(selected_bucket, selected_effective)| {
                        effective > *selected_effective
                            || (effective == *selected_effective && bucket_index > *selected_bucket)
                    })
                {
                    selected = Some((bucket_index, effective));
                }
                word &= !(1usize << highest_bit);
            }
        }

        let (bucket_index, _) = selected?;
        let value = self.pop_bucket_head(bucket_index)?;
        self.epoch = self.epoch.saturating_add(1);
        self.enqueue_epochs[value] = self.epoch;
        Some(value)
    }

    fn effective_priority(&self, value: usize) -> FiberTaskPriority {
        FiberTaskPriority::new(self.base_priorities[value])
            .effective_with_age(self.waiting_age(value))
    }

    fn waiting_age(&self, value: usize) -> FiberTaskAge {
        let age =
            FiberTaskAge::from_epoch_delta(self.epoch.saturating_sub(self.enqueue_epochs[value]));
        match self.age_cap {
            Some(cap) if age.get() > cap.as_age().get() => cap.as_age(),
            _ => age,
        }
    }

    fn pop_bucket_head(&mut self, bucket_index: usize) -> Option<usize> {
        let mut bucket = self.buckets[bucket_index];
        let value = bucket.head;
        if value == EMPTY_QUEUE_SLOT {
            return None;
        }
        let next = self.next[value];
        bucket.head = next;
        if bucket.tail == value {
            bucket.tail = next;
        }
        self.next[value] = EMPTY_QUEUE_SLOT;

        if bucket.head == EMPTY_QUEUE_SLOT {
            bucket = PriorityBucket::empty();
            self.non_empty[bucket_index / usize::BITS as usize] &=
                !(1usize << (bucket_index % usize::BITS as usize));
        }

        self.buckets[bucket_index] = bucket;
        self.len -= 1;
        Some(value)
    }
}

#[repr(C, align(64))]
struct InlineGreenJobBytes {
    bytes: [u8; INLINE_GREEN_JOB_BYTES],
}

struct InlineGreenJobStorage {
    storage: MaybeUninit<InlineGreenJobBytes>,
    run: Option<unsafe fn(*mut u8)>,
    drop: Option<unsafe fn(*mut u8)>,
    occupied: bool,
}

impl fmt::Debug for InlineGreenJobStorage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InlineGreenJobStorage")
            .field("occupied", &self.occupied)
            .finish_non_exhaustive()
    }
}

impl InlineGreenJobStorage {
    const fn empty() -> Self {
        Self {
            storage: MaybeUninit::uninit(),
            run: None,
            drop: None,
            occupied: false,
        }
    }

    fn store<F>(&mut self, job: F) -> Result<(), FiberError>
    where
        F: FnOnce() + Send + 'static,
    {
        if self.occupied {
            return Err(FiberError::state_conflict());
        }
        if size_of::<F>() > size_of::<InlineGreenJobBytes>()
            || align_of::<F>() > align_of::<InlineGreenJobBytes>()
        {
            return Err(FiberError::unsupported());
        }

        unsafe {
            self.storage.as_mut_ptr().cast::<F>().write(job);
        }
        self.run = Some(run_inline_green_job::<F>);
        self.drop = Some(drop_inline_green_job::<F>);
        self.occupied = true;
        Ok(())
    }

    fn take_runner(&mut self) -> Result<InlineGreenJobRunner, FiberError> {
        if !self.occupied {
            return Err(FiberError::state_conflict());
        }
        let run = self.run.take().ok_or_else(FiberError::state_conflict)?;
        self.drop = None;
        self.occupied = false;
        Ok(InlineGreenJobRunner {
            ptr: self.storage.as_mut_ptr().cast::<u8>(),
            run,
        })
    }

    fn clear(&mut self) {
        if !self.occupied {
            self.run = None;
            self.drop = None;
            return;
        }

        if let Some(drop) = self.drop.take() {
            unsafe {
                drop(self.storage.as_mut_ptr().cast::<u8>());
            }
        }
        self.run = None;
        self.occupied = false;
    }
}

impl Drop for InlineGreenJobStorage {
    fn drop(&mut self) {
        self.clear();
    }
}

struct InlineGreenJobRunner {
    ptr: *mut u8,
    run: unsafe fn(*mut u8),
}

impl InlineGreenJobRunner {
    fn run(self) {
        unsafe {
            (self.run)(self.ptr);
        }
    }
}

#[repr(C, align(64))]
struct InlineGreenResultBytes {
    bytes: [u8; INLINE_GREEN_RESULT_BYTES],
}

struct InlineGreenResultStorage {
    storage: MaybeUninit<InlineGreenResultBytes>,
    drop: Option<unsafe fn(*mut u8)>,
    type_id: Option<TypeId>,
    occupied: bool,
}

impl fmt::Debug for InlineGreenResultStorage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InlineGreenResultStorage")
            .field("occupied", &self.occupied)
            .finish_non_exhaustive()
    }
}

impl InlineGreenResultStorage {
    const fn empty() -> Self {
        Self {
            storage: MaybeUninit::uninit(),
            drop: None,
            type_id: None,
            occupied: false,
        }
    }

    const fn supports<T: 'static>() -> bool {
        size_of::<T>() <= size_of::<InlineGreenResultBytes>()
            && align_of::<T>() <= align_of::<InlineGreenResultBytes>()
    }

    fn store<T: 'static>(&mut self, value: T) -> Result<(), FiberError> {
        if self.occupied {
            return Err(FiberError::state_conflict());
        }
        if !Self::supports::<T>() {
            return Err(FiberError::unsupported());
        }

        unsafe {
            self.storage.as_mut_ptr().cast::<T>().write(value);
        }
        self.drop = Some(drop_inline_green_job::<T>);
        self.type_id = Some(TypeId::of::<T>());
        self.occupied = true;
        Ok(())
    }

    fn take<T: 'static>(&mut self) -> Result<T, FiberError> {
        if !self.occupied || self.type_id != Some(TypeId::of::<T>()) {
            return Err(FiberError::state_conflict());
        }

        self.drop = None;
        self.type_id = None;
        self.occupied = false;
        Ok(unsafe { self.storage.as_ptr().cast::<T>().read() })
    }

    fn clear(&mut self) {
        if !self.occupied {
            self.drop = None;
            self.type_id = None;
            return;
        }

        if let Some(drop) = self.drop.take() {
            unsafe {
                drop(self.storage.as_mut_ptr().cast::<u8>());
            }
        }
        self.type_id = None;
        self.occupied = false;
    }
}

impl Drop for InlineGreenResultStorage {
    fn drop(&mut self) {
        self.clear();
    }
}

unsafe fn run_inline_green_job<F>(ptr: *mut u8)
where
    F: FnOnce(),
{
    unsafe {
        ptr.cast::<F>().read()();
    }
}

unsafe fn drop_inline_green_job<F>(ptr: *mut u8) {
    unsafe {
        ptr.cast::<F>().drop_in_place();
    }
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CurrentGreenYieldAction {
    Requeue,
    WaitReadiness {
        source: EventSourceHandle,
        interest: EventInterest,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CarrierWaiterRecord {
    key: EventKey,
    source: EventSourceHandle,
    slot_index: usize,
    task_id: u64,
}

#[derive(Debug)]
struct CarrierReactorState {
    reactor: EventSystem,
    poller: SyncMutex<EventPoller>,
    waiters: SyncMutex<MetadataSlice<Option<CarrierWaiterRecord>>>,
    wake: PlatformFiberWakeSignal,
    wake_key: EventKey,
    capacity: PlatformFiberWakeSignal,
    capacity_key: EventKey,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct CarrierPollResult {
    ready_count: usize,
    capacity_signaled: bool,
}

#[cfg(feature = "std")]
#[derive(Debug)]
struct CarrierYieldBudgetState {
    slot_index: AtomicUsize,
    task_id: AtomicU64,
    budget_nanos: AtomicU64,
    started_nanos: AtomicU64,
    faulted: AtomicBool,
    reported: AtomicBool,
}

#[cfg(feature = "std")]
impl CarrierYieldBudgetState {
    const IDLE_SLOT: usize = usize::MAX;

    const fn new() -> Self {
        Self {
            slot_index: AtomicUsize::new(Self::IDLE_SLOT),
            task_id: AtomicU64::new(0),
            budget_nanos: AtomicU64::new(0),
            started_nanos: AtomicU64::new(0),
            faulted: AtomicBool::new(false),
            reported: AtomicBool::new(false),
        }
    }

    fn begin(&self, slot_index: usize, task_id: u64, start_nanos: u64, budget_nanos: u64) {
        self.task_id.store(task_id, Ordering::Release);
        self.started_nanos.store(start_nanos, Ordering::Release);
        self.budget_nanos.store(budget_nanos, Ordering::Release);
        self.faulted.store(false, Ordering::Release);
        self.reported.store(false, Ordering::Release);
        self.slot_index.store(slot_index, Ordering::Release);
    }

    fn clear(&self) {
        self.slot_index.store(Self::IDLE_SLOT, Ordering::Release);
        self.task_id.store(0, Ordering::Release);
        self.budget_nanos.store(0, Ordering::Release);
        self.started_nanos.store(0, Ordering::Release);
        self.faulted.store(false, Ordering::Release);
        self.reported.store(false, Ordering::Release);
    }
}

#[cfg(feature = "std")]
#[derive(Debug)]
struct GreenYieldBudgetRuntime {
    carriers: std::boxed::Box<[CarrierYieldBudgetState]>,
    watchdog_started: AtomicBool,
}

#[cfg(feature = "std")]
impl GreenYieldBudgetRuntime {
    fn new(carrier_count: usize) -> Self {
        let carriers = std::iter::repeat_with(CarrierYieldBudgetState::new)
            .take(carrier_count)
            .collect::<std::vec::Vec<_>>()
            .into_boxed_slice();
        Self {
            carriers,
            watchdog_started: AtomicBool::new(false),
        }
    }

    fn now_nanos() -> Result<u64, FiberError> {
        current_monotonic_nanos()
    }
}

impl CarrierReactorState {
    fn new(waiters: MetadataSlice<Option<CarrierWaiterRecord>>) -> Result<Self, FiberError> {
        for index in 0..waiters.len() {
            unsafe {
                waiters.write(index, None)?;
            }
        }

        let reactor = EventSystem::new();
        let host = system_fiber_host();
        let mut poller = reactor.create().map_err(fiber_error_from_event)?;
        let wake = host.create_wake_signal().map_err(fiber_error_from_host)?;
        let wake_key = reactor
            .register(
                &mut poller,
                EventSourceHandle(wake.source_handle().map_err(fiber_error_from_host)?),
                EventInterest::READABLE | EventInterest::ERROR | EventInterest::HANGUP,
            )
            .map_err(fiber_error_from_event)?;
        let capacity_signal = host.create_wake_signal().map_err(fiber_error_from_host)?;
        let capacity_key = reactor
            .register(
                &mut poller,
                EventSourceHandle(
                    capacity_signal
                        .source_handle()
                        .map_err(fiber_error_from_host)?,
                ),
                EventInterest::READABLE | EventInterest::ERROR | EventInterest::HANGUP,
            )
            .map_err(fiber_error_from_event)?;
        Ok(Self {
            reactor,
            poller: SyncMutex::new(poller),
            waiters: SyncMutex::new(waiters),
            wake,
            wake_key,
            capacity: capacity_signal,
            capacity_key,
        })
    }

    fn signal(&self) -> Result<(), FiberError> {
        self.wake.signal().map_err(fiber_error_from_host)
    }

    #[allow(clippy::missing_const_for_fn)]
    fn capacity_token(&self) -> PlatformWakeToken {
        self.capacity.token()
    }

    fn register_wait(
        &self,
        slot_index: usize,
        task_id: u64,
        source: EventSourceHandle,
        interest: EventInterest,
    ) -> Result<(), FiberError> {
        let mut poller = self.poller.lock().map_err(fiber_error_from_sync)?;
        let mut waiters = self.waiters.lock().map_err(fiber_error_from_sync)?;
        if waiters
            .iter()
            .flatten()
            .any(|waiter| waiter.source == source || waiter.slot_index == slot_index)
        {
            return Err(FiberError::state_conflict());
        }

        let slot = waiters
            .iter_mut()
            .find(|entry| entry.is_none())
            .ok_or_else(FiberError::resource_exhausted)?;
        let key = self
            .reactor
            .register(
                &mut poller,
                source,
                interest | EventInterest::ERROR | EventInterest::HANGUP,
            )
            .map_err(fiber_error_from_event)?;
        *slot = Some(CarrierWaiterRecord {
            key,
            source,
            slot_index,
            task_id,
        });
        Ok(())
    }

    fn waiter_count(&self) -> Result<usize, FiberError> {
        Ok(self
            .waiters
            .lock()
            .map_err(fiber_error_from_sync)?
            .iter()
            .flatten()
            .count())
    }

    fn poll_ready(
        &self,
        timeout: Option<Duration>,
        ready: &mut [Option<CarrierWaiterRecord>; CARRIER_EVENT_BATCH],
    ) -> Result<CarrierPollResult, FiberError> {
        let mut poller = self.poller.lock().map_err(fiber_error_from_sync)?;
        let mut events = [EMPTY_EVENT_RECORD; CARRIER_EVENT_BATCH];
        let count = self
            .reactor
            .poll(&mut poller, &mut events, timeout)
            .map_err(fiber_error_from_event)?;
        let mut result = CarrierPollResult::default();
        for event in events.into_iter().take(count) {
            if event.key == self.wake_key {
                self.wake.drain().map_err(fiber_error_from_host)?;
                continue;
            }
            if event.key == self.capacity_key {
                self.capacity.drain().map_err(fiber_error_from_host)?;
                result.capacity_signaled = true;
                continue;
            }

            let waiter = {
                let mut waiters = self.waiters.lock().map_err(fiber_error_from_sync)?;
                let slot = waiters
                    .iter_mut()
                    .find(|entry| entry.as_ref().is_some_and(|waiter| waiter.key == event.key));
                slot.and_then(Option::take)
            };

            if let Some(waiter) = waiter {
                self.reactor
                    .deregister(&mut poller, waiter.key)
                    .map_err(fiber_error_from_event)?;
                if result.ready_count < ready.len() {
                    ready[result.ready_count] = Some(waiter);
                    result.ready_count += 1;
                }
            }
        }
        Ok(result)
    }

    fn cancel_one_waiter(&self) -> Result<Option<CarrierWaiterRecord>, FiberError> {
        let mut poller = self.poller.lock().map_err(fiber_error_from_sync)?;
        let mut waiters = self.waiters.lock().map_err(fiber_error_from_sync)?;
        let Some(slot) = waiters.iter_mut().find(|entry| entry.is_some()) else {
            return Ok(None);
        };
        let waiter = slot.take().ok_or_else(FiberError::state_conflict)?;
        self.reactor
            .deregister(&mut poller, waiter.key)
            .map_err(fiber_error_from_event)?;
        Ok(Some(waiter))
    }
}

#[derive(Debug)]
struct CarrierQueue {
    queue: RuntimeCell<CarrierReadyQueue>,
    ready: Semaphore,
    reactor: Option<CarrierReactorState>,
    steal_state: AtomicUsize,
}

#[derive(Debug, Clone, Copy)]
struct CarrierQueueSlices {
    queue_entries: Option<MetadataSlice<usize>>,
    priority_buckets: Option<MetadataSlice<PriorityBucket>>,
    priority_next: Option<MetadataSlice<usize>>,
    priority_values: Option<MetadataSlice<i8>>,
    priority_enqueue_epochs: Option<MetadataSlice<u64>>,
    waiters: Option<MetadataSlice<Option<CarrierWaiterRecord>>>,
}

#[derive(Debug)]
enum CarrierReadyQueue {
    Fifo(MetadataIndexQueue),
    Priority(MetadataPriorityQueue),
}

impl CarrierReadyQueue {
    fn new(
        scheduling: GreenScheduling,
        slices: CarrierQueueSlices,
        priority_age_cap: Option<FiberTaskAgeCap>,
    ) -> Result<Self, FiberError> {
        match scheduling {
            GreenScheduling::Fifo | GreenScheduling::WorkStealing => Ok(Self::Fifo(
                MetadataIndexQueue::new(slices.queue_entries.ok_or_else(FiberError::invalid)?)?,
            )),
            GreenScheduling::Priority => Ok(Self::Priority(MetadataPriorityQueue::new(
                slices.priority_buckets.ok_or_else(FiberError::invalid)?,
                slices.priority_next.ok_or_else(FiberError::invalid)?,
                slices.priority_values.ok_or_else(FiberError::invalid)?,
                slices
                    .priority_enqueue_epochs
                    .ok_or_else(FiberError::invalid)?,
                priority_age_cap,
            )?)),
        }
    }

    fn enqueue(&mut self, value: usize, priority: FiberTaskPriority) -> Result<(), FiberError> {
        match self {
            Self::Fifo(queue) => queue.enqueue(value),
            Self::Priority(queue) => queue.enqueue(value, priority),
        }
    }

    fn dequeue(&mut self) -> Option<usize> {
        match self {
            Self::Fifo(queue) => queue.dequeue(),
            Self::Priority(queue) => queue.dequeue(),
        }
    }

    fn steal(&mut self) -> Option<usize> {
        match self {
            Self::Fifo(queue) => queue.steal(),
            Self::Priority(_) => None,
        }
    }
}

impl CarrierQueue {
    fn new(
        scheduling: GreenScheduling,
        slices: CarrierQueueSlices,
        priority_age_cap: Option<FiberTaskAgeCap>,
        seed: usize,
        fast: bool,
    ) -> Result<Self, FiberError> {
        let capacity = match scheduling {
            GreenScheduling::Fifo | GreenScheduling::WorkStealing => {
                slices.queue_entries.ok_or_else(FiberError::invalid)?.len()
            }
            GreenScheduling::Priority => {
                slices.priority_next.ok_or_else(FiberError::invalid)?.len()
            }
        };
        Ok(Self {
            queue: RuntimeCell::new(
                fast,
                CarrierReadyQueue::new(scheduling, slices, priority_age_cap)?,
            ),
            ready: Semaphore::new(
                0,
                u32::try_from(capacity).map_err(|_| FiberError::resource_exhausted())?,
            )
            .map_err(fiber_error_from_sync)?,
            reactor: match slices.waiters {
                Some(waiters) => Some(CarrierReactorState::new(waiters)?),
                None => None,
            },
            steal_state: AtomicUsize::new(seed.max(1)),
        })
    }

    fn signal(&self) -> Result<(), FiberError> {
        if let Some(reactor) = &self.reactor {
            return reactor.signal();
        }
        match self.ready.release(1) {
            Ok(()) => Ok(()),
            Err(error)
                if matches!(error.kind, SyncErrorKind::Overflow | SyncErrorKind::Invalid) =>
            {
                Ok(())
            }
            Err(error) => Err(fiber_error_from_sync(error)),
        }
    }

    fn capacity_token(&self) -> PlatformWakeToken {
        self.reactor.as_ref().map_or(
            PlatformWakeToken::invalid(),
            CarrierReactorState::capacity_token,
        )
    }

    fn next_steal_start(&self, carrier_count: usize) -> usize {
        if carrier_count <= 1 {
            return 0;
        }

        let mut current = self.steal_state.load(Ordering::Acquire).max(1);
        loop {
            let next = xorshift64(current);
            match self.steal_state.compare_exchange_weak(
                current,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    let peers = carrier_count - 1;
                    let offset = next % peers;
                    return offset + 1;
                }
                Err(observed) => current = observed.max(1),
            }
        }
    }
}

#[derive(Debug)]
struct GreenTaskRecord {
    allocated: bool,
    id: u64,
    carrier: usize,
    stack_pool_index: usize,
    stack_slot: usize,
    stack_class: FiberStackClass,
    priority: FiberTaskPriority,
    yield_budget: Option<Duration>,
    execution: FiberTaskExecution,
    fiber: Option<Fiber>,
    job: InlineGreenJobStorage,
    result: InlineGreenResultStorage,
    state: GreenTaskState,
}

impl GreenTaskRecord {
    const fn empty() -> Self {
        Self {
            allocated: false,
            id: 0,
            carrier: 0,
            stack_pool_index: 0,
            stack_slot: 0,
            stack_class: FiberStackClass::MIN,
            priority: FiberTaskPriority::DEFAULT,
            yield_budget: None,
            execution: FiberTaskExecution::Fiber,
            fiber: None,
            job: InlineGreenJobStorage::empty(),
            result: InlineGreenResultStorage::empty(),
            state: GreenTaskState::Completed,
        }
    }
}

#[derive(Debug)]
struct GreenTaskSlot {
    owner: AtomicUsize,
    slot_index: usize,
    cooperative_lock_depth: AtomicUsize,
    cooperative_lock_ranks: [AtomicU16; MAX_COOPERATIVE_LOCK_NESTING],
    cooperative_exclusion_spans: [AtomicU16; MAX_COOPERATIVE_LOCK_NESTING],
    cooperative_exclusion_summary_leaf: [AtomicU32; ACTIVE_COOPERATIVE_EXCLUSION_FAST_LEAF_WORDS],
    cooperative_exclusion_summary_root: AtomicU32,
    cooperative_exclusion_summary_overflow: AtomicBool,
    completion_published: AtomicBool,
    completion_waiters: AtomicUsize,
    yield_action: RuntimeCell<CurrentGreenYieldAction>,
    record: RuntimeCell<GreenTaskRecord>,
    completed: RuntimeCell<Option<Semaphore>>,
    handle_refs: AtomicUsize,
}

impl GreenTaskSlot {
    fn new(slot_index: usize, fast: bool) -> Result<Self, FiberError> {
        Ok(Self {
            owner: AtomicUsize::new(0),
            slot_index,
            cooperative_lock_depth: AtomicUsize::new(0),
            cooperative_lock_ranks: [const { AtomicU16::new(UNRANKED_COOPERATIVE_LOCK) };
                MAX_COOPERATIVE_LOCK_NESTING],
            cooperative_exclusion_spans: [const { AtomicU16::new(NO_COOPERATIVE_EXCLUSION_SPAN) };
                MAX_COOPERATIVE_LOCK_NESTING],
            cooperative_exclusion_summary_leaf: [const { AtomicU32::new(0) };
                ACTIVE_COOPERATIVE_EXCLUSION_FAST_LEAF_WORDS],
            cooperative_exclusion_summary_root: AtomicU32::new(0),
            cooperative_exclusion_summary_overflow: AtomicBool::new(false),
            completion_published: AtomicBool::new(false),
            completion_waiters: AtomicUsize::new(0),
            yield_action: RuntimeCell::new(fast, CurrentGreenYieldAction::Requeue),
            record: RuntimeCell::new(fast, GreenTaskRecord::empty()),
            completed: RuntimeCell::new(fast, None),
            handle_refs: AtomicUsize::new(0),
        })
    }

    const fn context_ptr(&self) -> *mut () {
        core::ptr::from_ref(self).cast_mut().cast()
    }

    fn set_owner(&self, inner: *const GreenPoolInner) {
        self.owner.store(inner as usize, Ordering::Release);
    }

    fn current_context(&self) -> Result<CurrentGreenContext, FiberError> {
        let inner = self.owner.load(Ordering::Acquire) as *const GreenPoolInner;
        if inner.is_null() {
            return Err(FiberError::state_conflict());
        }

        Ok(CurrentGreenContext {
            inner,
            slot_index: self.slot_index,
            id: self.current_id()?,
        })
    }

    fn set_yield_action(&self, action: CurrentGreenYieldAction) -> Result<(), FiberError> {
        self.yield_action
            .with(|yield_action| *yield_action = action)?;
        Ok(())
    }

    fn enter_cooperative_lock(
        &self,
        rank: Option<u16>,
        span: Option<CooperativeExclusionSpan>,
    ) -> Result<CooperativeGreenLockToken, SyncError> {
        let depth = self.cooperative_lock_depth.load(Ordering::Acquire);
        if depth >= MAX_COOPERATIVE_LOCK_NESTING {
            return Err(SyncError::overflow());
        }

        let rank_value = rank.unwrap_or(UNRANKED_COOPERATIVE_LOCK);
        if depth != 0 {
            let current_rank = self.cooperative_lock_ranks[depth - 1].load(Ordering::Acquire);
            if current_rank != UNRANKED_COOPERATIVE_LOCK
                && rank_value != UNRANKED_COOPERATIVE_LOCK
                && rank_value <= current_rank
            {
                return Err(SyncError::invalid());
            }
        }

        self.cooperative_lock_ranks[depth].store(rank_value, Ordering::Release);
        self.cooperative_exclusion_spans[depth].store(
            span.map_or(NO_COOPERATIVE_EXCLUSION_SPAN, CooperativeExclusionSpan::get),
            Ordering::Release,
        );
        self.cooperative_lock_depth
            .store(depth + 1, Ordering::Release);
        self.rebuild_cooperative_exclusion_summary_tree(depth + 1);
        Ok(CooperativeGreenLockToken {
            slot: core::ptr::from_ref(self).cast(),
            depth_index: depth,
        })
    }

    fn exit_cooperative_lock(&self, depth_index: usize) {
        let previous = self.cooperative_lock_depth.load(Ordering::Acquire);
        assert!(
            previous > 0,
            "cooperative green lock depth underflow indicates unbalanced guard bookkeeping"
        );
        assert_eq!(
            previous,
            depth_index + 1,
            "cooperative green locks should release in reverse acquisition order"
        );
        self.cooperative_lock_ranks[depth_index]
            .store(UNRANKED_COOPERATIVE_LOCK, Ordering::Release);
        self.cooperative_exclusion_spans[depth_index]
            .store(NO_COOPERATIVE_EXCLUSION_SPAN, Ordering::Release);
        self.cooperative_lock_depth
            .store(depth_index, Ordering::Release);
        self.rebuild_cooperative_exclusion_summary_tree(depth_index);
    }

    fn reset_cooperative_lock_depth(&self) {
        self.cooperative_lock_depth.store(0, Ordering::Release);
        for rank in &self.cooperative_lock_ranks {
            rank.store(UNRANKED_COOPERATIVE_LOCK, Ordering::Release);
        }
        for span in &self.cooperative_exclusion_spans {
            span.store(NO_COOPERATIVE_EXCLUSION_SPAN, Ordering::Release);
        }
        for word in &self.cooperative_exclusion_summary_leaf {
            word.store(0, Ordering::Release);
        }
        self.cooperative_exclusion_summary_root
            .store(0, Ordering::Release);
        self.cooperative_exclusion_summary_overflow
            .store(false, Ordering::Release);
    }

    fn cooperative_lock_depth(&self) -> usize {
        self.cooperative_lock_depth.load(Ordering::Acquire)
    }

    fn copy_active_exclusion_spans(&self, output: &mut [CooperativeExclusionSpan]) -> usize {
        let depth = self.cooperative_lock_depth();
        let mut written = 0;
        for index in 0..depth {
            if written >= output.len() {
                break;
            }
            let raw = self.cooperative_exclusion_spans[index].load(Ordering::Acquire);
            let Some(span) = NonZeroU16::new(raw).map(CooperativeExclusionSpan) else {
                continue;
            };
            output[written] = span;
            written += 1;
        }
        written
    }

    fn rebuild_cooperative_exclusion_summary_tree(&self, depth: usize) {
        let mut leaf = [0_u32; ACTIVE_COOPERATIVE_EXCLUSION_FAST_LEAF_WORDS];
        let mut root = 0_u32;
        let mut overflow = false;

        for index in 0..depth {
            let raw = self.cooperative_exclusion_spans[index].load(Ordering::Acquire);
            let Some(span) = NonZeroU16::new(raw) else {
                continue;
            };
            let span_index = usize::from(span.get() - 1);
            if span_index >= ACTIVE_COOPERATIVE_EXCLUSION_FAST_SPAN_CAPACITY {
                overflow = true;
                continue;
            }
            let word_index = span_index / COOPERATIVE_EXCLUSION_TREE_WORD_BITS;
            let bit = 1_u32 << (span_index % COOPERATIVE_EXCLUSION_TREE_WORD_BITS);
            leaf[word_index] |= bit;
            root |= 1_u32 << word_index;
        }

        for (index, word) in leaf.into_iter().enumerate() {
            self.cooperative_exclusion_summary_leaf[index].store(word, Ordering::Release);
        }
        self.cooperative_exclusion_summary_root
            .store(root, Ordering::Release);
        self.cooperative_exclusion_summary_overflow
            .store(overflow, Ordering::Release);
    }

    fn exclusion_summary_tree_allows(&self, tree: &CooperativeExclusionSummaryTree) -> bool {
        if tree.leaf_words.is_empty() {
            return true;
        }

        if !self
            .cooperative_exclusion_summary_overflow
            .load(Ordering::Acquire)
            && tree.leaf_words.len() <= ACTIVE_COOPERATIVE_EXCLUSION_FAST_LEAF_WORDS
        {
            match tree.summary_levels {
                [] if tree.leaf_words.len() == 1 => {
                    return self.cooperative_exclusion_summary_leaf[0].load(Ordering::Acquire)
                        & tree.leaf_words[0]
                        == 0;
                }
                [root] if root.len() == 1 => {
                    let overlap = self
                        .cooperative_exclusion_summary_root
                        .load(Ordering::Acquire)
                        & root[0];
                    if overlap == 0 {
                        return true;
                    }
                    let mut bits = overlap;
                    while bits != 0 {
                        let leaf_index = bits.trailing_zeros() as usize;
                        if self.cooperative_exclusion_summary_leaf[leaf_index]
                            .load(Ordering::Acquire)
                            & tree.leaf_words[leaf_index]
                            != 0
                        {
                            return false;
                        }
                        bits &= bits - 1;
                    }
                    return true;
                }
                _ => {}
            }
        }

        let depth = self.cooperative_lock_depth();
        for index in 0..depth {
            let raw = self.cooperative_exclusion_spans[index].load(Ordering::Acquire);
            let Some(active) = NonZeroU16::new(raw).map(CooperativeExclusionSpan) else {
                continue;
            };
            if tree.contains(active) {
                return false;
            }
        }
        true
    }

    fn take_yield_action(&self) -> Result<CurrentGreenYieldAction, FiberError> {
        self.yield_action
            .with(|yield_action| core::mem::replace(yield_action, CurrentGreenYieldAction::Requeue))
    }

    fn assign<F>(
        &self,
        id: u64,
        carrier: usize,
        lease: Option<FiberStackLease>,
        task: FiberTaskAttributes,
        job: F,
    ) -> Result<(), FiberError>
    where
        F: FnOnce() + Send + 'static,
    {
        self.completed.with_ref(|completed| {
            if let Some(semaphore) = completed.as_ref() {
                while semaphore.try_acquire().map_err(fiber_error_from_sync)? {}
            }
            Ok::<(), FiberError>(())
        })??;

        self.record.with(|record| {
            if record.allocated {
                return Err(FiberError::state_conflict());
            }

            record.job.clear();
            record.result.clear();
            record.job.store(job)?;
            record.allocated = true;
            record.id = id;
            record.carrier = carrier;
            record.stack_pool_index = lease.map_or(0, |reserved| reserved.pool_index);
            record.stack_slot = lease.map_or(0, |reserved| reserved.slot_index);
            record.stack_class = lease.map_or(task.stack_class, |reserved| reserved.class);
            record.priority = task.priority;
            record.yield_budget = task.yield_budget;
            record.execution = task.execution;
            record.fiber = None;
            record.state = GreenTaskState::Queued;
            Ok(())
        })??;
        self.completion_published.store(false, Ordering::Release);
        self.completion_waiters.store(0, Ordering::Release);
        self.handle_refs.store(1, Ordering::Release);
        self.reset_cooperative_lock_depth();
        Ok(())
    }

    fn clone_handle(&self) {
        self.handle_refs.fetch_add(1, Ordering::AcqRel);
    }

    fn current_id(&self) -> Result<u64, FiberError> {
        self.record.with_ref(|record| {
            if !record.allocated {
                return Err(FiberError::state_conflict());
            }
            Ok(record.id)
        })?
    }

    fn priority(&self) -> Result<FiberTaskPriority, FiberError> {
        self.record.with_ref(|record| {
            if !record.allocated {
                return Err(FiberError::state_conflict());
            }
            Ok(record.priority)
        })?
    }

    fn execution(&self) -> Result<FiberTaskExecution, FiberError> {
        self.record.with_ref(|record| {
            if !record.allocated {
                return Err(FiberError::state_conflict());
            }
            Ok(record.execution)
        })?
    }

    fn execution_for(&self, id: u64) -> Result<FiberTaskExecution, FiberError> {
        self.record.with_ref(|record| {
            if !Self::matches_id(record, id) {
                return Err(FiberError::state_conflict());
            }
            Ok(record.execution)
        })?
    }

    fn admission_for(&self, id: u64) -> Result<FiberTaskAdmission, FiberError> {
        self.record.with_ref(|record| {
            if !Self::matches_id(record, id) {
                return Err(FiberError::state_conflict());
            }
            Ok(FiberTaskAdmission {
                carrier: record.carrier,
                stack_class: record.stack_class,
                priority: record.priority,
                yield_budget: record.yield_budget,
                execution: record.execution,
            })
        })?
    }

    const fn matches_id(record: &GreenTaskRecord, id: u64) -> bool {
        record.allocated && record.id == id
    }

    fn install_fiber(&self, id: u64, fiber: Fiber) -> Result<(), FiberError> {
        self.record.with(|record| {
            if !Self::matches_id(record, id) {
                return Err(FiberError::state_conflict());
            }
            record.fiber = Some(fiber);
            Ok(())
        })??;
        Ok(())
    }

    fn clear_fiber(&self, id: u64) -> Result<(), FiberError> {
        self.record.with(|record| {
            if !Self::matches_id(record, id) {
                return Err(FiberError::state_conflict());
            }
            record.fiber = None;
            Ok(())
        })??;
        Ok(())
    }

    fn stack_location(&self, id: u64) -> Result<(usize, usize), FiberError> {
        self.record.with_ref(|record| {
            if !Self::matches_id(record, id) {
                return Err(FiberError::state_conflict());
            }
            Ok((record.stack_pool_index, record.stack_slot))
        })?
    }

    fn assignment(&self) -> Result<Option<(u64, usize)>, FiberError> {
        self.record.with_ref(|record| {
            if !record.allocated {
                return Ok(None);
            }
            Ok(Some((record.id, record.carrier)))
        })?
    }

    fn reassign_carrier(&self, id: u64, carrier: usize) -> Result<(), FiberError> {
        self.record.with(|record| {
            if !Self::matches_id(record, id) {
                return Err(FiberError::state_conflict());
            }
            if matches!(
                record.state,
                GreenTaskState::Running | GreenTaskState::Waiting | GreenTaskState::Finishing
            ) {
                return Err(FiberError::state_conflict());
            }
            record.carrier = carrier;
            Ok(())
        })??;
        Ok(())
    }

    fn state(&self, id: u64) -> Result<GreenTaskState, FiberError> {
        self.record.with_ref(|record| {
            if !Self::matches_id(record, id) {
                return Err(FiberError::state_conflict());
            }
            Ok(record.state)
        })?
    }

    fn is_finished(&self, id: u64) -> Result<bool, FiberError> {
        Ok(is_terminal_task_state(self.state(id)?)
            && self.completion_published.load(Ordering::Acquire))
    }

    fn ensure_completion_semaphore(&self) -> Result<*const Semaphore, FiberError> {
        self.completed.with(|completed| {
            if completed.is_none() {
                *completed = Some(Semaphore::new(0, 1).map_err(fiber_error_from_sync)?);
            }
            Ok::<*const Semaphore, FiberError>(core::ptr::from_ref(
                completed.as_ref().ok_or_else(FiberError::state_conflict)?,
            ))
        })?
    }

    fn wait_until_terminal(&self, id: u64) -> Result<GreenTaskState, FiberError> {
        let waited = if self.is_finished(id)? {
            false
        } else {
            let semaphore = self.ensure_completion_semaphore()?;
            self.completion_waiters.fetch_add(1, Ordering::AcqRel);
            if self.is_finished(id)? {
                self.completion_waiters.fetch_sub(1, Ordering::AcqRel);
                false
            } else {
                unsafe { &*semaphore }
                    .acquire()
                    .map_err(fiber_error_from_sync)?;
                true
            }
        };

        let state = self.state(id)?;
        if waited {
            let remaining_waiters = self
                .completion_waiters
                .fetch_sub(1, Ordering::AcqRel)
                .saturating_sub(1);
            if is_terminal_task_state(state) && remaining_waiters != 0 {
                self.completed.with_ref(|completed| {
                    completed
                        .as_ref()
                        .ok_or_else(FiberError::state_conflict)?
                        .release(1)
                        .map_err(fiber_error_from_sync)
                })??;
            }
        }
        Ok(state)
    }

    fn set_state(&self, id: u64, state: GreenTaskState) -> Result<(), FiberError> {
        self.record.with(|record| {
            if !Self::matches_id(record, id) {
                return Err(FiberError::state_conflict());
            }
            record.state = state;
            Ok(())
        })??;
        Ok(())
    }

    fn signal_completed(&self, id: u64) -> Result<(), FiberError> {
        self.record.with_ref(|record| {
            if !Self::matches_id(record, id) || !is_terminal_task_state(record.state) {
                #[cfg(feature = "std")]
                if std::env::var_os("FUSION_TRACE_CARRIER_ERRORS").is_some() {
                    std::eprintln!(
                        "fusion-std signal_completed mismatch: slot_index={} expected_id={} actual_id={} allocated={} state={:?}",
                        self.slot_index,
                        id,
                        record.id,
                        record.allocated,
                        record.state,
                    );
                }
                return Err(FiberError::state_conflict());
            }
            Ok(())
        })??;
        let release = self.completed.with_ref(|completed| {
            completed
                .as_ref()
                .ok_or_else(FiberError::state_conflict)?
                .release(1)
                .map_err(fiber_error_from_sync)
        })?;
        self.completion_published.store(true, Ordering::Release);
        match release {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == FiberError::state_conflict().kind() => Ok(()),
            Err(error) => Err(error),
        }
    }

    fn resume(&self, id: u64) -> Result<FiberYield, FiberError> {
        let mut fiber = {
            self.record.with(|record| {
                if !Self::matches_id(record, id) {
                    return Err(FiberError::state_conflict());
                }
                record.fiber.take().ok_or_else(FiberError::state_conflict)
            })??
        };

        match fiber.resume() {
            Ok(FiberYield::Yielded) => {
                self.record.with(|record| {
                    if !Self::matches_id(record, id) {
                        return Err(FiberError::state_conflict());
                    }
                    record.fiber = Some(fiber);
                    Ok(())
                })??;
                Ok(FiberYield::Yielded)
            }
            Ok(FiberYield::Completed(result)) => Ok(FiberYield::Completed(result)),
            Err(error) => Err(error),
        }
    }

    fn take_job_runner(&self, id: u64) -> Result<InlineGreenJobRunner, FiberError> {
        self.record.with(|record| {
            if !Self::matches_id(record, id) {
                return Err(FiberError::state_conflict());
            }
            record.job.take_runner()
        })?
    }

    fn store_output<T: 'static>(&self, id: u64, value: T) -> Result<(), FiberError> {
        self.record.with(|record| {
            if !Self::matches_id(record, id) {
                return Err(FiberError::state_conflict());
            }
            record.result.store(value)
        })?
    }

    fn take_output<T: 'static>(&self, id: u64) -> Result<T, FiberError> {
        self.record.with(|record| {
            if !Self::matches_id(record, id) {
                return Err(FiberError::state_conflict());
            }
            record.result.take::<T>()
        })?
    }

    fn force_recycle(&self, id: u64) -> Result<bool, FiberError> {
        let recycled = self.record.with(|record| {
            if !Self::matches_id(record, id) {
                return Ok::<bool, FiberError>(false);
            }
            record.job.clear();
            record.result.clear();
            record.fiber = None;
            record.allocated = false;
            record.id = 0;
            record.carrier = 0;
            record.stack_pool_index = 0;
            record.stack_slot = 0;
            record.stack_class = FiberStackClass::MIN;
            record.priority = FiberTaskPriority::DEFAULT;
            record.yield_budget = None;
            record.execution = FiberTaskExecution::Fiber;
            record.state = GreenTaskState::Completed;
            Ok::<bool, FiberError>(true)
        })??;
        if recycled {
            #[cfg(feature = "std")]
            if std::env::var_os("FUSION_TRACE_CARRIER_ERRORS").is_some() {
                std::eprintln!(
                    "fusion-std force_recycle: slot_index={} id={}",
                    self.slot_index,
                    id
                );
            }
            self.completion_published.store(false, Ordering::Release);
            self.completion_waiters.store(0, Ordering::Release);
            self.handle_refs.store(0, Ordering::Release);
            self.reset_cooperative_lock_depth();
        }
        Ok(recycled)
    }

    fn try_recycle(&self, id: u64) -> Result<bool, FiberError> {
        let recycled = self.record.with(|record| {
            if !Self::matches_id(record, id) {
                return Ok::<bool, FiberError>(false);
            }
            if !is_terminal_task_state(record.state)
                || !self.completion_published.load(Ordering::Acquire)
                || self.handle_refs.load(Ordering::Acquire) != 0
            {
                return Ok::<bool, FiberError>(false);
            }
            record.job.clear();
            record.result.clear();
            record.fiber = None;
            record.allocated = false;
            record.id = 0;
            record.carrier = 0;
            record.stack_pool_index = 0;
            record.stack_slot = 0;
            record.stack_class = FiberStackClass::MIN;
            record.priority = FiberTaskPriority::DEFAULT;
            record.yield_budget = None;
            record.execution = FiberTaskExecution::Fiber;
            record.state = GreenTaskState::Completed;
            Ok::<bool, FiberError>(true)
        })??;
        if recycled {
            #[cfg(feature = "std")]
            if std::env::var_os("FUSION_TRACE_CARRIER_ERRORS").is_some() {
                std::eprintln!(
                    "fusion-std try_recycle: slot_index={} id={}",
                    self.slot_index,
                    id
                );
            }
            self.completion_published.store(false, Ordering::Release);
            self.completion_waiters.store(0, Ordering::Release);
            self.reset_cooperative_lock_depth();
        }
        Ok(recycled)
    }

    fn begin_run(&self) -> Result<(u64, Option<Duration>, FiberTaskExecution), FiberError> {
        self.record.with(|record| {
            if !record.allocated {
                return Err(FiberError::state_conflict());
            }
            let task_id = record.id;
            let yield_budget = record.yield_budget;
            let execution = record.execution;
            record.state = GreenTaskState::Running;
            Ok((task_id, yield_budget, execution))
        })?
    }

    fn settle_terminal_state(&self, id: u64, terminal: GreenTaskState) -> Result<(), FiberError> {
        self.record.with(|record| {
            if !Self::matches_id(record, id) {
                return Err(FiberError::state_conflict());
            }
            if !matches!(record.state, GreenTaskState::Failed(_)) {
                record.state = terminal;
            }
            Ok(())
        })??;
        Ok(())
    }

    fn begin_finish(
        &self,
        id: u64,
        terminal: GreenTaskState,
    ) -> Result<GreenTaskState, FiberError> {
        self.record.with(|record| {
            if !Self::matches_id(record, id) {
                return Err(FiberError::state_conflict());
            }
            let resolved = if let GreenTaskState::Failed(error) = record.state {
                GreenTaskState::Failed(error)
            } else {
                terminal
            };
            record.state = GreenTaskState::Finishing;
            Ok(resolved)
        })?
    }
}

#[derive(Debug)]
struct GreenTaskRegistry {
    slots: MetadataSlice<GreenTaskSlot>,
    free: RuntimeCell<MetadataIndexStack>,
}

impl GreenTaskRegistry {
    fn new(
        slots: MetadataSlice<GreenTaskSlot>,
        free_entries: MetadataSlice<usize>,
        fast: bool,
    ) -> Result<Self, FiberError> {
        if slots.is_empty() || slots.len() != free_entries.len() {
            return Err(FiberError::invalid());
        }

        for slot_index in 0..slots.len() {
            unsafe {
                slots.write(slot_index, GreenTaskSlot::new(slot_index, fast)?)?;
            }
        }

        Ok(Self {
            free: RuntimeCell::new(
                fast,
                MetadataIndexStack::with_prefix(free_entries, slots.len())?,
            ),
            slots,
        })
    }

    fn reserve_slot(&self) -> Result<usize, FiberError> {
        self.free
            .with(|free| free.pop().ok_or_else(FiberError::resource_exhausted))?
    }

    fn initialize_owner(&self, inner: *const GreenPoolInner) {
        for slot in &*self.slots {
            slot.set_owner(inner);
        }
    }

    fn assign_job<F>(
        &self,
        slot_index: usize,
        id: u64,
        carrier: usize,
        lease: Option<FiberStackLease>,
        task: FiberTaskAttributes,
        job: F,
    ) -> Result<(), FiberError>
    where
        F: FnOnce() + Send + 'static,
    {
        let slot = &self.slots[slot_index];
        slot.assign(id, carrier, lease, task, job)
    }

    fn recycle_slot(&self, slot_index: usize) -> Result<(), FiberError> {
        self.free.with(|free| free.push(slot_index))?
    }

    fn slot(&self, slot_index: usize) -> Result<&GreenTaskSlot, FiberError> {
        self.slots.get(slot_index).ok_or_else(FiberError::invalid)
    }

    fn slot_context(&self, slot_index: usize) -> Result<*mut (), FiberError> {
        Ok(self.slot(slot_index)?.context_ptr())
    }

    fn clone_handle(&self, slot_index: usize) -> Result<(), FiberError> {
        self.slot(slot_index)?.clone_handle();
        Ok(())
    }

    fn current_id(&self, slot_index: usize) -> Result<u64, FiberError> {
        self.slot(slot_index)?.current_id()
    }

    fn priority(&self, slot_index: usize) -> Result<FiberTaskPriority, FiberError> {
        self.slot(slot_index)?.priority()
    }

    fn execution(&self, slot_index: usize) -> Result<FiberTaskExecution, FiberError> {
        self.slot(slot_index)?.execution()
    }

    fn execution_for(&self, slot_index: usize, id: u64) -> Result<FiberTaskExecution, FiberError> {
        self.slot(slot_index)?.execution_for(id)
    }

    fn admission_for(&self, slot_index: usize, id: u64) -> Result<FiberTaskAdmission, FiberError> {
        self.slot(slot_index)?.admission_for(id)
    }

    fn install_fiber(&self, slot_index: usize, id: u64, fiber: Fiber) -> Result<(), FiberError> {
        self.slot(slot_index)?.install_fiber(id, fiber)
    }

    fn clear_fiber(&self, slot_index: usize, id: u64) -> Result<(), FiberError> {
        self.slot(slot_index)?.clear_fiber(id)
    }

    fn stack_location(&self, slot_index: usize, id: u64) -> Result<(usize, usize), FiberError> {
        self.slot(slot_index)?.stack_location(id)
    }

    fn assignment(&self, slot_index: usize) -> Result<Option<(u64, usize)>, FiberError> {
        self.slot(slot_index)?.assignment()
    }

    fn reassign_carrier(
        &self,
        slot_index: usize,
        id: u64,
        carrier: usize,
    ) -> Result<(), FiberError> {
        self.slot(slot_index)?.reassign_carrier(id, carrier)
    }

    fn state(&self, slot_index: usize, id: u64) -> Result<GreenTaskState, FiberError> {
        self.slot(slot_index)?.state(id)
    }

    fn is_finished(&self, slot_index: usize, id: u64) -> Result<bool, FiberError> {
        self.slot(slot_index)?.is_finished(id)
    }

    fn wait_until_terminal(
        &self,
        slot_index: usize,
        id: u64,
    ) -> Result<GreenTaskState, FiberError> {
        self.slot(slot_index)?.wait_until_terminal(id)
    }

    fn set_state(
        &self,
        slot_index: usize,
        id: u64,
        state: GreenTaskState,
    ) -> Result<(), FiberError> {
        self.slot(slot_index)?.set_state(id, state)
    }

    fn signal_completed(&self, slot_index: usize, id: u64) -> Result<(), FiberError> {
        self.slot(slot_index)?.signal_completed(id)
    }

    fn resume(&self, slot_index: usize, id: u64) -> Result<FiberYield, FiberError> {
        self.slot(slot_index)?.resume(id)
    }

    fn take_output<T: 'static>(&self, slot_index: usize, id: u64) -> Result<T, FiberError> {
        self.slot(slot_index)?.take_output::<T>(id)
    }

    fn release_handle(&self, slot_index: usize, id: u64) -> Result<(), FiberError> {
        let slot = self.slot(slot_index)?;
        let previous = slot
            .handle_refs
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current: usize| {
                current.checked_sub(1)
            })
            .map_err(|_| FiberError::state_conflict())?;
        #[cfg(feature = "std")]
        if std::env::var_os("FUSION_TRACE_CARRIER_ERRORS").is_some() {
            std::eprintln!(
                "fusion-std release_handle: slot_index={slot_index} id={id} previous_refs={previous}"
            );
        }
        if previous == 1 && slot.try_recycle(id)? {
            self.recycle_slot(slot_index)?;
        }
        Ok(())
    }

    fn try_reclaim(&self, slot_index: usize, id: u64) -> Result<(), FiberError> {
        if self.slot(slot_index)?.try_recycle(id)? {
            self.recycle_slot(slot_index)?;
        }
        Ok(())
    }

    fn abandon(&self, slot_index: usize, id: u64) -> Result<(), FiberError> {
        if self.slot(slot_index)?.force_recycle(id)? {
            self.recycle_slot(slot_index)?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
struct CurrentGreenContext {
    inner: *const GreenPoolInner,
    slot_index: usize,
    id: u64,
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
            GreenPoolControlBacking::Owned { control, metadata } => {
                let _ = control.resolved();
                let _ = metadata.resolved();
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
    scheduling: GreenScheduling,
    capacity_policy: CapacityPolicy,
    yield_budget_supported: bool,
    #[cfg(feature = "std")]
    yield_budget_policy: FiberYieldBudgetPolicy,
    shutdown: AtomicBool,
    client_refs: AtomicUsize,
    active: AtomicUsize,
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

/// Opaque public green-thread handle.
#[derive(Debug)]
pub struct GreenHandle<T = ()> {
    id: u64,
    slot_index: usize,
    inner: GreenPoolLease,
    drive_mode: GreenHandleDriveMode,
    _marker: PhantomData<fn() -> T>,
}

impl<T> GreenHandle<T>
where
    T: 'static,
{
    /// Returns the stable green-thread identifier.
    #[must_use]
    pub const fn id(&self) -> u64 {
        self.id
    }

    /// Returns whether the green thread has completed.
    ///
    /// # Errors
    ///
    /// Returns an error if the green-thread state cannot be observed honestly.
    pub fn is_finished(&self) -> Result<bool, FiberError> {
        self.inner.tasks.is_finished(self.slot_index, self.id)
    }

    /// Returns the execution strategy used for this admitted task.
    ///
    /// This is the runtime-facing truth for whether the task became one real stackful fiber or
    /// one admitted inline `NoYield` job.
    ///
    /// # Errors
    ///
    /// Returns an error if the green-thread state cannot be observed honestly.
    pub fn execution(&self) -> Result<FiberTaskExecution, FiberError> {
        self.inner.tasks.execution_for(self.slot_index, self.id)
    }

    /// Returns whether this admitted task is executing inline instead of on one dedicated fiber
    /// stack.
    ///
    /// # Errors
    ///
    /// Returns an error if the green-thread state cannot be observed honestly.
    pub fn runs_inline(&self) -> Result<bool, FiberError> {
        Ok(self.execution()?.is_inline())
    }

    /// Returns the runtime admission snapshot for this task.
    ///
    /// # Errors
    ///
    /// Returns an error if the green-thread state cannot be observed honestly.
    pub fn admission(&self) -> Result<FiberTaskAdmission, FiberError> {
        self.inner.tasks.admission_for(self.slot_index, self.id)
    }

    /// Waits for the green thread to complete.
    ///
    /// # Errors
    ///
    /// Returns the fiber failure that stopped execution, if any.
    pub fn join(self) -> Result<T, FiberError> {
        let state = if let Some(current) = current_green_context() {
            if core::ptr::eq(current.inner, self.inner.as_ptr())
                && current.slot_index == self.slot_index
                && current.id == self.id
            {
                return Err(FiberError::state_conflict());
            }
            loop {
                let state = self.inner.tasks.state(self.slot_index, self.id)?;
                if is_terminal_task_state(state) {
                    break state;
                }
                ensure_current_green_handoff_unlocked()?;
                system_yield_now()?;
            }
        } else {
            match self.drive_mode {
                GreenHandleDriveMode::CarrierPool => self
                    .inner
                    .tasks
                    .wait_until_terminal(self.slot_index, self.id)?,
                GreenHandleDriveMode::CurrentThread => loop {
                    let state = self.inner.tasks.state(self.slot_index, self.id)?;
                    if is_terminal_task_state(state) {
                        break state;
                    }
                    if !drive_current_pool_once(&self.inner)? {
                        return Err(FiberError::state_conflict());
                    }
                },
            }
        };

        match state {
            GreenTaskState::Completed if size_of::<T>() == 0 => {
                Ok(unsafe { MaybeUninit::<T>::zeroed().assume_init() })
            }
            GreenTaskState::Completed => {
                self.inner.tasks.take_output::<T>(self.slot_index, self.id)
            }
            GreenTaskState::Failed(error) => Err(error),
            GreenTaskState::Queued
            | GreenTaskState::Running
            | GreenTaskState::Yielded
            | GreenTaskState::Finishing
            | GreenTaskState::Waiting => Err(FiberError::state_conflict()),
        }
    }
}

impl GreenHandle<()> {
    /// Attempts to clone one unit-result green-thread handle.
    ///
    /// # Errors
    ///
    /// Returns an error when the underlying green-pool root cannot be retained honestly.
    pub fn try_clone(&self) -> Result<Self, FiberError> {
        let inner = self.inner.try_clone()?;
        self.inner.tasks.clone_handle(self.slot_index)?;
        Ok(Self {
            id: self.id,
            slot_index: self.slot_index,
            inner,
            drive_mode: self.drive_mode,
            _marker: PhantomData,
        })
    }
}

impl<T> Drop for GreenHandle<T> {
    fn drop(&mut self) {
        let _ = self.inner.tasks.release_handle(self.slot_index, self.id);
    }
}

/// Thread-affine current-thread fiber handle.
#[derive(Debug)]
pub struct CurrentFiberHandle<T = ()> {
    inner: GreenHandle<T>,
    _not_send_sync: PhantomData<*mut ()>,
}

impl<T> CurrentFiberHandle<T>
where
    T: 'static,
{
    /// Returns the stable current-thread fiber identifier.
    #[must_use]
    pub const fn id(&self) -> u64 {
        self.inner.id()
    }

    /// Returns whether the fiber has completed.
    ///
    /// # Errors
    ///
    /// Returns an error if the current-thread pool state cannot be observed honestly.
    pub fn is_finished(&self) -> Result<bool, FiberError> {
        self.inner.is_finished()
    }

    /// Returns the execution strategy used for this admitted task.
    ///
    /// # Errors
    ///
    /// Returns an error if the current-thread pool state cannot be observed honestly.
    pub fn execution(&self) -> Result<FiberTaskExecution, FiberError> {
        self.inner.execution()
    }

    /// Returns whether this admitted task is executing inline instead of on one dedicated fiber
    /// stack.
    ///
    /// # Errors
    ///
    /// Returns an error if the current-thread pool state cannot be observed honestly.
    pub fn runs_inline(&self) -> Result<bool, FiberError> {
        self.inner.runs_inline()
    }

    /// Returns the runtime admission snapshot for this task.
    ///
    /// # Errors
    ///
    /// Returns an error if the current-thread pool state cannot be observed honestly.
    pub fn admission(&self) -> Result<FiberTaskAdmission, FiberError> {
        self.inner.admission()
    }

    /// Waits for the fiber to complete by manually driving the current-thread pool.
    ///
    /// # Errors
    ///
    /// Returns the fiber failure that stopped execution, if any.
    pub fn join(self) -> Result<T, FiberError> {
        self.inner.join()
    }
}

impl CurrentFiberHandle<()> {
    /// Attempts to clone one unit-result current-thread fiber handle.
    ///
    /// # Errors
    ///
    /// Returns an error when the underlying current-thread pool root cannot be retained honestly.
    pub fn try_clone(&self) -> Result<Self, FiberError> {
        Ok(Self {
            inner: self.inner.try_clone()?,
            _not_send_sync: PhantomData,
        })
    }
}

/// Public green-thread pool wrapper.
#[derive(Debug)]
pub struct GreenPool {
    inner: GreenPoolLease,
}

#[derive(Debug)]
struct SpawnReservation {
    lease: Option<FiberStackLease>,
    id: u64,
    carrier: usize,
    slot_index: usize,
    context: *mut (),
}

#[cfg(feature = "std")]
#[allow(clippy::trivially_copy_pass_by_ref)]
fn trace_spawn_failure(stage: &str, slot_index: Option<usize>, error: &FiberError) {
    if std::env::var_os("FUSION_TRACE_SPAWN_FAILURES").is_none() {
        return;
    }
    std::eprintln!(
        "fusion-std spawn failure: stage={stage} slot_index={slot_index:?} kind={:?}",
        error.kind()
    );
}

#[cfg(not(feature = "std"))]
#[allow(clippy::trivially_copy_pass_by_ref)]
fn trace_spawn_failure(_stage: &str, _slot_index: Option<usize>, _error: &FiberError) {}

#[cfg(feature = "std")]
#[allow(clippy::trivially_copy_pass_by_ref)]
fn trace_carrier_failure(stage: &str, carrier_index: usize, error: &FiberError) {
    if std::env::var_os("FUSION_TRACE_CARRIER_ERRORS").is_none() {
        return;
    }
    std::eprintln!(
        "fusion-std carrier failure: stage={stage} carrier_index={carrier_index} kind={:?}",
        error.kind()
    );
}

#[cfg(not(feature = "std"))]
#[allow(clippy::trivially_copy_pass_by_ref)]
fn trace_carrier_failure(_stage: &str, _carrier_index: usize, _error: &FiberError) {}

const fn fiber_error_from_resource(error: ResourceError) -> FiberError {
    match error.kind {
        ResourceErrorKind::UnsupportedRequest | ResourceErrorKind::UnsupportedOperation => {
            FiberError::unsupported()
        }
        ResourceErrorKind::OutOfMemory => FiberError::resource_exhausted(),
        ResourceErrorKind::SynchronizationFailure(_)
        | ResourceErrorKind::InvalidRequest
        | ResourceErrorKind::ContractViolation
        | ResourceErrorKind::InvalidRange
        | ResourceErrorKind::Platform(_) => FiberError::invalid(),
    }
}

fn apply_fiber_sizing_strategy_bytes(
    bytes: usize,
    strategy: RuntimeSizingStrategy,
) -> Result<usize, FiberError> {
    strategy
        .apply_bytes(bytes)
        .ok_or_else(FiberError::resource_exhausted)
}

fn apply_fiber_sizing_strategy_non_zero(
    bytes: NonZeroUsize,
    strategy: RuntimeSizingStrategy,
) -> Result<NonZeroUsize, FiberError> {
    let bytes = apply_fiber_sizing_strategy_bytes(bytes.get(), strategy)?;
    NonZeroUsize::new(bytes).ok_or_else(FiberError::invalid)
}

fn apply_fiber_sizing_strategy_backing(
    backing: FiberStackBacking,
    strategy: RuntimeSizingStrategy,
) -> Result<FiberStackBacking, FiberError> {
    match backing {
        FiberStackBacking::Fixed { stack_size } => Ok(FiberStackBacking::Fixed {
            stack_size: apply_fiber_sizing_strategy_non_zero(stack_size, strategy)?,
        }),
        FiberStackBacking::Elastic {
            initial_size,
            max_size,
        } => {
            let initial_size = apply_fiber_sizing_strategy_non_zero(initial_size, strategy)?;
            let max_size = apply_fiber_sizing_strategy_non_zero(max_size, strategy)?;
            if initial_size.get() > max_size.get() {
                return Err(FiberError::invalid());
            }
            Ok(FiberStackBacking::Elastic {
                initial_size,
                max_size,
            })
        }
    }
}

fn apply_fiber_backing_request(
    request: FiberPoolBackingRequest,
    strategy: RuntimeSizingStrategy,
) -> Result<FiberPoolBackingRequest, FiberError> {
    Ok(FiberPoolBackingRequest {
        bytes: apply_fiber_sizing_strategy_bytes(request.bytes, strategy)?,
        align: request.align,
    })
}

fn task_attributes_from_stack_bytes<const STACK_BYTES: usize>()
-> Result<FiberTaskAttributes, FiberError> {
    let stack_bytes = NonZeroUsize::new(STACK_BYTES).ok_or_else(FiberError::invalid)?;
    FiberTaskAttributes::from_stack_bytes(stack_bytes, FiberTaskPriority::DEFAULT)
}

fn reserve_spawn_slot_for(
    inner: &GreenPoolLease,
    task: FiberTaskAttributes,
) -> Result<SpawnReservation, FiberError> {
    if task.yield_budget.is_some() && !inner.yield_budget_supported {
        return Err(FiberError::unsupported());
    }
    #[cfg(feature = "std")]
    ensure_yield_budget_watchdog_started(inner, task)?;
    if task.execution.requires_fiber() && !inner.stacks.supports_task_class(task.stack_class) {
        return Err(FiberError::unsupported());
    }
    loop {
        let active = inner.active.load(Ordering::Acquire);
        if active >= inner.stacks.total_capacity() {
            return Err(FiberError::resource_exhausted());
        }
        if inner
            .active
            .compare_exchange(active, active + 1, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            break;
        }
    }

    let lease = if task.execution.requires_fiber() {
        match inner.stacks.acquire(task) {
            Ok(lease) => Some(lease),
            Err(error) => {
                trace_spawn_failure("stacks.acquire", None, &error);
                inner.active.fetch_sub(1, Ordering::AcqRel);
                return Err(error);
            }
        }
    } else {
        None
    };

    let id = inner.next_id.fetch_add(1, Ordering::AcqRel) as u64;
    let carrier = inner.next_carrier.fetch_add(1, Ordering::AcqRel) % inner.carriers.len();
    let slot_index = match inner.tasks.reserve_slot() {
        Ok(slot_index) => slot_index,
        Err(error) => {
            trace_spawn_failure("tasks.reserve_slot", None, &error);
            if let Some(lease) = lease {
                let _ = inner.stacks.release(lease.pool_index, lease.slot_index);
            }
            inner.active.fetch_sub(1, Ordering::AcqRel);
            return Err(error);
        }
    };

    let context = match inner.tasks.slot_context(slot_index) {
        Ok(context) => context,
        Err(error) => {
            trace_spawn_failure("tasks.slot_context", Some(slot_index), &error);
            let _ = inner.tasks.abandon(slot_index, id);
            if let Some(lease) = lease {
                let _ = inner.stacks.release(lease.pool_index, lease.slot_index);
            }
            inner.active.fetch_sub(1, Ordering::AcqRel);
            return Err(error);
        }
    };

    Ok(SpawnReservation {
        lease,
        id,
        carrier,
        slot_index,
        context,
    })
}

fn cleanup_failed_spawn_for(inner: &GreenPoolLease, reservation: &SpawnReservation) {
    let _ = inner.tasks.abandon(reservation.slot_index, reservation.id);
    if let Some(lease) = reservation.lease {
        let _ = inner.stacks.release(lease.pool_index, lease.slot_index);
    }
    inner.active.fetch_sub(1, Ordering::AcqRel);
}

fn spawn_on_lease<F, T>(
    inner: &GreenPoolLease,
    task: FiberTaskAttributes,
    job: F,
    signal: bool,
    drive_mode: GreenHandleDriveMode,
) -> Result<GreenHandle<T>, FiberError>
where
    F: FnOnce() -> T + Send + 'static,
    T: 'static,
{
    if !InlineGreenResultStorage::supports::<T>() {
        return Err(FiberError::unsupported());
    }

    let reservation = reserve_spawn_slot_for(inner, task)?;
    let slot_addr = reservation.context as usize;
    let wrapped = move || {
        let output = generated_closure_task_root(job);
        if size_of::<T>() == 0 {
            return;
        }
        let slot = unsafe { &*(slot_addr as *const GreenTaskSlot) };
        if let Ok(id) = slot.current_id()
            && slot.store_output(id, output).is_err()
        {
            let _ = slot.set_state(id, GreenTaskState::Failed(FiberError::state_conflict()));
        }
    };

    if let Err(error) = inner.tasks.assign_job(
        reservation.slot_index,
        reservation.id,
        reservation.carrier,
        reservation.lease,
        task,
        wrapped,
    ) {
        trace_spawn_failure("tasks.assign_job", Some(reservation.slot_index), &error);
        cleanup_failed_spawn_for(inner, &reservation);
        return Err(error);
    }

    if let Some(lease) = reservation.lease {
        if let Err(error) = inner.stacks.attach_slot_identity(
            lease.pool_index,
            lease.slot_index,
            reservation.id,
            reservation.carrier,
            inner.carriers[reservation.carrier].capacity_token(),
        ) {
            trace_spawn_failure(
                "stacks.attach_slot_identity",
                Some(reservation.slot_index),
                &error,
            );
            cleanup_failed_spawn_for(inner, &reservation);
            return Err(error);
        }

        let fiber = match Fiber::new(lease.stack, green_task_entry, reservation.context) {
            Ok(fiber) => fiber,
            Err(error) => {
                trace_spawn_failure("Fiber::new", Some(reservation.slot_index), &error);
                cleanup_failed_spawn_for(inner, &reservation);
                return Err(error);
            }
        };

        if let Err(error) = inner
            .tasks
            .install_fiber(reservation.slot_index, reservation.id, fiber)
        {
            trace_spawn_failure("tasks.install_fiber", Some(reservation.slot_index), &error);
            cleanup_failed_spawn_for(inner, &reservation);
            return Err(error);
        }
    }

    if let Err(error) =
        inner.enqueue_with_signal(reservation.carrier, reservation.slot_index, signal)
    {
        trace_spawn_failure("enqueue_with_signal", Some(reservation.slot_index), &error);
        cleanup_failed_spawn_for(inner, &reservation);
        return Err(error);
    }

    Ok(GreenHandle {
        id: reservation.id,
        slot_index: reservation.slot_index,
        inner: inner.try_clone()?,
        drive_mode,
        _marker: PhantomData,
    })
}

fn drive_current_pool_once(inner: &GreenPoolLease) -> Result<bool, FiberError> {
    let Some(slot_index) = dequeue_ready(inner, 0)? else {
        return Ok(false);
    };
    run_ready_task(inner, 0, slot_index)?;
    Ok(true)
}

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
}

/// Public current-thread fiber pool wrapper for manual same-thread driving.
#[derive(Debug)]
pub struct CurrentFiberPool {
    inner: GreenPoolLease,
    _not_send_sync: PhantomData<*mut ()>,
}

impl CurrentFiberPool {
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
                scheduling: config.scheduling,
                capacity_policy: config.capacity_policy,
                yield_budget_supported: yield_budget_enforcement_supported(),
                #[cfg(feature = "std")]
                yield_budget_policy: config.yield_budget_policy,
                shutdown: AtomicBool::new(false),
                client_refs: AtomicUsize::new(1),
                active: AtomicUsize::new(0),
                next_id: AtomicUsize::new(1),
                next_carrier: AtomicUsize::new(0),
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
            GreenPoolInner {
                support,
                scheduling: config.scheduling,
                capacity_policy: config.capacity_policy,
                yield_budget_supported: yield_budget_enforcement_supported(),
                #[cfg(feature = "std")]
                yield_budget_policy: config.yield_budget_policy,
                shutdown: AtomicBool::new(false),
                client_refs: AtomicUsize::new(1),
                active: AtomicUsize::new(0),
                next_id: AtomicUsize::new(1),
                next_carrier: AtomicUsize::new(0),
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
    #[cfg(not(feature = "critical-safe-generated-contracts"))]
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
    #[cfg(feature = "critical-safe-generated-contracts")]
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
        let task = closure_spawn_task_attributes::<F>(self.inner.stacks.default_task_class()?);
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
        let handle = spawn_on_lease(
            &self.inner,
            task,
            job,
            false,
            GreenHandleDriveMode::CurrentThread,
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
    pub fn spawn_explicit<T>(&self, task: T) -> Result<CurrentFiberHandle<T::Output>, FiberError>
    where
        T: ExplicitFiberTask,
    {
        let attributes = T::task_attributes()?;
        self.validate_task_attributes(attributes)?;
        self.spawn_with_attrs(attributes, move || task.run())
    }

    /// Spawns one explicit fiber task using build-generated stack metadata.
    ///
    /// # Errors
    ///
    /// Returns an error when generated metadata is missing or the task cannot be admitted.
    #[cfg(not(feature = "critical-safe-generated-contracts"))]
    pub fn spawn_generated<T>(&self, task: T) -> Result<CurrentFiberHandle<T::Output>, FiberError>
    where
        T: GeneratedExplicitFiberTask,
    {
        let attributes = T::task_attributes()?;
        self.validate_task_attributes(attributes)?;
        self.spawn_with_attrs(attributes, move || task.run())
    }

    /// Spawns one explicit fiber task using a compile-time generated contract.
    ///
    /// # Errors
    ///
    /// Returns an error when the generated contract is not provisioned by the current pool.
    #[cfg(feature = "critical-safe-generated-contracts")]
    pub fn spawn_generated<T>(&self, task: T) -> Result<CurrentFiberHandle<T::Output>, FiberError>
    where
        T: GeneratedExplicitFiberTask + GeneratedExplicitFiberTaskContract,
    {
        let attributes = generated_explicit_task_contract_attributes::<T>()
            .with_optional_yield_budget(T::YIELD_BUDGET);
        self.validate_task_attributes(attributes)?;
        self.spawn_with_attrs(attributes, move || task.run())
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
        self.spawn_with_attrs(attributes, move || task.run())
    }

    /// Drives at most one ready task segment on the current thread.
    ///
    /// # Errors
    ///
    /// Returns an error when the current pool cannot resume the next ready fiber honestly.
    pub fn drive_once(&self) -> Result<bool, FiberError> {
        drive_current_pool_once(&self.inner)
    }

    /// Drives ready work until the current-thread pool reaches an idle state.
    ///
    /// # Errors
    ///
    /// Returns an error when one resumed task fails dishonestly.
    pub fn run_until_idle(&self) -> Result<usize, FiberError> {
        let mut steps = 0usize;
        while self.drive_once()? {
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

impl Drop for CurrentFiberPool {
    fn drop(&mut self) {
        if self.inner.client_refs.fetch_sub(1, Ordering::AcqRel) == 1 {
            let _ = self.inner.request_shutdown();
        }
    }
}

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
        let summary = system_hardware()
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
        let summary = system_hardware()
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
        let summary = system_hardware()
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
            let summary = system_hardware()
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
            scheduling: config.scheduling,
            capacity_policy: config.capacity_policy,
            yield_budget_supported: yield_budget_enforcement_supported(),
            #[cfg(feature = "std")]
            yield_budget_policy: config.yield_budget_policy,
            shutdown: AtomicBool::new(false),
            client_refs: AtomicUsize::new(1),
            active: AtomicUsize::new(0),
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
        let inner = build_hosted_green_inner(config, carrier_workers)?;
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
    #[cfg(not(feature = "critical-safe-generated-contracts"))]
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
    #[cfg(feature = "critical-safe-generated-contracts")]
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
        let task = closure_spawn_task_attributes::<F>(self.inner.stacks.default_task_class()?);
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
    pub fn spawn_explicit<T>(&self, task: T) -> Result<GreenHandle<T::Output>, FiberError>
    where
        T: ExplicitFiberTask,
    {
        let attributes = T::task_attributes()?;
        self.validate_task_attributes(attributes)?;
        self.spawn_with_attrs(attributes, move || task.run())
    }

    /// Spawns one explicit fiber task using build-generated stack metadata.
    ///
    /// # Errors
    ///
    /// Returns an error when generated metadata is missing or invalid for the task type, or when
    /// ordinary green-task admission fails.
    #[cfg(not(feature = "critical-safe-generated-contracts"))]
    pub fn spawn_generated<T>(&self, task: T) -> Result<GreenHandle<T::Output>, FiberError>
    where
        T: GeneratedExplicitFiberTask,
    {
        let attributes = T::task_attributes()?;
        self.validate_task_attributes(attributes)?;
        self.spawn_with_attrs(attributes, move || task.run())
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
    #[cfg(feature = "critical-safe-generated-contracts")]
    pub fn spawn_generated<T>(&self, task: T) -> Result<GreenHandle<T::Output>, FiberError>
    where
        T: GeneratedExplicitFiberTask + GeneratedExplicitFiberTaskContract,
    {
        let attributes = generated_explicit_task_contract_attributes::<T>()
            .with_optional_yield_budget(T::YIELD_BUDGET);
        self.validate_task_attributes(attributes)?;
        self.spawn_with_attrs(attributes, move || task.run())
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
        self.spawn_with_attrs(attributes, move || task.run())
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
        spawn_on_lease(
            &self.inner,
            task,
            job,
            true,
            GreenHandleDriveMode::CarrierPool,
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
    system_hardware()
        .topology_summary()
        .ok()
        .and_then(select_automatic_carrier_count)
        .filter(|count| *count != 0)
}

#[cfg(feature = "std")]
const fn select_automatic_carrier_count(
    summary: fusion_pal::hal::HardwareTopologySummary,
) -> Option<usize> {
    hosted_carrier_count_from_summary(summary, HostedCarrierCountPolicy::Automatic)
}

#[cfg(feature = "std")]
const fn hosted_carrier_count_from_summary(
    summary: fusion_pal::hal::HardwareTopologySummary,
    policy: HostedCarrierCountPolicy,
) -> Option<usize> {
    match policy {
        HostedCarrierCountPolicy::Automatic => match summary.core_count {
            Some(count) => Some(count),
            None => summary.logical_cpu_count,
        },
        HostedCarrierCountPolicy::VisibleLogicalCpus => summary.logical_cpu_count,
        HostedCarrierCountPolicy::VisibleCores => summary.core_count,
        HostedCarrierCountPolicy::VisiblePackages => summary.package_count,
    }
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
    let slot = unsafe { &*context.cast::<GreenTaskSlot>() };
    let Ok(id) = slot.current_id() else {
        return FiberReturn::new(usize::MAX);
    };

    let runner = match slot.take_job_runner(id) {
        Ok(runner) => runner,
        Err(error) => {
            let _ = slot.set_state(id, GreenTaskState::Failed(error));
            return FiberReturn::new(usize::MAX);
        }
    };

    #[cfg(feature = "std")]
    if run_green_job_contained(runner).is_err() {
        let _ = slot.set_state(id, GreenTaskState::Failed(FiberError::state_conflict()));
        return FiberReturn::new(usize::MAX);
    }

    #[cfg(not(feature = "std"))]
    run_green_job_contained(runner);

    FiberReturn::new(0)
}

fn run_carrier_loop(inner: &GreenPoolInner, carrier_index: usize) -> Result<(), FiberError> {
    if inner.carriers[carrier_index].reactor.is_some() {
        return run_reactor_carrier_loop(inner, carrier_index);
    }

    let _alt_stack = if inner.stacks.requires_signal_handler() {
        Some(install_carrier_signal_stack()?)
    } else {
        None
    };
    loop {
        while let Some(slot_index) = dequeue_ready(inner, carrier_index)? {
            if let Err(error) = run_ready_task(inner, carrier_index, slot_index) {
                trace_carrier_failure("run_carrier_loop.run_ready_task", carrier_index, &error);
                return Err(error);
            }
        }
        if let Some(slot_index) = inner.try_steal_ready(carrier_index)? {
            if let Err(error) = run_ready_task(inner, carrier_index, slot_index) {
                trace_carrier_failure("run_carrier_loop.run_stolen_task", carrier_index, &error);
                return Err(error);
            }
            continue;
        }
        if inner.shutdown.load(Ordering::Acquire) {
            break;
        }
        let carrier = &inner.carriers[carrier_index];
        if let Err(error) = carrier.ready.acquire().map_err(fiber_error_from_sync) {
            trace_carrier_failure("run_carrier_loop.ready.acquire", carrier_index, &error);
            return Err(error);
        }
    }
    Ok(())
}

fn run_reactor_carrier_loop(
    inner: &GreenPoolInner,
    carrier_index: usize,
) -> Result<(), FiberError> {
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
    let (task_id, yield_budget, execution) = match slot.begin_run() {
        Ok(values) => values,
        Err(error) => {
            trace_carrier_failure("run_ready_task.begin_run", carrier_index, &error);
            return Err(error);
        }
    };
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
        trace_carrier_failure("run_ready_task.set_yield_action", carrier_index, &error);
        return Err(error);
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
    let resume = match inner.tasks.resume(slot_index, task_id) {
        Ok(resume) => Ok(resume),
        Err(error) => {
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
        Ok(FiberYield::Yielded) => match take_current_green_yield_action(inner, slot_index)
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
                inner.dispatch_capacity_for_task(slot_index, task_id)?;
                inner.enqueue_with_signal(carrier_index, slot_index, false)?;
            }
            CurrentGreenYieldAction::WaitReadiness { source, interest } => {
                inner.dispatch_capacity_for_task(slot_index, task_id)?;
                if let Err(error) =
                    inner.park_on_readiness(carrier_index, slot_index, task_id, source, interest)
                {
                    inner.finish_task(slot_index, task_id, GreenTaskState::Failed(error))?;
                }
            }
        },
        Ok(FiberYield::Completed(_)) => {
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
    let context = unsafe { &*context.cast::<CarrierLoopContext>() };
    let inner = unsafe { &context.control.as_ref().inner };
    if let Err(_error) = run_carrier_loop(inner, context.carrier_index) {
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

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use crate::sync::Mutex as FusionMutex;
    use fusion_pal::sys::mem::{Address, CachePolicy, MemAdviceCaps, Protect, Region};
    use fusion_sys::mem::resource::{
        BoundMemoryResource,
        BoundResourceSpec,
        MemoryDomain,
        MemoryGeometry,
        MemoryResourceHandle,
        OvercommitPolicy,
        ResourceAttrs,
        ResourceBackingKind,
        ResourceContract,
        ResourceOpSet,
        ResourceRequest,
        ResourceResidencySupport,
        ResourceState,
        ResourceSupport,
        SharingPolicy,
        StateValue,
        VirtualMemoryResource,
    };
    use std::sync::{Arc, Mutex as StdMutex, OnceLock as StdOnceLock};
    use std::vec::Vec;

    fn aligned_bound_resource(len: usize, align: usize) -> MemoryResourceHandle {
        use std::alloc::{Layout, alloc_zeroed};

        let layout = Layout::from_size_align(len, align).expect("aligned test layout should build");
        let ptr = unsafe { alloc_zeroed(layout) };
        assert!(
            !ptr.is_null(),
            "aligned test slab allocation should succeed"
        );
        MemoryResourceHandle::from(
            BoundMemoryResource::new(BoundResourceSpec::new(
                Region {
                    base: Address::new(ptr as usize),
                    len,
                },
                MemoryDomain::StaticRegion,
                ResourceBackingKind::Borrowed,
                ResourceAttrs::ALLOCATABLE | ResourceAttrs::CACHEABLE | ResourceAttrs::COHERENT,
                MemoryGeometry {
                    base_granule: NonZeroUsize::new(1).expect("non-zero granule"),
                    alloc_granule: NonZeroUsize::new(1).expect("non-zero granule"),
                    protect_granule: None,
                    commit_granule: None,
                    lock_granule: None,
                    large_granule: None,
                },
                fusion_sys::mem::resource::AllocatorLayoutPolicy::exact_static(),
                ResourceContract {
                    allowed_protect: Protect::READ | Protect::WRITE,
                    write_xor_execute: true,
                    sharing: SharingPolicy::Private,
                    overcommit: OvercommitPolicy::Disallow,
                    cache_policy: CachePolicy::Default,
                    integrity: None,
                },
                ResourceSupport {
                    protect: Protect::READ | Protect::WRITE,
                    ops: ResourceOpSet::QUERY,
                    advice: MemAdviceCaps::empty(),
                    residency: ResourceResidencySupport::BEST_EFFORT,
                },
                ResourceState::static_state(
                    StateValue::Uniform(Protect::READ | Protect::WRITE),
                    StateValue::Uniform(false),
                    StateValue::Uniform(true),
                ),
            ))
            .expect("aligned bound resource should bind"),
        )
    }

    struct OversizedExplicitTask;

    impl ExplicitFiberTask for OversizedExplicitTask {
        type Output = ();

        const STACK_BYTES: NonZeroUsize =
            NonZeroUsize::new(32 * 1024).expect("non-zero oversized stack");

        fn run(self) -> Self::Output {}
    }

    struct SupportedExplicitTask;

    impl ExplicitFiberTask for SupportedExplicitTask {
        type Output = ();

        const STACK_BYTES: NonZeroUsize =
            NonZeroUsize::new(8 * 1024).expect("non-zero supported stack");

        fn run(self) -> Self::Output {}
    }

    struct SupportedInlineNoYieldTask;

    impl ExplicitFiberTask for SupportedInlineNoYieldTask {
        type Output = u32;

        const STACK_BYTES: NonZeroUsize =
            NonZeroUsize::new(32 * 1024).expect("non-zero oversized inline stack");
        const EXECUTION: FiberTaskExecution = FiberTaskExecution::InlineNoYield;

        fn run(self) -> Self::Output {
            17
        }
    }

    struct SupportedGeneratedContractTask;

    impl GeneratedExplicitFiberTask for SupportedGeneratedContractTask {
        type Output = ();

        fn run(self) -> Self::Output {}

        fn task_attributes() -> Result<FiberTaskAttributes, FiberError>
        where
            Self: Sized,
        {
            Ok(generated_explicit_task_contract_attributes::<Self>())
        }
    }

    declare_generated_fiber_task_contract!(
        SupportedGeneratedContractTask,
        NonZeroUsize::new(8 * 1024).expect("non-zero supported generated stack"),
    );

    const COMPILE_TIME_EXPLICIT_CLASSES: [FiberStackClassConfig; 1] = [
        match FiberStackClassConfig::new(
            match FiberStackClass::new(NonZeroUsize::new(8 * 1024).expect("non-zero class")) {
                Ok(class) => class,
                Err(_) => panic!("valid class"),
            },
            2,
        ) {
            Ok(class) => class,
            Err(_) => panic!("valid compile-time class config"),
        },
    ];

    const COMPILE_TIME_EXPLICIT_CONFIG: FiberPoolConfig<'static> =
        match FiberPoolConfig::classed(&COMPILE_TIME_EXPLICIT_CLASSES) {
            Ok(config) => config,
            Err(_) => panic!("compile-time explicit config should be valid"),
        };
    const COMPILE_TIME_EXPLICIT_ATTRIBUTES: FiberTaskAttributes =
        match FiberTaskAttributes::from_stack_bytes(
            NonZeroUsize::new(8 * 1024).expect("non-zero supported stack"),
            FiberTaskPriority::DEFAULT,
        ) {
            Ok(attributes) => attributes,
            Err(_) => panic!("compile-time explicit attributes should be valid"),
        };

    const _: () =
        COMPILE_TIME_EXPLICIT_CONFIG.assert_explicit_task_supported::<SupportedExplicitTask>();
    const _: () = COMPILE_TIME_EXPLICIT_CONFIG
        .assert_task_attributes_supported(COMPILE_TIME_EXPLICIT_ATTRIBUTES);
    const _: () = COMPILE_TIME_EXPLICIT_CONFIG
        .assert_generated_task_supported::<SupportedGeneratedContractTask>();

    static CAPACITY_EVENT_CALLS: AtomicU32 = AtomicU32::new(0);
    static LAST_CAPACITY_FIBER_ID: AtomicUsize = AtomicUsize::new(0);
    static LAST_CAPACITY_CARRIER_ID: AtomicUsize = AtomicUsize::new(usize::MAX);
    static LAST_CAPACITY_COMMITTED: AtomicU32 = AtomicU32::new(0);
    static LAST_CAPACITY_RESERVATION: AtomicU32 = AtomicU32::new(0);
    static YIELD_BUDGET_EVENT_CALLS: AtomicU32 = AtomicU32::new(0);
    static LAST_YIELD_BUDGET_FIBER_ID: AtomicUsize = AtomicUsize::new(0);
    static LAST_YIELD_BUDGET_CARRIER_ID: AtomicUsize = AtomicUsize::new(usize::MAX);
    static LAST_YIELD_BUDGET_NANOS: AtomicU64 = AtomicU64::new(0);
    static LAST_YIELD_OBSERVED_NANOS: AtomicU64 = AtomicU64::new(0);
    static ELASTIC_TEST_LOCK: StdOnceLock<StdMutex<()>> = StdOnceLock::new();

    fn lock_elastic_tests() -> std::sync::MutexGuard<'static, ()> {
        ELASTIC_TEST_LOCK
            .get_or_init(|| StdMutex::new(()))
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn record_capacity_event(event: FiberCapacityEvent) {
        CAPACITY_EVENT_CALLS.fetch_add(1, Ordering::AcqRel);
        LAST_CAPACITY_FIBER_ID.store(
            usize::try_from(event.fiber_id).unwrap_or(usize::MAX),
            Ordering::Release,
        );
        LAST_CAPACITY_CARRIER_ID.store(event.carrier_id, Ordering::Release);
        LAST_CAPACITY_COMMITTED.store(event.committed_pages, Ordering::Release);
        LAST_CAPACITY_RESERVATION.store(event.reservation_pages, Ordering::Release);
    }

    fn record_yield_budget_event(event: FiberYieldBudgetEvent) {
        YIELD_BUDGET_EVENT_CALLS.fetch_add(1, Ordering::AcqRel);
        LAST_YIELD_BUDGET_FIBER_ID.store(
            usize::try_from(event.fiber_id).unwrap_or(usize::MAX),
            Ordering::Release,
        );
        LAST_YIELD_BUDGET_CARRIER_ID.store(event.carrier_id, Ordering::Release);
        LAST_YIELD_BUDGET_NANOS.store(
            saturating_duration_to_nanos_u64(event.budget),
            Ordering::Release,
        );
        LAST_YIELD_OBSERVED_NANOS.store(
            saturating_duration_to_nanos_u64(event.observed),
            Ordering::Release,
        );
    }

    #[test]
    fn stack_class_rounding_matches_current_slab_envelope() {
        let support = GreenPool::support();
        let config = FiberPoolConfig::fixed(
            NonZeroUsize::new(12 * 1024).expect("non-zero fixed stack"),
            2,
        );
        let slab = FiberStackSlab::new(
            &config,
            support.context.min_stack_alignment.max(16),
            support.context.stack_direction,
        )
        .expect("fixed stack slab should build");

        let default = slab
            .default_task_class()
            .expect("slab should map its envelope to a task class");
        assert_eq!(default.size_bytes().get(), 8 * 1024);
        assert!(slab.supports_task_class(default));
        assert!(slab.supports_task_class(FiberStackClass::MIN));
        assert!(
            !slab.supports_task_class(
                FiberStackClass::new(NonZeroUsize::new(32 * 1024).expect("non-zero larger class"))
                    .expect("larger class should be valid"),
            )
        );
    }

    #[test]
    fn class_store_selects_smallest_matching_pool() {
        let support = GreenPool::support();
        let classes = [
            FiberStackClassConfig::new(
                FiberStackClass::new(NonZeroUsize::new(4 * 1024).expect("non-zero class"))
                    .expect("valid class"),
                1,
            )
            .expect("valid class config"),
            FiberStackClassConfig::new(
                FiberStackClass::new(NonZeroUsize::new(8 * 1024).expect("non-zero class"))
                    .expect("valid class"),
                1,
            )
            .expect("valid class config"),
            FiberStackClassConfig::new(
                FiberStackClass::new(NonZeroUsize::new(16 * 1024).expect("non-zero class"))
                    .expect("valid class"),
                1,
            )
            .expect("valid class config"),
        ];
        let config = FiberPoolConfig::classed(&classes).expect("classed config should build");
        let store = FiberStackStore::new(
            &config,
            support.context.min_stack_alignment.max(16),
            support.context.stack_direction,
        )
        .expect("class store should build");

        assert_eq!(store.total_capacity(), 3);
        assert_eq!(
            store
                .default_task_class()
                .expect("largest configured class should be discoverable")
                .size_bytes()
                .get(),
            16 * 1024
        );

        let task = FiberTaskAttributes::new(
            FiberStackClass::from_stack_bytes(
                NonZeroUsize::new(6 * 1024).expect("non-zero requested size"),
            )
            .expect("task class should round"),
        );
        let lease = store.acquire(task).expect("matching class should allocate");
        assert_eq!(lease.class.size_bytes().get(), 8 * 1024);
        store
            .release(lease.pool_index, lease.slot_index)
            .expect("allocated class slot should release");

        let oversize = FiberStackClass::new(NonZeroUsize::new(32 * 1024).expect("non-zero class"))
            .expect("valid class");
        assert!(!store.supports_task_class(oversize));
    }

    #[test]
    fn classed_config_derives_capacity_and_largest_backing() {
        let classes = [
            FiberStackClassConfig::new(FiberStackClass::MIN, 3).expect("valid class config"),
            FiberStackClassConfig::new(
                FiberStackClass::new(NonZeroUsize::new(8 * 1024).expect("non-zero class"))
                    .expect("valid class"),
                5,
            )
            .expect("valid class config"),
        ];
        let config = FiberPoolConfig::classed(&classes).expect("classed config should build");

        assert_eq!(config.task_capacity_per_carrier().expect("capacity"), 8);
        assert_eq!(config.growth_chunk, 8);
        assert_eq!(config.max_fibers_per_carrier, 8);
        assert_eq!(config.growth, GreenGrowth::Fixed);
        assert_eq!(config.scheduling, GreenScheduling::Fifo);
        assert_eq!(
            config.stack_backing,
            FiberStackBacking::Fixed {
                stack_size: classes[1].class.size_bytes(),
            }
        );
    }

    #[test]
    fn class_store_uses_per_class_growth_chunks() {
        let support = GreenPool::support();
        let classes = [
            FiberStackClassConfig::new(FiberStackClass::MIN, 4)
                .expect("valid class config")
                .with_growth_chunk(2)
                .expect("valid class growth chunk"),
            FiberStackClassConfig::new(
                FiberStackClass::new(NonZeroUsize::new(8 * 1024).expect("non-zero class"))
                    .expect("valid class"),
                6,
            )
            .expect("valid class config")
            .with_growth_chunk(3)
            .expect("valid class growth chunk"),
        ];
        let config = FiberPoolConfig::classed(&classes)
            .expect("classed config should build")
            .with_growth(GreenGrowth::OnDemand);
        let store = FiberStackStore::new(
            &config,
            support.context.min_stack_alignment.max(16),
            support.context.stack_direction,
        )
        .expect("class store should build");

        let FiberStackStore::Classes(pools) = store else {
            panic!("expected class-backed store");
        };
        assert_eq!(pools.entry(0).expect("first pool").slab.chunk_size, 2);
        assert_eq!(pools.entry(0).expect("first pool").slab.initial_slots, 2);
        assert_eq!(pools.entry(1).expect("second pool").slab.chunk_size, 3);
        assert_eq!(pools.entry(1).expect("second pool").slab.initial_slots, 3);
    }

    #[test]
    fn classed_config_validates_explicit_task_contracts() {
        let classes = [FiberStackClassConfig::new(
            FiberStackClass::new(NonZeroUsize::new(8 * 1024).expect("non-zero class"))
                .expect("valid class"),
            2,
        )
        .expect("valid class config")];
        let config = FiberPoolConfig::classed(&classes).expect("classed config should build");

        assert!(
            config
                .validate_generated_task::<SupportedGeneratedContractTask>()
                .is_ok()
        );

        let error = config
            .validate_explicit_task::<OversizedExplicitTask>()
            .expect_err("oversized explicit task should be rejected");
        assert_eq!(error.kind(), FiberError::unsupported().kind());
    }

    #[test]
    fn explicit_task_contracts_work_in_const_context() {
        const VALIDATION: Result<(), FiberError> =
            COMPILE_TIME_EXPLICIT_CONFIG.validate_explicit_task::<SupportedExplicitTask>();

        assert_eq!(
            SupportedExplicitTask::ATTRIBUTES
                .stack_class
                .size_bytes()
                .get(),
            8 * 1024
        );
        VALIDATION.expect("supported explicit task should validate in const context");
    }

    #[test]
    fn raw_task_attributes_work_in_const_context() {
        const VALIDATION: Result<(), FiberError> =
            COMPILE_TIME_EXPLICIT_CONFIG.validate_task_attributes(COMPILE_TIME_EXPLICIT_ATTRIBUTES);

        assert_eq!(
            COMPILE_TIME_EXPLICIT_ATTRIBUTES
                .stack_class
                .size_bytes()
                .get(),
            8 * 1024
        );
        VALIDATION.expect("raw task attributes should validate in const context");
    }

    #[test]
    fn generated_task_contracts_work_in_const_context() {
        const VALIDATION: Result<(), FiberError> = COMPILE_TIME_EXPLICIT_CONFIG
            .validate_generated_task_contract::<SupportedGeneratedContractTask>(
        );

        assert_eq!(
            <SupportedGeneratedContractTask as GeneratedExplicitFiberTaskContract>::ATTRIBUTES
                .stack_class
                .size_bytes()
                .get(),
            8 * 1024
        );
        VALIDATION.expect("generated task should validate in const context");
    }

    #[test]
    fn live_pool_validates_generated_task_contracts_before_spawn() {
        let carrier = ThreadPool::new(&ThreadPoolConfig::new()).expect("carrier pool should build");
        let classes =
            [FiberStackClassConfig::new(FiberStackClass::MIN, 2).expect("valid class config")];
        let green = GreenPool::new(
            &FiberPoolConfig::classed(&classes).expect("classed config should build"),
            &carrier,
        )
        .expect("green pool should build");

        let error = green
            .validate_generated_task::<SupportedGeneratedContractTask>()
            .expect_err("generated task should be rejected when class is missing");
        assert_eq!(error.kind(), FiberError::unsupported().kind());

        let error = green
            .spawn_generated(SupportedGeneratedContractTask)
            .expect_err("spawn should reject unsupported generated class");
        assert_eq!(error.kind(), FiberError::unsupported().kind());

        green
            .shutdown()
            .expect("green pool should shut down cleanly");
        carrier
            .shutdown()
            .expect("carrier pool should shut down cleanly");
    }

    #[test]
    fn elastic_stack_slab_grows_and_shrinks_by_chunk() {
        let _guard = lock_elastic_tests();
        let support = GreenPool::support();
        let config = FiberPoolConfig {
            classes: &[],
            stack_backing: FiberStackBacking::Elastic {
                initial_size: NonZeroUsize::new(4 * 1024).expect("non-zero initial stack"),
                max_size: NonZeroUsize::new(16 * 1024).expect("non-zero max stack"),
            },
            sizing: default_runtime_sizing_strategy(),
            guard_pages: 1,
            growth_chunk: 2,
            max_fibers_per_carrier: 5,
            scheduling: GreenScheduling::Fifo,
            priority_age_cap: None,
            growth: GreenGrowth::OnDemand,
            telemetry: FiberTelemetry::Full,
            capacity_policy: CapacityPolicy::Abort,
            yield_budget_policy: FiberYieldBudgetPolicy::Abort,
            reactor_policy: GreenReactorPolicy::Automatic,
            huge_pages: HugePagePolicy::Disabled,
        };
        let slab = FiberStackSlab::new(
            &config,
            support.context.min_stack_alignment.max(16),
            support.context.stack_direction,
        )
        .expect("elastic stack slab should build");

        {
            let state = slab
                .state
                .lock()
                .map_err(fiber_error_from_sync)
                .expect("slab state should be observable");
            assert_eq!(state.committed_slots, 2);
        }

        let mut leases = Vec::new();
        for _ in 0..5 {
            leases.push(slab.acquire().expect("chunked slab should grow on demand"));
        }
        {
            let state = slab
                .state
                .lock()
                .map_err(fiber_error_from_sync)
                .expect("slab state should be observable");
            assert_eq!(state.committed_slots, 5);
        }

        for lease in &leases {
            if lease.slot_index >= 2 {
                slab.release(lease.slot_index)
                    .expect("tail slots should release cleanly");
            }
        }
        {
            let state = slab
                .state
                .lock()
                .map_err(fiber_error_from_sync)
                .expect("slab state should be observable");
            assert_eq!(state.committed_slots, 4);
        }

        for lease in &leases {
            if lease.slot_index < 2 {
                slab.release(lease.slot_index)
                    .expect("initial slots should release cleanly");
            }
        }
        {
            let state = slab
                .state
                .lock()
                .map_err(fiber_error_from_sync)
                .expect("slab state should be observable");
            assert_eq!(state.committed_slots, 2);
        }
    }

    #[test]
    fn elastic_stack_fault_promotion_makes_detector_page_writable() {
        let _guard = lock_elastic_tests();
        let support = GreenPool::support();
        let config = FiberPoolConfig {
            classes: &[],
            stack_backing: FiberStackBacking::Elastic {
                initial_size: NonZeroUsize::new(4 * 1024).expect("non-zero initial stack"),
                max_size: NonZeroUsize::new(16 * 1024).expect("non-zero max stack"),
            },
            sizing: default_runtime_sizing_strategy(),
            guard_pages: 1,
            growth_chunk: 1,
            max_fibers_per_carrier: 1,
            scheduling: GreenScheduling::Fifo,
            priority_age_cap: None,
            growth: GreenGrowth::OnDemand,
            telemetry: FiberTelemetry::Full,
            capacity_policy: CapacityPolicy::Abort,
            yield_budget_policy: FiberYieldBudgetPolicy::Abort,
            reactor_policy: GreenReactorPolicy::Automatic,
            huge_pages: HugePagePolicy::Disabled,
        };
        let slab = FiberStackSlab::new(
            &config,
            support.context.min_stack_alignment.max(16),
            support.context.stack_direction,
        )
        .expect("elastic stack slab should build");

        let metadata = match &slab.backing {
            FiberStackBackingState::Elastic { metadata, .. } => metadata,
            FiberStackBackingState::Fixed(_) => panic!("expected elastic backing"),
        };
        let meta = &metadata[0];
        let detector = meta.detector_page.load(Ordering::Acquire);
        let old_guard = meta.guard_page.load(Ordering::Acquire);

        assert!(try_promote_elastic_stack_fault(detector));
        assert_eq!(meta.detector_page.load(Ordering::Acquire), old_guard);
        assert_ne!(meta.guard_page.load(Ordering::Acquire), old_guard);

        unsafe {
            (detector as *mut u8).write_volatile(0x5A);
            assert_eq!((detector as *const u8).read_volatile(), 0x5A);
        }
    }

    #[test]
    fn elastic_stack_stats_track_growth_and_capacity() {
        let _guard = lock_elastic_tests();
        let support = GreenPool::support();
        let config = FiberPoolConfig {
            classes: &[],
            stack_backing: FiberStackBacking::Elastic {
                initial_size: NonZeroUsize::new(4 * 1024).expect("non-zero initial stack"),
                max_size: NonZeroUsize::new(8 * 1024).expect("non-zero max stack"),
            },
            sizing: default_runtime_sizing_strategy(),
            guard_pages: 1,
            growth_chunk: 1,
            max_fibers_per_carrier: 1,
            scheduling: GreenScheduling::Fifo,
            priority_age_cap: None,
            growth: GreenGrowth::OnDemand,
            telemetry: FiberTelemetry::Full,
            capacity_policy: CapacityPolicy::Abort,
            yield_budget_policy: FiberYieldBudgetPolicy::Abort,
            reactor_policy: GreenReactorPolicy::Automatic,
            huge_pages: HugePagePolicy::Disabled,
        };
        let slab = FiberStackSlab::new(
            &config,
            support.context.min_stack_alignment.max(16),
            support.context.stack_direction,
        )
        .expect("elastic stack slab should build");

        let lease = slab
            .acquire()
            .expect("elastic slab should allocate one slot");
        let metadata = match &slab.backing {
            FiberStackBackingState::Elastic { metadata, .. } => metadata,
            FiberStackBackingState::Fixed(_) => panic!("expected elastic backing"),
        };
        let meta = &metadata[lease.slot_index];
        let detector = meta.detector_page.load(Ordering::Acquire);

        assert!(try_promote_elastic_stack_fault(detector));

        let stats = slab.stack_stats().expect("telemetry should be enabled");
        assert_eq!(stats.total_growth_events, 1);
        assert_eq!(stats.peak_committed_pages, 2);
        assert_eq!(stats.committed_distribution.as_slice(), &[(2, 1)]);
        assert_eq!(stats.at_capacity_count, 1);

        slab.release(lease.slot_index)
            .expect("elastic slab should release the slot cleanly");
        let stats = slab.stack_stats().expect("telemetry should remain enabled");
        assert_eq!(stats.total_growth_events, 0);
        assert_eq!(stats.peak_committed_pages, 0);
        assert!(stats.committed_distribution.is_empty());
        assert_eq!(stats.at_capacity_count, 0);
    }

    #[test]
    fn elastic_capacity_events_dispatch_with_fiber_identity() {
        let _guard = lock_elastic_tests();
        CAPACITY_EVENT_CALLS.store(0, Ordering::Release);
        LAST_CAPACITY_FIBER_ID.store(0, Ordering::Release);
        LAST_CAPACITY_CARRIER_ID.store(usize::MAX, Ordering::Release);
        LAST_CAPACITY_COMMITTED.store(0, Ordering::Release);
        LAST_CAPACITY_RESERVATION.store(0, Ordering::Release);

        let support = GreenPool::support();
        let config = FiberPoolConfig {
            classes: &[],
            stack_backing: FiberStackBacking::Elastic {
                initial_size: NonZeroUsize::new(4 * 1024).expect("non-zero initial stack"),
                max_size: NonZeroUsize::new(8 * 1024).expect("non-zero max stack"),
            },
            sizing: default_runtime_sizing_strategy(),
            guard_pages: 1,
            growth_chunk: 1,
            max_fibers_per_carrier: 1,
            scheduling: GreenScheduling::Fifo,
            priority_age_cap: None,
            growth: GreenGrowth::OnDemand,
            telemetry: FiberTelemetry::Full,
            capacity_policy: CapacityPolicy::Notify(record_capacity_event),
            yield_budget_policy: FiberYieldBudgetPolicy::Abort,
            reactor_policy: GreenReactorPolicy::Automatic,
            huge_pages: HugePagePolicy::Disabled,
        };
        let slab = FiberStackSlab::new(
            &config,
            support.context.min_stack_alignment.max(16),
            support.context.stack_direction,
        )
        .expect("elastic stack slab should build");

        let lease = slab
            .acquire()
            .expect("elastic slab should allocate one slot");
        slab.attach_slot_identity(lease.slot_index, 41, 3, PlatformWakeToken::invalid())
            .expect("slot identity should attach");

        let metadata = match &slab.backing {
            FiberStackBackingState::Elastic { metadata, .. } => metadata,
            FiberStackBackingState::Fixed(_) => panic!("expected elastic backing"),
        };
        let meta = &metadata[lease.slot_index];
        let detector = meta.detector_page.load(Ordering::Acquire);
        assert!(try_promote_elastic_stack_fault(detector));

        slab.dispatch_capacity_event(lease.slot_index, config.capacity_policy)
            .expect("capacity event should dispatch");
        assert_eq!(CAPACITY_EVENT_CALLS.load(Ordering::Acquire), 1);
        assert_eq!(LAST_CAPACITY_FIBER_ID.load(Ordering::Acquire), 41);
        assert_eq!(LAST_CAPACITY_CARRIER_ID.load(Ordering::Acquire), 3);
        assert_eq!(LAST_CAPACITY_COMMITTED.load(Ordering::Acquire), 2);
        assert_eq!(LAST_CAPACITY_RESERVATION.load(Ordering::Acquire), 2);

        slab.dispatch_capacity_event(lease.slot_index, config.capacity_policy)
            .expect("capacity event should not redispatch");
        assert_eq!(CAPACITY_EVENT_CALLS.load(Ordering::Acquire), 1);
    }

    #[test]
    fn elastic_stack_registry_tracks_live_slots_and_clears_on_drop() {
        let _guard = lock_elastic_tests();
        let support = GreenPool::support();
        let config = FiberPoolConfig {
            classes: &[],
            stack_backing: FiberStackBacking::Elastic {
                initial_size: NonZeroUsize::new(4 * 1024).expect("non-zero initial stack"),
                max_size: NonZeroUsize::new(16 * 1024).expect("non-zero max stack"),
            },
            sizing: default_runtime_sizing_strategy(),
            guard_pages: 1,
            growth_chunk: 1,
            max_fibers_per_carrier: 1,
            scheduling: GreenScheduling::Fifo,
            priority_age_cap: None,
            growth: GreenGrowth::OnDemand,
            telemetry: FiberTelemetry::Disabled,
            capacity_policy: CapacityPolicy::Abort,
            yield_budget_policy: FiberYieldBudgetPolicy::Abort,
            reactor_policy: GreenReactorPolicy::Automatic,
            huge_pages: HugePagePolicy::Disabled,
        };
        let slab = FiberStackSlab::new(
            &config,
            support.context.min_stack_alignment.max(16),
            support.context.stack_direction,
        )
        .expect("elastic stack slab should build");

        let lease = slab
            .acquire()
            .expect("elastic slab should allocate one slot");
        let metadata = match &slab.backing {
            FiberStackBackingState::Elastic { metadata, .. } => metadata,
            FiberStackBackingState::Fixed(_) => panic!("expected elastic backing"),
        };
        let detector = metadata[lease.slot_index]
            .detector_page
            .load(Ordering::Acquire);

        let snapshot_ptr =
            ELASTIC_STACK_SNAPSHOT.load(Ordering::Acquire) as *const ElasticRegistrySnapshotHeader;
        assert!(!snapshot_ptr.is_null());
        let snapshot = unsafe { &*snapshot_ptr };
        assert!(find_snapshot_elastic_entry(snapshot, detector).is_some());

        drop(slab);
        let snapshot_ptr =
            ELASTIC_STACK_SNAPSHOT.load(Ordering::Acquire) as *const ElasticRegistrySnapshotHeader;
        assert!(snapshot_ptr.is_null());
    }

    #[test]
    fn elastic_huge_page_policy_leaves_a_small_page_growth_window() {
        let _guard = lock_elastic_tests();
        if !system_mem()
            .support()
            .advice
            .contains(MemAdviceCaps::HUGE_PAGE)
        {
            return;
        }
        let support = GreenPool::support();
        let page = system_mem().page_info().alloc_granule.get();
        let config = FiberPoolConfig {
            classes: &[],
            stack_backing: FiberStackBacking::Elastic {
                initial_size: NonZeroUsize::new(4 * 1024).expect("non-zero initial stack"),
                max_size: NonZeroUsize::new(4 * 1024 * 1024).expect("non-zero max stack"),
            },
            sizing: default_runtime_sizing_strategy(),
            guard_pages: 1,
            growth_chunk: 1,
            max_fibers_per_carrier: 1,
            scheduling: GreenScheduling::Fifo,
            priority_age_cap: None,
            growth: GreenGrowth::OnDemand,
            telemetry: FiberTelemetry::Disabled,
            capacity_policy: CapacityPolicy::Abort,
            yield_budget_policy: FiberYieldBudgetPolicy::Abort,
            reactor_policy: GreenReactorPolicy::Automatic,
            huge_pages: HugePagePolicy::Enabled {
                size: HugePageSize::TwoMiB,
            },
        };
        let slab = FiberStackSlab::new(
            &config,
            support.context.min_stack_alignment.max(16),
            support.context.stack_direction,
        )
        .expect("elastic stack slab should build with huge-page advice");

        let (huge_region, no_huge_region) = slab
            .huge_page_regions(0, HugePageSize::TwoMiB)
            .expect("huge-page planning should succeed");
        let huge_region =
            huge_region.expect("large elastic slots should expose an upper huge region");
        let no_huge_region = no_huge_region
            .expect("elastic huge-page planning should keep a lower small-page window");
        assert!(huge_region.len >= HugePageSize::TwoMiB.bytes());
        assert!(no_huge_region.len >= 3 * page);
        assert!(huge_region.base.addr().get() > no_huge_region.base.addr().get());
    }

    #[test]
    fn priority_queue_dequeues_higher_priorities_first() {
        let mut buckets = [PriorityBucket::empty(); FIBER_PRIORITY_LEVELS];
        let mut next = [EMPTY_QUEUE_SLOT; 8];
        let mut priorities = [FiberTaskPriority::DEFAULT.get(); 8];
        let mut enqueue_epochs = [0u64; 8];
        let bucket_slice = MetadataSlice {
            ptr: core::ptr::NonNull::new(buckets.as_mut_ptr()).expect("bucket slice pointer"),
            len: buckets.len(),
        };
        let next_slice = MetadataSlice {
            ptr: core::ptr::NonNull::new(next.as_mut_ptr()).expect("next slice pointer"),
            len: next.len(),
        };
        let priority_slice = MetadataSlice {
            ptr: core::ptr::NonNull::new(priorities.as_mut_ptr()).expect("priority slice pointer"),
            len: priorities.len(),
        };
        let enqueue_epoch_slice = MetadataSlice {
            ptr: core::ptr::NonNull::new(enqueue_epochs.as_mut_ptr())
                .expect("enqueue epoch slice pointer"),
            len: enqueue_epochs.len(),
        };
        let mut queue = MetadataPriorityQueue::new(
            bucket_slice,
            next_slice,
            priority_slice,
            enqueue_epoch_slice,
            None,
        )
        .expect("priority queue builds");

        queue
            .enqueue(1, FiberTaskPriority::new(-5))
            .expect("low-priority item should enqueue");
        queue
            .enqueue(2, FiberTaskPriority::DEFAULT)
            .expect("default-priority item should enqueue");
        queue
            .enqueue(3, FiberTaskPriority::new(10))
            .expect("high-priority item should enqueue");

        assert_eq!(queue.dequeue(), Some(3));
        assert_eq!(queue.dequeue(), Some(2));
        assert_eq!(queue.dequeue(), Some(1));
        assert_eq!(queue.dequeue(), None);
    }

    #[test]
    fn priority_queue_aging_eventually_promotes_waiting_work() {
        let mut buckets = [PriorityBucket::empty(); FIBER_PRIORITY_LEVELS];
        let mut next = [EMPTY_QUEUE_SLOT; 8];
        let mut priorities = [FiberTaskPriority::DEFAULT.get(); 8];
        let mut enqueue_epochs = [0u64; 8];
        let bucket_slice = MetadataSlice {
            ptr: core::ptr::NonNull::new(buckets.as_mut_ptr()).expect("bucket slice pointer"),
            len: buckets.len(),
        };
        let next_slice = MetadataSlice {
            ptr: core::ptr::NonNull::new(next.as_mut_ptr()).expect("next slice pointer"),
            len: next.len(),
        };
        let priority_slice = MetadataSlice {
            ptr: core::ptr::NonNull::new(priorities.as_mut_ptr()).expect("priority slice pointer"),
            len: priorities.len(),
        };
        let enqueue_epoch_slice = MetadataSlice {
            ptr: core::ptr::NonNull::new(enqueue_epochs.as_mut_ptr())
                .expect("enqueue epoch slice pointer"),
            len: enqueue_epochs.len(),
        };
        let mut queue = MetadataPriorityQueue::new(
            bucket_slice,
            next_slice,
            priority_slice,
            enqueue_epoch_slice,
            None,
        )
        .expect("priority queue builds");

        queue
            .enqueue(1, FiberTaskPriority::DEFAULT)
            .expect("default-priority task should enqueue");
        queue
            .enqueue(2, FiberTaskPriority::new(1))
            .expect("slightly higher-priority task should enqueue");

        assert_eq!(queue.dequeue(), Some(2));
        assert_eq!(queue.waiting_age(1), FiberTaskAge(1));

        queue
            .enqueue(3, FiberTaskPriority::new(1))
            .expect("another slightly higher-priority task should enqueue");
        assert_eq!(queue.dequeue(), Some(3));
        assert_eq!(queue.waiting_age(1), FiberTaskAge(2));

        queue
            .enqueue(4, FiberTaskPriority::new(1))
            .expect("third slightly higher-priority task should enqueue");
        assert_eq!(queue.dequeue(), Some(1));
        assert_eq!(queue.waiting_age(1), FiberTaskAge::ZERO);
    }

    #[test]
    fn priority_queue_prefers_higher_base_priority_when_effective_priorities_tie() {
        let mut buckets = [PriorityBucket::empty(); FIBER_PRIORITY_LEVELS];
        let mut next = [EMPTY_QUEUE_SLOT; 8];
        let mut priorities = [FiberTaskPriority::DEFAULT.get(); 8];
        let mut enqueue_epochs = [0u64; 8];
        let bucket_slice = MetadataSlice {
            ptr: core::ptr::NonNull::new(buckets.as_mut_ptr()).expect("bucket slice pointer"),
            len: buckets.len(),
        };
        let next_slice = MetadataSlice {
            ptr: core::ptr::NonNull::new(next.as_mut_ptr()).expect("next slice pointer"),
            len: next.len(),
        };
        let priority_slice = MetadataSlice {
            ptr: core::ptr::NonNull::new(priorities.as_mut_ptr()).expect("priority slice pointer"),
            len: priorities.len(),
        };
        let enqueue_epoch_slice = MetadataSlice {
            ptr: core::ptr::NonNull::new(enqueue_epochs.as_mut_ptr())
                .expect("enqueue epoch slice pointer"),
            len: enqueue_epochs.len(),
        };
        let mut queue = MetadataPriorityQueue::new(
            bucket_slice,
            next_slice,
            priority_slice,
            enqueue_epoch_slice,
            None,
        )
        .expect("priority queue builds");

        queue
            .enqueue(1, FiberTaskPriority::DEFAULT)
            .expect("default-priority task should enqueue");
        queue
            .enqueue(7, FiberTaskPriority::new(10))
            .expect("high-priority task should enqueue");
        assert_eq!(queue.dequeue(), Some(7));

        queue
            .enqueue(2, FiberTaskPriority::new(1))
            .expect("slightly higher-priority task should enqueue");

        assert_eq!(queue.waiting_age(1), FiberTaskAge(1));
        assert_eq!(queue.waiting_age(2), FiberTaskAge::ZERO);
        assert_eq!(queue.effective_priority(1), FiberTaskPriority::new(1));
        assert_eq!(queue.effective_priority(2), FiberTaskPriority::new(1));
        assert_eq!(queue.dequeue(), Some(2));
        assert_eq!(queue.dequeue(), Some(1));
    }

    #[test]
    fn priority_queue_preserves_fifo_order_within_one_priority_bucket() {
        let mut buckets = [PriorityBucket::empty(); FIBER_PRIORITY_LEVELS];
        let mut next = [EMPTY_QUEUE_SLOT; 8];
        let mut priorities = [FiberTaskPriority::DEFAULT.get(); 8];
        let mut enqueue_epochs = [0u64; 8];
        let bucket_slice = MetadataSlice {
            ptr: core::ptr::NonNull::new(buckets.as_mut_ptr()).expect("bucket slice pointer"),
            len: buckets.len(),
        };
        let next_slice = MetadataSlice {
            ptr: core::ptr::NonNull::new(next.as_mut_ptr()).expect("next slice pointer"),
            len: next.len(),
        };
        let priority_slice = MetadataSlice {
            ptr: core::ptr::NonNull::new(priorities.as_mut_ptr()).expect("priority slice pointer"),
            len: priorities.len(),
        };
        let enqueue_epoch_slice = MetadataSlice {
            ptr: core::ptr::NonNull::new(enqueue_epochs.as_mut_ptr())
                .expect("enqueue epoch slice pointer"),
            len: enqueue_epochs.len(),
        };
        let mut queue = MetadataPriorityQueue::new(
            bucket_slice,
            next_slice,
            priority_slice,
            enqueue_epoch_slice,
            None,
        )
        .expect("priority queue builds");

        queue
            .enqueue(1, FiberTaskPriority::new(3))
            .expect("first task should enqueue");
        queue
            .enqueue(2, FiberTaskPriority::new(3))
            .expect("second task should enqueue");
        queue
            .enqueue(3, FiberTaskPriority::new(3))
            .expect("third task should enqueue");

        assert_eq!(queue.dequeue(), Some(1));
        assert_eq!(queue.dequeue(), Some(2));
        assert_eq!(queue.dequeue(), Some(3));
    }

    #[test]
    fn priority_queue_age_cap_limits_virtual_promotion() {
        let mut buckets = [PriorityBucket::empty(); FIBER_PRIORITY_LEVELS];
        let mut next = [EMPTY_QUEUE_SLOT; 8];
        let mut priorities = [FiberTaskPriority::DEFAULT.get(); 8];
        let mut enqueue_epochs = [0u64; 8];
        let bucket_slice = MetadataSlice {
            ptr: core::ptr::NonNull::new(buckets.as_mut_ptr()).expect("bucket slice pointer"),
            len: buckets.len(),
        };
        let next_slice = MetadataSlice {
            ptr: core::ptr::NonNull::new(next.as_mut_ptr()).expect("next slice pointer"),
            len: next.len(),
        };
        let priority_slice = MetadataSlice {
            ptr: core::ptr::NonNull::new(priorities.as_mut_ptr()).expect("priority slice pointer"),
            len: priorities.len(),
        };
        let enqueue_epoch_slice = MetadataSlice {
            ptr: core::ptr::NonNull::new(enqueue_epochs.as_mut_ptr())
                .expect("enqueue epoch slice pointer"),
            len: enqueue_epochs.len(),
        };
        let mut queue = MetadataPriorityQueue::new(
            bucket_slice,
            next_slice,
            priority_slice,
            enqueue_epoch_slice,
            Some(FiberTaskAgeCap::new(1)),
        )
        .expect("priority queue builds");

        queue
            .enqueue(1, FiberTaskPriority::new(-4))
            .expect("low-priority task should enqueue");
        queue
            .enqueue(2, FiberTaskPriority::new(1))
            .expect("higher-priority task should enqueue");

        assert_eq!(queue.dequeue(), Some(2));
        assert_eq!(queue.waiting_age(1), FiberTaskAge(1));
        assert_eq!(queue.effective_priority(1), FiberTaskPriority::new(-3));

        queue
            .enqueue(3, FiberTaskPriority::new(1))
            .expect("second higher-priority task should enqueue");
        assert_eq!(queue.dequeue(), Some(3));
        assert_eq!(queue.waiting_age(1), FiberTaskAge(1));
        assert_eq!(queue.dequeue(), Some(1));
    }

    #[test]
    fn green_exclusion_span_guard_tracks_active_spans_and_blocks_yield() {
        const REQUIRED_LEAF: [u32; 2] = [1_u32 << 6, 0];
        const REQUIRED_ROOT: [u32; 1] = [1_u32 << 0];
        const REQUIRED_LEVELS: [&[u32]; 1] = [&REQUIRED_ROOT];
        const REQUIRED_TREE: CooperativeExclusionSummaryTree =
            CooperativeExclusionSummaryTree::new(&REQUIRED_LEAF, &REQUIRED_LEVELS);

        let carrier = ThreadPool::new(&ThreadPoolConfig {
            min_threads: 1,
            max_threads: 1,
            placement: PoolPlacement::Inherit,
            ..ThreadPoolConfig::new()
        })
        .expect("single-carrier pool should build");
        let fibers =
            GreenPool::new(&FiberPoolConfig::new(), &carrier).expect("green pool should build");

        let task = fibers
            .spawn(move || -> Result<(), FiberError> {
                let span = CooperativeExclusionSpan::new(7).map_err(fiber_error_from_sync)?;
                let _guard =
                    enter_current_green_exclusion_span(span).map_err(fiber_error_from_sync)?;
                let mut active = [span; 4];
                let copied = current_green_exclusion_spans(&mut active);
                assert_eq!(copied, 1);
                assert_eq!(active[0], span);
                assert!(current_green_exclusion_allows(&[]));
                assert!(current_green_exclusion_allows(&[
                    CooperativeExclusionSpan::new(9).map_err(fiber_error_from_sync)?
                ]));
                assert!(!current_green_exclusion_allows(&[span]));
                assert!(!current_green_exclusion_allows_tree(&REQUIRED_TREE));
                let error = yield_now().expect_err("yield should reject while exclusion span held");
                assert_eq!(error.kind(), FiberError::state_conflict().kind());
                Ok(())
            })
            .expect("task should spawn");

        task.join()
            .expect("task should complete without runtime failure")
            .expect("task should observe the expected span behavior");
        fibers
            .shutdown()
            .expect("green pool should shut down cleanly");
        carrier
            .shutdown()
            .expect("carrier pool should shut down cleanly");
    }

    #[test]
    fn cooperative_exclusion_summary_tree_matches_named_spans() {
        const LEAF: [u32; 33] = {
            let mut words = [0_u32; 33];
            words[0] = 1_u32 << 2;
            words[32] = 1_u32 << 0;
            words
        };
        const LEVEL_ONE: [u32; 2] = [1_u32 << 0, 1_u32 << 0];
        const ROOT: [u32; 1] = [(1_u32 << 0) | (1_u32 << 1)];
        const LEVELS: [&[u32]; 2] = [&LEVEL_ONE, &ROOT];
        const TREE: CooperativeExclusionSummaryTree =
            CooperativeExclusionSummaryTree::new(&LEAF, &LEVELS);

        assert_eq!(
            TREE.span_capacity(),
            33 * COOPERATIVE_EXCLUSION_TREE_WORD_BITS
        );
        assert!(TREE.contains(
            CooperativeExclusionSpan::new(3).expect("span identifiers should be non-zero")
        ));
        assert!(TREE.contains(
            CooperativeExclusionSpan::new(1025).expect("span identifiers should be non-zero")
        ));
        assert!(!TREE.contains(
            CooperativeExclusionSpan::new(4).expect("span identifiers should be non-zero")
        ));
        assert!(!TREE.contains(
            CooperativeExclusionSpan::new(2048).expect("span identifiers should be non-zero")
        ));
    }

    #[test]
    fn green_exclusion_summary_tree_falls_back_honestly_for_spans_beyond_fast_cache() {
        const LEAF: [u32; 33] = {
            let mut words = [0_u32; 33];
            words[32] = 1_u32 << 0;
            words
        };
        const LEVEL_ONE: [u32; 2] = [0, 1_u32 << 0];
        const ROOT: [u32; 1] = [1_u32 << 1];
        const LEVELS: [&[u32]; 2] = [&LEVEL_ONE, &ROOT];
        const TREE: CooperativeExclusionSummaryTree =
            CooperativeExclusionSummaryTree::new(&LEAF, &LEVELS);

        let carrier = ThreadPool::new(&ThreadPoolConfig {
            min_threads: 1,
            max_threads: 1,
            placement: PoolPlacement::Inherit,
            ..ThreadPoolConfig::new()
        })
        .expect("single-carrier pool should build");
        let fibers =
            GreenPool::new(&FiberPoolConfig::new(), &carrier).expect("green pool should build");

        let task = fibers
            .spawn(move || -> Result<(), FiberError> {
                let span = CooperativeExclusionSpan::new(1025).map_err(fiber_error_from_sync)?;
                let _guard =
                    enter_current_green_exclusion_span(span).map_err(fiber_error_from_sync)?;
                assert!(!current_green_exclusion_allows_tree(&TREE));
                Ok(())
            })
            .expect("task should spawn");

        task.join()
            .expect("task should complete without runtime failure")
            .expect("task should observe the overflow fallback");
        fibers
            .shutdown()
            .expect("green pool should shut down cleanly");
        carrier
            .shutdown()
            .expect("carrier pool should shut down cleanly");
    }

    #[test]
    fn green_yield_rejects_when_cooperative_mutex_is_held() {
        let carrier = ThreadPool::new(&ThreadPoolConfig {
            min_threads: 1,
            max_threads: 1,
            placement: PoolPlacement::Inherit,
            ..ThreadPoolConfig::new()
        })
        .expect("single-carrier pool should build");
        let fibers =
            GreenPool::new(&FiberPoolConfig::new(), &carrier).expect("green pool should build");
        let lock = Arc::new(FusionMutex::new(()));

        let task = fibers
            .spawn({
                let lock = Arc::clone(&lock);
                move || -> Result<(), FiberError> {
                    let _guard = lock.lock().map_err(fiber_error_from_sync)?;
                    let error =
                        yield_now().expect_err("yield should reject while cooperative lock held");
                    assert_eq!(error.kind(), FiberError::state_conflict().kind());
                    Ok(())
                }
            })
            .expect("task should spawn");

        task.join()
            .expect("task should complete without runtime failure")
            .expect("task should observe the expected yield rejection");
        fibers
            .shutdown()
            .expect("green pool should shut down cleanly");
        carrier
            .shutdown()
            .expect("carrier pool should shut down cleanly");
    }

    #[test]
    fn ranked_green_mutexes_reject_descending_acquisition_order() {
        let carrier = ThreadPool::new(&ThreadPoolConfig {
            min_threads: 1,
            max_threads: 1,
            placement: PoolPlacement::Inherit,
            ..ThreadPoolConfig::new()
        })
        .expect("single-carrier pool should build");
        let fibers =
            GreenPool::new(&FiberPoolConfig::new(), &carrier).expect("green pool should build");
        let low = Arc::new(FusionMutex::ranked(
            (),
            crate::sync::CooperativeLockRank::new(1).expect("rank one should be valid"),
        ));
        let high = Arc::new(FusionMutex::ranked(
            (),
            crate::sync::CooperativeLockRank::new(2).expect("rank two should be valid"),
        ));

        let task = fibers
            .spawn({
                let low = Arc::clone(&low);
                let high = Arc::clone(&high);
                move || -> Result<(), FiberError> {
                    let _high_guard = high.lock().map_err(fiber_error_from_sync)?;
                    let Err(error) = low.lock() else {
                        panic!("descending ranked acquisition should be rejected");
                    };
                    assert_eq!(error.kind, SyncErrorKind::Invalid);
                    Ok(())
                }
            })
            .expect("task should spawn");

        task.join()
            .expect("task should complete without runtime failure")
            .expect("task should observe the expected ranked-lock rejection");
        fibers
            .shutdown()
            .expect("green pool should shut down cleanly");
        carrier
            .shutdown()
            .expect("carrier pool should shut down cleanly");
    }

    #[test]
    fn ranked_green_mutexes_allow_ascending_acquisition_order() {
        let carrier = ThreadPool::new(&ThreadPoolConfig {
            min_threads: 1,
            max_threads: 1,
            placement: PoolPlacement::Inherit,
            ..ThreadPoolConfig::new()
        })
        .expect("single-carrier pool should build");
        let fibers =
            GreenPool::new(&FiberPoolConfig::new(), &carrier).expect("green pool should build");
        let low = Arc::new(FusionMutex::ranked(
            (),
            crate::sync::CooperativeLockRank::new(1).expect("rank one should be valid"),
        ));
        let high = Arc::new(FusionMutex::ranked(
            (),
            crate::sync::CooperativeLockRank::new(2).expect("rank two should be valid"),
        ));

        let task = fibers
            .spawn({
                let low = Arc::clone(&low);
                let high = Arc::clone(&high);
                move || -> Result<(), FiberError> {
                    let _low_guard = low.lock().map_err(fiber_error_from_sync)?;
                    let _high_guard = high.lock().map_err(fiber_error_from_sync)?;
                    assert_eq!(current_green_cooperative_lock_depth(), 2);
                    Ok(())
                }
            })
            .expect("task should spawn");

        task.join()
            .expect("task should complete without runtime failure")
            .expect("ascending ranked acquisition should succeed");
        fibers
            .shutdown()
            .expect("green pool should shut down cleanly");
        carrier
            .shutdown()
            .expect("carrier pool should shut down cleanly");
    }

    #[test]
    fn green_join_rejects_when_cooperative_mutex_is_held() {
        let carrier = ThreadPool::new(&ThreadPoolConfig {
            min_threads: 1,
            max_threads: 1,
            placement: PoolPlacement::Inherit,
            ..ThreadPoolConfig::new()
        })
        .expect("single-carrier pool should build");
        let fibers = GreenPool::new(
            &FiberPoolConfig::new().with_scheduling(GreenScheduling::Priority),
            &carrier,
        )
        .expect("priority green pool should build");
        let lock = Arc::new(FusionMutex::new(()));
        let child_ran = Arc::new(AtomicBool::new(false));
        let pool_for_parent = fibers
            .try_clone()
            .expect("green pool handle should clone for in-fiber spawn");
        let parent = fibers
            .spawn_with_attrs(
                FiberTaskAttributes::new(FiberStackClass::MIN)
                    .with_priority(FiberTaskPriority::new(10)),
                {
                    let lock = Arc::clone(&lock);
                    let child_ran = Arc::clone(&child_ran);
                    move || -> Result<(), FiberError> {
                        let child = pool_for_parent.spawn_with_attrs(
                            FiberTaskAttributes::new(FiberStackClass::MIN)
                                .with_priority(FiberTaskPriority::new(-10)),
                            move || {
                                child_ran.store(true, Ordering::Release);
                            },
                        )?;
                        let _guard = lock.lock().map_err(fiber_error_from_sync)?;
                        let error = child
                            .join()
                            .expect_err("join should reject while cooperative lock held");
                        assert_eq!(error.kind(), FiberError::state_conflict().kind());
                        Ok(())
                    }
                },
            )
            .expect("parent should spawn");

        parent
            .join()
            .expect("parent should complete without runtime failure")
            .expect("parent should observe the expected join rejection");
        for _ in 0..1_000 {
            if child_ran.load(Ordering::Acquire) {
                break;
            }
            std::thread::yield_now();
        }
        assert!(
            child_ran.load(Ordering::Acquire),
            "child should still run after the parent join attempt is rejected"
        );

        fibers
            .shutdown()
            .expect("green pool should shut down cleanly");
        carrier
            .shutdown()
            .expect("carrier pool should shut down cleanly");
    }

    #[test]
    fn green_task_yield_budget_faults_after_overrun_and_yield() {
        YIELD_BUDGET_EVENT_CALLS.store(0, Ordering::Release);
        LAST_YIELD_BUDGET_FIBER_ID.store(0, Ordering::Release);
        LAST_YIELD_BUDGET_CARRIER_ID.store(usize::MAX, Ordering::Release);
        LAST_YIELD_BUDGET_NANOS.store(0, Ordering::Release);
        LAST_YIELD_OBSERVED_NANOS.store(0, Ordering::Release);

        let carrier = ThreadPool::new(&ThreadPoolConfig {
            min_threads: 1,
            max_threads: 1,
            placement: PoolPlacement::Inherit,
            ..ThreadPoolConfig::new()
        })
        .expect("single-carrier pool should build");
        let fibers = GreenPool::new(
            &FiberPoolConfig::new().with_yield_budget_policy(FiberYieldBudgetPolicy::Notify(
                record_yield_budget_event,
            )),
            &carrier,
        )
        .expect("green pool should build");

        let task = fibers
            .spawn_with_attrs(
                FiberTaskAttributes::new(FiberStackClass::MIN)
                    .with_yield_budget(Duration::from_millis(5)),
                || {
                    std::thread::sleep(Duration::from_millis(15));
                    yield_now().expect("task should still be able to yield after long segment");
                },
            )
            .expect("budgeted task should spawn");
        let task_id = task.id();

        let error = task
            .join()
            .expect_err("run-between-yield overrun should fault the task");
        assert_eq!(error.kind(), FiberError::deadline_exceeded().kind());
        assert_eq!(YIELD_BUDGET_EVENT_CALLS.load(Ordering::Acquire), 1);
        assert_eq!(
            LAST_YIELD_BUDGET_FIBER_ID.load(Ordering::Acquire),
            usize::try_from(task_id).unwrap_or(usize::MAX)
        );
        assert_eq!(LAST_YIELD_BUDGET_CARRIER_ID.load(Ordering::Acquire), 0);
        assert!(
            LAST_YIELD_OBSERVED_NANOS.load(Ordering::Acquire)
                >= LAST_YIELD_BUDGET_NANOS.load(Ordering::Acquire)
        );

        fibers
            .shutdown()
            .expect("green pool should shut down cleanly");
        carrier
            .shutdown()
            .expect("carrier pool should shut down cleanly");
    }

    #[test]
    fn watchdog_faults_non_yielding_green_budget_overrun() {
        YIELD_BUDGET_EVENT_CALLS.store(0, Ordering::Release);
        LAST_YIELD_BUDGET_FIBER_ID.store(0, Ordering::Release);
        LAST_YIELD_BUDGET_CARRIER_ID.store(usize::MAX, Ordering::Release);
        LAST_YIELD_BUDGET_NANOS.store(0, Ordering::Release);
        LAST_YIELD_OBSERVED_NANOS.store(0, Ordering::Release);

        let carrier = ThreadPool::new(&ThreadPoolConfig {
            min_threads: 1,
            max_threads: 1,
            placement: PoolPlacement::Inherit,
            ..ThreadPoolConfig::new()
        })
        .expect("single-carrier pool should build");
        let fibers = GreenPool::new(
            &FiberPoolConfig::new().with_yield_budget_policy(FiberYieldBudgetPolicy::Notify(
                record_yield_budget_event,
            )),
            &carrier,
        )
        .expect("green pool should build");

        let task = fibers
            .spawn_with_attrs(
                FiberTaskAttributes::new(FiberStackClass::MIN)
                    .with_yield_budget(Duration::from_millis(5)),
                || {
                    std::thread::sleep(Duration::from_millis(25));
                },
            )
            .expect("budgeted task should spawn");
        let task_id = task.id();

        let error = task
            .join()
            .expect_err("watchdog should fault one non-yielding overrun");
        assert_eq!(error.kind(), FiberError::deadline_exceeded().kind());
        assert_eq!(YIELD_BUDGET_EVENT_CALLS.load(Ordering::Acquire), 1);
        assert_eq!(
            LAST_YIELD_BUDGET_FIBER_ID.load(Ordering::Acquire),
            usize::try_from(task_id).unwrap_or(usize::MAX)
        );
        assert_eq!(LAST_YIELD_BUDGET_CARRIER_ID.load(Ordering::Acquire), 0);
        assert!(
            LAST_YIELD_OBSERVED_NANOS.load(Ordering::Acquire)
                >= LAST_YIELD_BUDGET_NANOS.load(Ordering::Acquire)
        );

        fibers
            .shutdown()
            .expect("green pool should shut down cleanly");
        carrier
            .shutdown()
            .expect("carrier pool should shut down cleanly");
    }

    #[test]
    fn work_stealing_runs_ready_work_on_an_idle_carrier() {
        if GreenPool::support().context.migration != ContextMigrationSupport::CrossCarrier {
            return;
        }

        let carrier = ThreadPool::new(&ThreadPoolConfig {
            min_threads: 2,
            max_threads: 2,
            placement: PoolPlacement::Inherit,
            ..ThreadPoolConfig::new()
        })
        .expect("two-carrier pool should build");
        let fibers = GreenPool::new(
            &FiberPoolConfig {
                scheduling: GreenScheduling::WorkStealing,
                growth_chunk: 4,
                max_fibers_per_carrier: 4,
                ..FiberPoolConfig::new()
            },
            &carrier,
        )
        .expect("work-stealing fiber pool should build");

        let first_thread = Arc::new(StdMutex::new(None));
        let second_thread = Arc::new(StdMutex::new(None));
        let started = Arc::new(AtomicBool::new(false));
        let release = Arc::new(AtomicBool::new(false));

        fibers.inner.next_carrier.store(0, Ordering::Release);
        let blocker = fibers
            .spawn({
                let first_thread = Arc::clone(&first_thread);
                let started = Arc::clone(&started);
                let release = Arc::clone(&release);
                move || {
                    *first_thread
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner) =
                        Some(std::thread::current().id());
                    started.store(true, Ordering::Release);
                    while !release.load(Ordering::Acquire) {
                        std::thread::yield_now();
                    }
                }
            })
            .expect("blocking fiber should spawn");
        while !started.load(Ordering::Acquire) {
            std::thread::yield_now();
        }

        fibers.inner.next_carrier.store(0, Ordering::Release);
        let stolen = fibers
            .spawn({
                let second_thread = Arc::clone(&second_thread);
                move || {
                    *second_thread
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner) =
                        Some(std::thread::current().id());
                }
            })
            .expect("second fiber should spawn onto the busy source carrier");
        stolen
            .join()
            .expect("idle carrier should steal and complete ready work");

        release.store(true, Ordering::Release);
        blocker
            .join()
            .expect("blocking fiber should finish after release");

        let first = first_thread
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .expect("first fiber should record a carrier thread");
        let second = second_thread
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .expect("stolen fiber should record a carrier thread");
        assert_ne!(first, second);

        fibers
            .shutdown()
            .expect("work-stealing fiber pool should shut down cleanly");
        carrier
            .shutdown()
            .expect("carrier pool should shut down cleanly");
    }

    #[test]
    fn automatic_huge_page_policy_tracks_backend_support_and_reservation_size() {
        let small = automatic_huge_page_policy(FiberStackBacking::Elastic {
            initial_size: NonZeroUsize::new(4 * 1024).expect("non-zero initial stack"),
            max_size: NonZeroUsize::new(64 * 1024).expect("non-zero max stack"),
        });
        assert_eq!(small, HugePagePolicy::Disabled);

        let large = automatic_huge_page_policy(FiberStackBacking::Elastic {
            initial_size: NonZeroUsize::new(4 * 1024).expect("non-zero initial stack"),
            max_size: NonZeroUsize::new(4 * 1024 * 1024).expect("non-zero max stack"),
        });
        let expected = if system_mem()
            .support()
            .advice
            .contains(MemAdviceCaps::HUGE_PAGE)
        {
            HugePagePolicy::Enabled {
                size: HugePageSize::TwoMiB,
            }
        } else {
            HugePagePolicy::Disabled
        };
        assert_eq!(large, expected);
    }

    #[test]
    fn automatic_carrier_selection_prefers_visible_core_count() {
        let summary = fusion_pal::hal::HardwareTopologySummary {
            logical_cpu_count: Some(8),
            core_count: Some(4),
            cluster_count: None,
            package_count: None,
            numa_node_count: None,
            core_class_count: None,
        };
        assert_eq!(select_automatic_carrier_count(summary), Some(4));

        let no_cores = fusion_pal::hal::HardwareTopologySummary {
            core_count: None,
            ..summary
        };
        assert_eq!(select_automatic_carrier_count(no_cores), Some(8));
    }

    #[cfg(feature = "std")]
    #[test]
    fn hosted_carrier_count_policy_reads_requested_topology_count() {
        let summary = fusion_pal::hal::HardwareTopologySummary {
            logical_cpu_count: Some(12),
            core_count: Some(6),
            cluster_count: Some(3),
            package_count: Some(2),
            numa_node_count: None,
            core_class_count: None,
        };
        assert_eq!(
            hosted_carrier_count_from_summary(summary, HostedCarrierCountPolicy::Automatic),
            Some(6)
        );
        assert_eq!(
            hosted_carrier_count_from_summary(
                summary,
                HostedCarrierCountPolicy::VisibleLogicalCpus
            ),
            Some(12)
        );
        assert_eq!(
            hosted_carrier_count_from_summary(summary, HostedCarrierCountPolicy::VisibleCores),
            Some(6)
        );
        assert_eq!(
            hosted_carrier_count_from_summary(summary, HostedCarrierCountPolicy::VisiblePackages),
            Some(2)
        );
    }

    #[test]
    fn per_carrier_capacity_rounds_total_budget_up() {
        assert_eq!(
            per_carrier_capacity_for_total(8, 4).expect("capacity should divide cleanly"),
            2
        );
        assert_eq!(
            per_carrier_capacity_for_total(9, 4).expect("capacity should round up"),
            3
        );
        assert_eq!(
            per_carrier_capacity_for_total(1, 8).expect("single total fiber should still admit"),
            1
        );
        assert_eq!(
            per_carrier_capacity_for_total(0, 1)
                .expect_err("zero total fibers should be rejected")
                .kind(),
            FiberError::invalid().kind()
        );
    }

    #[cfg(feature = "std")]
    #[test]
    fn hosted_class_distribution_rounds_total_slots_and_growth_chunk_up() {
        let classes = [
            HostedFiberClassConfig::new(FiberStackClass::MIN, 5)
                .expect("hosted class config should build")
                .with_growth_chunk(3)
                .expect("hosted growth chunk should build"),
            HostedFiberClassConfig::new(
                FiberStackClass::new(NonZeroUsize::new(8 * 1024).expect("non-zero class"))
                    .expect("valid class"),
                9,
            )
            .expect("hosted class config should build"),
        ];

        let distributed = distribute_hosted_class_configs(&classes, 4)
            .expect("class configs should distribute across carriers");
        assert_eq!(distributed[0].class, FiberStackClass::MIN);
        assert_eq!(distributed[0].slots_per_carrier, 2);
        assert_eq!(distributed[0].growth_chunk, 1);
        assert_eq!(distributed[1].class.size_bytes().get(), 8 * 1024);
        assert_eq!(distributed[1].slots_per_carrier, 3);
        assert_eq!(distributed[1].growth_chunk, 3);
    }

    #[test]
    fn steal_seed_randomizes_the_first_victim_choice() {
        let first = (xorshift64(initial_steal_seed(0)) % 7) + 1;
        let second = (xorshift64(initial_steal_seed(1)) % 7) + 1;
        assert_ne!(first, second);
    }

    #[test]
    fn current_fiber_pool_join_drives_yielding_closure_to_completion() {
        let fibers = CurrentFiberPool::new(&FiberPoolConfig::new())
            .expect("current fiber pool should build");
        let stages = Arc::new(AtomicUsize::new(0));

        let task = fibers
            .spawn({
                let stages = Arc::clone(&stages);
                move || -> Result<u32, FiberError> {
                    stages.fetch_add(1, Ordering::AcqRel);
                    yield_now()?;
                    stages.fetch_add(1, Ordering::AcqRel);
                    Ok(42)
                }
            })
            .expect("yielding task should spawn");

        assert_eq!(
            task.join()
                .expect("current-thread join should drive the pool")
                .expect("task should complete without runtime failure"),
            42
        );
        assert_eq!(stages.load(Ordering::Acquire), 2);

        fibers
            .shutdown()
            .expect("current fiber pool should shut down cleanly");
    }

    #[test]
    fn current_fiber_pool_from_explicit_backing_runs_task() {
        let config =
            FiberPoolConfig::fixed(NonZeroUsize::new(8 * 1024).expect("non-zero stack"), 2)
                .with_guard_pages(0);
        let plan = CurrentFiberPool::backing_plan(&config).expect("backing plan should build");
        let backing = CurrentFiberPoolBacking {
            control: MemoryResourceHandle::from(
                VirtualMemoryResource::create(&ResourceRequest::anonymous_private(
                    plan.control.bytes,
                ))
                .expect("control resource should build"),
            ),
            runtime_metadata: MemoryResourceHandle::from(
                VirtualMemoryResource::create(&ResourceRequest::anonymous_private(
                    plan.runtime_metadata.bytes,
                ))
                .expect("runtime metadata resource should build"),
            ),
            stack_metadata: MemoryResourceHandle::from(
                VirtualMemoryResource::create(&ResourceRequest::anonymous_private(
                    plan.stack_metadata.bytes,
                ))
                .expect("stack metadata resource should build"),
            ),
            stacks: MemoryResourceHandle::from(
                VirtualMemoryResource::create(&ResourceRequest::anonymous_private(
                    plan.stacks.bytes,
                ))
                .expect("stack resource should build"),
            ),
        };
        let fibers = CurrentFiberPool::from_backing(&config, backing)
            .expect("current fiber pool should build from explicit backing");

        let task = fibers
            .spawn(|| -> Result<u32, FiberError> {
                yield_now()?;
                Ok(11)
            })
            .expect("yielding task should spawn");

        assert_eq!(
            task.join()
                .expect("current-thread join should drive the explicit-backed pool")
                .expect("task should complete without runtime failure"),
            11
        );

        fibers
            .shutdown()
            .expect("explicit-backed current fiber pool should shut down cleanly");
    }

    #[test]
    fn global_nearest_round_up_fiber_sizing_inflates_backing_requests() {
        let exact = FiberPoolConfig::fixed(NonZeroUsize::new(8 * 1024).expect("non-zero stack"), 2)
            .with_guard_pages(0);
        let rounded = exact.with_sizing_strategy(RuntimeSizingStrategy::GlobalNearestRoundUp);

        let exact_plan =
            CurrentFiberPool::backing_plan(&exact).expect("exact backing plan should build");
        let rounded_plan =
            CurrentFiberPool::backing_plan(&rounded).expect("rounded backing plan should build");

        assert!(rounded_plan.control.bytes >= exact_plan.control.bytes);
        assert!(rounded_plan.runtime_metadata.bytes >= exact_plan.runtime_metadata.bytes);
        assert!(rounded_plan.stack_metadata.bytes >= exact_plan.stack_metadata.bytes);
        assert!(rounded_plan.stacks.bytes >= exact_plan.stacks.bytes);
        assert!(rounded_plan.control.bytes.is_power_of_two());
        assert!(rounded_plan.runtime_metadata.bytes.is_power_of_two());
        assert!(rounded_plan.stack_metadata.bytes.is_power_of_two());
        assert!(rounded_plan.stacks.bytes.is_power_of_two());
    }

    #[test]
    fn global_nearest_round_up_fiber_internal_mappers_use_rounded_sizes() {
        let support = GreenPool::support();
        let alignment = support.context.min_stack_alignment.max(16);
        let exact = FiberPoolConfig::fixed(NonZeroUsize::new(8 * 1024).expect("non-zero stack"), 2)
            .with_guard_pages(0);
        let rounded = exact.with_sizing_strategy(RuntimeSizingStrategy::GlobalNearestRoundUp);

        let exact_slab = FiberStackSlab::new(&exact, alignment, support.context.stack_direction)
            .expect("exact slab should build");
        let rounded_slab =
            FiberStackSlab::new(&rounded, alignment, support.context.stack_direction)
                .expect("rounded slab should build");

        assert!(rounded_slab.metadata_bytes >= exact_slab.metadata_bytes);
        assert!(rounded_slab.region.len >= exact_slab.region.len);

        let (exact_region, _) =
            green_pool_runtime_regions(1, 2, GreenScheduling::Fifo, false, exact.sizing)
                .expect("exact green runtime region should build");
        let (rounded_region, _) =
            green_pool_runtime_regions(1, 2, GreenScheduling::Fifo, false, rounded.sizing)
                .expect("rounded green runtime region should build");

        assert!(rounded_region.len >= exact_region.len);

        let _ = unsafe { system_mem().unmap(exact_region) };
        let _ = unsafe { system_mem().unmap(rounded_region) };
    }

    #[test]
    fn current_fiber_pool_from_bound_slab_runs_task() {
        let config =
            FiberPoolConfig::fixed(NonZeroUsize::new(8 * 1024).expect("non-zero stack"), 2)
                .with_guard_pages(0);
        let layout = CurrentFiberPool::backing_plan(&config)
            .expect("backing plan should build")
            .combined()
            .expect("combined layout should build");
        let slab = aligned_bound_resource(layout.slab.bytes, layout.slab.align);
        let fibers = CurrentFiberPool::from_bound_slab(&config, slab)
            .expect("current fiber pool should build from one bound slab");

        let task = fibers
            .spawn(|| -> Result<u32, FiberError> {
                yield_now()?;
                Ok(17)
            })
            .expect("yielding task should spawn");

        assert_eq!(
            task.join()
                .expect("current-thread join should drive the bound-slab pool")
                .expect("task should complete without runtime failure"),
            17
        );

        fibers
            .shutdown()
            .expect("bound-slab current fiber pool should shut down cleanly");
    }

    #[test]
    fn current_fiber_pool_from_exact_aligned_bound_slab_runs_task() {
        let config =
            FiberPoolConfig::fixed(NonZeroUsize::new(8 * 1024).expect("non-zero stack"), 2)
                .with_guard_pages(0);
        let conservative = CurrentFiberPool::backing_plan(&config)
            .expect("backing plan should build")
            .combined()
            .expect("conservative layout should build");
        let exact = CurrentFiberPool::backing_plan(&config)
            .expect("backing plan should build")
            .combined_for_base_alignment(conservative.slab.align)
            .expect("exact-aligned layout should build");
        let slab = aligned_bound_resource(exact.slab.bytes, exact.slab.align);
        let fibers = CurrentFiberPool::from_bound_slab(&config, slab)
            .expect("current fiber pool should build from exact-aligned slab");

        let task = fibers
            .spawn(|| -> Result<u32, FiberError> {
                yield_now()?;
                Ok(19)
            })
            .expect("yielding task should spawn");

        assert_eq!(
            task.join()
                .expect("current-thread join should drive the exact-aligned bound-slab pool")
                .expect("task should complete without runtime failure"),
            19
        );

        fibers
            .shutdown()
            .expect("exact-aligned bound-slab current fiber pool should shut down cleanly");
    }

    #[test]
    fn current_fiber_pool_from_bound_slab_reuses_slots_across_many_noop_spawns() {
        let config =
            FiberPoolConfig::fixed(NonZeroUsize::new(8 * 1024).expect("non-zero stack"), 1)
                .with_guard_pages(0);
        let layout = CurrentFiberPool::backing_plan(&config)
            .expect("backing plan should build")
            .combined()
            .expect("combined layout should build");
        let slab = aligned_bound_resource(layout.slab.bytes, layout.slab.align);
        let fibers = CurrentFiberPool::from_bound_slab(&config, slab)
            .expect("current fiber pool should build from one bound slab");

        for _ in 0..128 {
            let handle = fibers
                .spawn(|| 1_u32)
                .expect("noop task should spawn repeatedly");
            assert_eq!(handle.join().expect("noop task should join repeatedly"), 1);
        }

        fibers
            .shutdown()
            .expect("bound-slab current fiber pool should shut down cleanly");
    }

    #[test]
    fn uniform_bootstrap_uses_requested_stack_size() {
        let bootstrap = FiberPoolBootstrap::uniform(
            4,
            NonZeroUsize::new(16 * 1024).expect("non-zero uniform stack"),
        );
        assert_eq!(
            bootstrap.config().stack_backing,
            FiberStackBacking::Fixed {
                stack_size: NonZeroUsize::new(16 * 1024).expect("non-zero uniform stack"),
            }
        );
        assert_eq!(bootstrap.config().max_fibers_per_carrier, 4);
    }

    #[test]
    fn current_fiber_pool_run_until_idle_drives_multiple_ready_segments() {
        let fibers = CurrentFiberPool::new(&FiberPoolConfig::new())
            .expect("current fiber pool should build");
        let total = Arc::new(AtomicUsize::new(0));

        let first = fibers
            .spawn({
                let total = Arc::clone(&total);
                move || {
                    total.fetch_add(1, Ordering::AcqRel);
                    yield_now().expect("first task should yield cleanly");
                    total.fetch_add(10, Ordering::AcqRel);
                }
            })
            .expect("first current-thread task should spawn");
        let second = fibers
            .spawn({
                let total = Arc::clone(&total);
                move || {
                    total.fetch_add(100, Ordering::AcqRel);
                }
            })
            .expect("second current-thread task should spawn");

        assert_eq!(
            fibers
                .run_until_idle()
                .expect("current-thread pool should drive until idle"),
            3
        );
        assert_eq!(total.load(Ordering::Acquire), 111);
        first
            .join()
            .expect("first task should already be complete after run_until_idle");
        second
            .join()
            .expect("second task should already be complete after run_until_idle");

        fibers
            .shutdown()
            .expect("current fiber pool should shut down cleanly");
    }

    #[test]
    fn current_fiber_pool_spawns_generated_contract_tasks() {
        let classes = [FiberStackClassConfig::new(
            FiberStackClass::new(NonZeroUsize::new(8 * 1024).expect("non-zero class"))
                .expect("valid class"),
            1,
        )
        .expect("valid class config")];
        let fibers = CurrentFiberPool::new(
            &FiberPoolConfig::classed(&classes).expect("classed config should build"),
        )
        .expect("current fiber pool should build");

        fibers
            .spawn_generated_contract(SupportedGeneratedContractTask)
            .expect("generated-contract task should spawn")
            .join()
            .expect("generated-contract task should complete");

        fibers
            .shutdown()
            .expect("current fiber pool should shut down cleanly");
    }

    #[test]
    fn current_fiber_pool_spawn_with_stack_admits_closure_override() {
        let classes = [FiberStackClassConfig::new(
            FiberStackClass::new(NonZeroUsize::new(4 * 1024).expect("non-zero class"))
                .expect("valid class"),
            1,
        )
        .expect("valid class config")];
        let fibers = CurrentFiberPool::new(
            &FiberPoolConfig::classed(&classes).expect("classed config should build"),
        )
        .expect("current fiber pool should build");

        assert_eq!(
            fibers
                .spawn_with_stack::<4096, _, _>(|| 7_u32)
                .expect("stack-constrained closure should spawn")
                .join()
                .expect("stack-constrained closure should complete"),
            7
        );

        let error = fibers
            .spawn_with_stack::<8192, _, _>(|| ())
            .expect_err("unsupported stack class should be rejected");
        assert_eq!(error.kind(), FiberError::unsupported().kind());

        fibers
            .shutdown()
            .expect("current fiber pool should shut down cleanly");
    }

    #[test]
    fn current_fiber_pool_handles_report_execution_mode() {
        let fibers = CurrentFiberPool::new(&FiberPoolConfig::new())
            .expect("current fiber pool should build");

        let inline = fibers
            .spawn_explicit(SupportedInlineNoYieldTask)
            .expect("inline task should spawn");
        let inline_admission = inline
            .admission()
            .expect("inline admission should be observable");
        assert_eq!(
            inline
                .execution()
                .expect("inline execution should be observable"),
            FiberTaskExecution::InlineNoYield
        );
        assert!(
            inline
                .runs_inline()
                .expect("inline realization should be observable")
        );
        assert_eq!(
            inline_admission.execution,
            FiberTaskExecution::InlineNoYield
        );
        assert_eq!(inline_admission.priority, FiberTaskPriority::DEFAULT);
        assert_eq!(inline_admission.yield_budget, None);
        assert_eq!(inline.join().expect("inline task should complete"), 17);

        let yielding = fibers
            .spawn(|| -> Result<(), FiberError> {
                yield_now()?;
                Ok(())
            })
            .expect("yielding task should spawn");
        let yielding_admission = yielding
            .admission()
            .expect("fiber admission should be observable");
        assert_eq!(
            yielding
                .execution()
                .expect("fiber execution should be observable"),
            FiberTaskExecution::Fiber
        );
        assert!(
            !yielding
                .runs_inline()
                .expect("fiber realization should be observable")
        );
        assert_eq!(yielding_admission.execution, FiberTaskExecution::Fiber);
        assert_eq!(yielding_admission.priority, FiberTaskPriority::DEFAULT);
        assert_eq!(yielding_admission.yield_budget, None);
        yielding
            .join()
            .expect("yielding task should complete")
            .expect("yielding task should not fail");

        fibers
            .shutdown()
            .expect("current fiber pool should shut down cleanly");
    }

    #[test]
    fn current_fiber_pool_runs_no_yield_tasks_inline_without_stack_admission() {
        let fibers = CurrentFiberPool::new(
            &FiberPoolConfig::fixed(NonZeroUsize::new(4 * 1024).expect("non-zero stack"), 1)
                .with_telemetry(FiberTelemetry::Full),
        )
        .expect("current fiber pool should build");

        assert_eq!(
            fibers
                .spawn_explicit(SupportedInlineNoYieldTask)
                .expect("inline no-yield task should spawn")
                .join()
                .expect("inline no-yield task should complete"),
            17
        );
        assert_eq!(
            fibers
                .stack_stats()
                .expect("telemetry should be enabled")
                .peak_used_bytes,
            0
        );

        fibers
            .shutdown()
            .expect("current fiber pool should shut down cleanly");
    }

    #[test]
    fn fiber_pool_bootstrap_fixed_builds_current_thread_pool() {
        let fibers = FiberPoolBootstrap::fixed(2)
            .build_current()
            .expect("bootstrap should build one current-thread pool");
        assert_eq!(fibers.active_count(), 0);
        fibers
            .shutdown()
            .expect("current fiber pool should shut down cleanly");
    }

    #[test]
    fn fixed_growing_config_commits_by_requested_chunk() {
        let config = FiberPoolConfig::fixed_growing(
            NonZeroUsize::new(4 * 1024).expect("non-zero stack"),
            8,
            2,
        )
        .expect("fixed growing config should build");
        let slab = FiberStackSlab::new(
            &config,
            align_of::<usize>(),
            FiberSystem::new().support().context.stack_direction,
        )
        .expect("fixed growing slab should build");

        assert_eq!(slab.initial_slots, 2);
        assert_eq!(slab.chunk_size, 2);
        assert!(matches!(slab.growth, GreenGrowth::OnDemand));
    }

    #[test]
    fn fixed_growing_config_rejects_invalid_chunk() {
        assert!(matches!(
            FiberPoolConfig::fixed_growing(
                NonZeroUsize::new(4 * 1024).expect("non-zero stack"),
                4,
                0,
            ),
            Err(error) if error.kind() == FiberError::invalid().kind()
        ));
        assert!(matches!(
            FiberPoolConfig::fixed_growing(
                NonZeroUsize::new(4 * 1024).expect("non-zero stack"),
                4,
                5,
            ),
            Err(error) if error.kind() == FiberError::invalid().kind()
        ));
    }

    #[test]
    fn current_fiber_pool_fixed_growing_runs_tasks() {
        let fibers = CurrentFiberPool::new(
            &FiberPoolConfig::fixed_growing(
                NonZeroUsize::new(4 * 1024).expect("non-zero stack"),
                4,
                1,
            )
            .expect("fixed growing config should build"),
        )
        .expect("current fixed-growing pool should build");

        assert_eq!(
            fibers
                .spawn(|| 11_u32)
                .expect("task should spawn")
                .join()
                .expect("task should complete"),
            11
        );

        fibers
            .shutdown()
            .expect("current fiber pool should shut down cleanly");
    }

    #[cfg(feature = "std")]
    #[test]
    fn hosted_fiber_runtime_bootstrap_builds_automatic_carriers() {
        let runtime = FiberPoolBootstrap::fixed(2)
            .build_hosted()
            .expect("hosted runtime should build");
        assert_eq!(
            runtime.carriers().bootstrap(),
            HostedCarrierBootstrap::Direct
        );
        assert!(
            runtime
                .carriers()
                .worker_count()
                .expect("worker count should be observable")
                >= 1
        );
        assert_eq!(runtime.fibers().active_count(), 0);
        let (mut carriers, fibers) = runtime.into_parts();
        fibers
            .shutdown()
            .expect("hosted fiber pool should shut down cleanly");
        carriers
            .shutdown()
            .expect("carrier pool should shut down cleanly");
    }

    #[cfg(feature = "std")]
    #[test]
    fn hosted_fiber_runtime_fixed_growing_builds_from_total_budget() {
        let runtime = HostedFiberRuntime::fixed_growing_with_config(
            4,
            1,
            HostedFiberRuntimeConfig::new(1).with_placement(PoolPlacement::Inherit),
        )
        .expect("fixed growing hosted runtime should build");
        assert_eq!(runtime.fibers().active_count(), 0);
        let (mut carriers, fibers) = runtime.into_parts();
        fibers
            .shutdown()
            .expect("hosted fiber pool should shut down cleanly");
        carriers
            .shutdown()
            .expect("carrier pool should shut down cleanly");
    }

    #[cfg(feature = "std")]
    #[test]
    fn green_pool_skips_yield_budget_watchdog_without_budgeted_tasks() {
        let carriers = ThreadPool::new(&ThreadPoolConfig {
            min_threads: 2,
            max_threads: 2,
            placement: PoolPlacement::Inherit,
            ..ThreadPoolConfig::new()
        })
        .expect("carrier pool should build");
        let fibers = GreenPool::new(
            &FiberPoolConfig::fixed_growing(
                NonZeroUsize::new(4 * 1024).expect("non-zero stack"),
                2,
                1,
            )
            .expect("fixed growing config should build")
            .with_reactor_policy(GreenReactorPolicy::Disabled),
            &carriers,
        )
        .expect("green pool should build");

        assert!(
            !fibers
                .inner
                .yield_budget_runtime
                .watchdog_started
                .load(Ordering::Acquire)
        );
        fibers
            .spawn(|| ())
            .expect("task should spawn")
            .join()
            .expect("task should complete");
        assert!(
            !fibers
                .inner
                .yield_budget_runtime
                .watchdog_started
                .load(Ordering::Acquire)
        );

        fibers
            .shutdown()
            .expect("green pool should shut down cleanly");
        carriers
            .shutdown()
            .expect("carrier pool should shut down cleanly");
    }

    #[cfg(feature = "std")]
    #[test]
    fn green_pool_starts_yield_budget_watchdog_for_budgeted_tasks() {
        let carriers = ThreadPool::new(&ThreadPoolConfig {
            min_threads: 2,
            max_threads: 2,
            placement: PoolPlacement::Inherit,
            ..ThreadPoolConfig::new()
        })
        .expect("carrier pool should build");
        let fibers = GreenPool::new(
            &FiberPoolConfig::fixed_growing(
                NonZeroUsize::new(4 * 1024).expect("non-zero stack"),
                2,
                1,
            )
            .expect("fixed growing config should build")
            .with_reactor_policy(GreenReactorPolicy::Disabled)
            .with_yield_budget_policy(FiberYieldBudgetPolicy::Notify(record_yield_budget_event)),
            &carriers,
        )
        .expect("green pool should build");

        fibers
            .spawn_with_attrs(
                FiberTaskAttributes::new(FiberStackClass::MIN)
                    .with_yield_budget(Duration::from_millis(5)),
                || (),
            )
            .expect("budgeted task should spawn")
            .join()
            .expect("budgeted task should complete");

        for _ in 0..1_000 {
            if fibers
                .inner
                .yield_budget_runtime
                .watchdog_started
                .load(Ordering::Acquire)
            {
                break;
            }
            std::thread::yield_now();
        }
        assert!(
            fibers
                .inner
                .yield_budget_runtime
                .watchdog_started
                .load(Ordering::Acquire)
        );

        fibers
            .shutdown()
            .expect("green pool should shut down cleanly");
        carriers
            .shutdown()
            .expect("carrier pool should shut down cleanly");
    }

    #[cfg(feature = "std")]
    #[test]
    fn hosted_green_yield_once_batch_fits_with_16k_stacks() {
        let carriers = ThreadPool::new(&ThreadPoolConfig {
            min_threads: 4,
            max_threads: 4,
            placement: PoolPlacement::Inherit,
            ..ThreadPoolConfig::new()
        })
        .expect("carrier pool should build");
        let fibers = GreenPool::new(
            &FiberPoolConfig::fixed_growing(
                NonZeroUsize::new(16 * 1024).expect("non-zero stack"),
                16,
                4,
            )
            .expect("fixed growing config should build")
            .with_telemetry(FiberTelemetry::Full)
            .with_reactor_policy(GreenReactorPolicy::Disabled),
            &carriers,
        )
        .expect("green pool should build");

        let mut handles = Vec::new();
        for _ in 0..16 {
            handles.push(
                fibers
                    .spawn(|| {
                        yield_now().expect("yield should work");
                    })
                    .expect("task should spawn"),
            );
        }
        for handle in handles {
            handle.join().expect("task should complete");
        }

        let stats = fibers.stack_stats().expect("telemetry should be enabled");
        assert!(stats.peak_used_bytes <= 8 * 1024);

        fibers
            .shutdown()
            .expect("green pool should shut down cleanly");
        carriers
            .shutdown()
            .expect("carrier pool should shut down cleanly");
    }

    #[cfg(feature = "std")]
    #[test]
    fn hosted_green_inline_no_yield_spawn_join_stress_completes() {
        let carriers = ThreadPool::new(&ThreadPoolConfig {
            min_threads: 1,
            max_threads: 1,
            placement: PoolPlacement::Inherit,
            ..ThreadPoolConfig::new()
        })
        .expect("carrier pool should build");
        let fibers = GreenPool::new(
            &FiberPoolConfig::fixed_growing(
                NonZeroUsize::new(4 * 1024).expect("non-zero stack"),
                4,
                1,
            )
            .expect("fixed growing config should build")
            .with_reactor_policy(GreenReactorPolicy::Disabled),
            &carriers,
        )
        .expect("green pool should build");

        for _ in 0..1_000 {
            fibers
                .spawn_explicit(SupportedInlineNoYieldTask)
                .expect("inline no-yield task should spawn")
                .join()
                .expect("inline no-yield task should complete");
        }

        fibers
            .shutdown()
            .expect("green pool should shut down cleanly");
        carriers
            .shutdown()
            .expect("carrier pool should shut down cleanly");
    }

    #[cfg(feature = "std")]
    #[test]
    fn hosted_green_inline_no_yield_rapid_reuse_stays_alive() {
        let carriers = ThreadPool::new(&ThreadPoolConfig {
            min_threads: 1,
            max_threads: 1,
            placement: PoolPlacement::Inherit,
            ..ThreadPoolConfig::new()
        })
        .expect("carrier pool should build");
        let fibers = GreenPool::new(
            &FiberPoolConfig::fixed(NonZeroUsize::new(16 * 1024).expect("non-zero stack"), 64),
            &carriers,
        )
        .expect("green pool should build");

        for _ in 0..1_000 {
            fibers
                .spawn_explicit(SupportedInlineNoYieldTask)
                .expect("inline no-yield task should spawn")
                .join()
                .expect("inline no-yield task should complete");
        }

        fibers
            .shutdown()
            .expect("green pool should shut down cleanly");
        carriers
            .shutdown()
            .expect("carrier pool should shut down cleanly");
    }

    #[cfg(feature = "std")]
    #[test]
    fn hosted_green_yield_once_rapid_reuse_stays_alive() {
        let carriers = ThreadPool::new(&ThreadPoolConfig {
            min_threads: 1,
            max_threads: 1,
            placement: PoolPlacement::Inherit,
            ..ThreadPoolConfig::new()
        })
        .expect("carrier pool should build");
        let fibers = GreenPool::new(
            &FiberPoolConfig::fixed(NonZeroUsize::new(16 * 1024).expect("non-zero stack"), 64),
            &carriers,
        )
        .expect("green pool should build");

        for _ in 0..1_000 {
            fibers
                .spawn(|| {
                    yield_now().expect("yield should work");
                })
                .expect("yielding task should spawn")
                .join()
                .expect("yielding task should complete");
        }

        fibers
            .shutdown()
            .expect("green pool should shut down cleanly");
        carriers
            .shutdown()
            .expect("carrier pool should shut down cleanly");
    }

    #[cfg(feature = "std")]
    #[test]
    fn hosted_fiber_runtime_classed_builds_from_total_budget() {
        let runtime = HostedFiberRuntime::classed(&[
            HostedFiberClassConfig::new(FiberStackClass::MIN, 2)
                .expect("hosted class config should build"),
            HostedFiberClassConfig::new(
                FiberStackClass::new(NonZeroUsize::new(8 * 1024).expect("non-zero class"))
                    .expect("valid class"),
                2,
            )
            .expect("hosted class config should build"),
        ])
        .expect("classed hosted runtime should build");
        assert_eq!(runtime.fibers().active_count(), 0);
        let (mut carriers, fibers) = runtime.into_parts();
        fibers
            .shutdown()
            .expect("hosted fiber pool should shut down cleanly");
        carriers
            .shutdown()
            .expect("carrier pool should shut down cleanly");
    }

    #[cfg(feature = "std")]
    #[test]
    fn hosted_runtime_config_defaults_to_fusion_carrier_shape() {
        let automatic = HostedFiberRuntimeConfig::automatic();
        assert!(automatic.carrier_count >= 1);
        assert_eq!(automatic.name_prefix, Some("fusion-fiber"));

        let explicit = HostedFiberRuntimeConfig::new(2)
            .with_placement(PoolPlacement::PerCore)
            .with_name_prefix(Some("fusion-explicit"));
        assert_eq!(explicit.carrier_count, 2);
        assert_eq!(explicit.placement, PoolPlacement::PerCore);
        assert_eq!(explicit.name_prefix, Some("fusion-explicit"));
    }

    #[cfg(feature = "std")]
    #[test]
    fn hosted_fiber_runtime_respects_explicit_carrier_config() {
        let runtime = FiberPoolBootstrap::fixed(2)
            .build_hosted_with(
                HostedFiberRuntimeConfig::new(1)
                    .with_placement(PoolPlacement::Inherit)
                    .with_name_prefix(Some("fusion-test")),
            )
            .expect("hosted runtime should build from explicit carrier config");
        assert_eq!(
            runtime
                .carriers()
                .worker_count()
                .expect("worker count should be observable"),
            1
        );
        assert_eq!(
            runtime.carriers().bootstrap(),
            HostedCarrierBootstrap::Direct
        );
        let (mut carriers, fibers) = runtime.into_parts();
        fibers
            .shutdown()
            .expect("hosted fiber pool should shut down cleanly");
        carriers
            .shutdown()
            .expect("carrier pool should shut down cleanly");
    }

    #[cfg(feature = "std")]
    #[test]
    fn hosted_fiber_runtime_can_use_composed_thread_pool_bootstrap() {
        let runtime = FiberPoolBootstrap::fixed(2)
            .build_hosted_with(
                HostedFiberRuntimeConfig::new(1)
                    .with_bootstrap(HostedCarrierBootstrap::ThreadPool)
                    .with_placement(PoolPlacement::Inherit),
            )
            .expect("hosted runtime should build from composed carrier config");
        assert_eq!(
            runtime.carriers().bootstrap(),
            HostedCarrierBootstrap::ThreadPool
        );
        assert_eq!(
            runtime
                .carriers()
                .worker_count()
                .expect("worker count should be observable"),
            1
        );
        assert!(runtime.carriers().thread_pool().is_some());
        let (mut carriers, fibers) = runtime.into_parts();
        fibers
            .shutdown()
            .expect("hosted fiber pool should shut down cleanly");
        carriers
            .shutdown()
            .expect("carrier pool should shut down cleanly");
    }
}

const fn fiber_error_from_thread_pool(error: super::ThreadPoolError) -> FiberError {
    match error.kind() {
        fusion_sys::thread::ThreadErrorKind::Unsupported => FiberError::unsupported(),
        fusion_sys::thread::ThreadErrorKind::ResourceExhausted => FiberError::resource_exhausted(),
        fusion_sys::thread::ThreadErrorKind::Busy
        | fusion_sys::thread::ThreadErrorKind::Timeout
        | fusion_sys::thread::ThreadErrorKind::StateConflict => FiberError::state_conflict(),
        fusion_sys::thread::ThreadErrorKind::Invalid
        | fusion_sys::thread::ThreadErrorKind::PermissionDenied
        | fusion_sys::thread::ThreadErrorKind::PlacementDenied
        | fusion_sys::thread::ThreadErrorKind::SchedulerDenied
        | fusion_sys::thread::ThreadErrorKind::StackDenied
        | fusion_sys::thread::ThreadErrorKind::Platform(_) => FiberError::invalid(),
    }
}

const fn fiber_error_from_sync(error: SyncError) -> FiberError {
    match error.kind {
        SyncErrorKind::Unsupported => FiberError::unsupported(),
        SyncErrorKind::Invalid | SyncErrorKind::Overflow => FiberError::invalid(),
        SyncErrorKind::Busy | SyncErrorKind::PermissionDenied | SyncErrorKind::Platform(_) => {
            FiberError::state_conflict()
        }
    }
}

const fn fiber_error_from_mem(error: fusion_pal::sys::mem::MemError) -> FiberError {
    match error.kind {
        fusion_pal::sys::mem::MemErrorKind::Unsupported => FiberError::unsupported(),
        fusion_pal::sys::mem::MemErrorKind::InvalidInput
        | fusion_pal::sys::mem::MemErrorKind::InvalidAddress
        | fusion_pal::sys::mem::MemErrorKind::Misaligned
        | fusion_pal::sys::mem::MemErrorKind::OutOfBounds
        | fusion_pal::sys::mem::MemErrorKind::PermissionDenied
        | fusion_pal::sys::mem::MemErrorKind::Overflow => FiberError::invalid(),
        fusion_pal::sys::mem::MemErrorKind::OutOfMemory => FiberError::resource_exhausted(),
        fusion_pal::sys::mem::MemErrorKind::Busy
        | fusion_pal::sys::mem::MemErrorKind::Platform(_) => FiberError::state_conflict(),
    }
}

const fn fiber_error_from_event(error: fusion_sys::event::EventError) -> FiberError {
    match error.kind() {
        fusion_sys::event::EventErrorKind::Unsupported => FiberError::unsupported(),
        fusion_sys::event::EventErrorKind::Invalid => FiberError::invalid(),
        fusion_sys::event::EventErrorKind::ResourceExhausted => FiberError::resource_exhausted(),
        fusion_sys::event::EventErrorKind::Busy
        | fusion_sys::event::EventErrorKind::Timeout
        | fusion_sys::event::EventErrorKind::StateConflict
        | fusion_sys::event::EventErrorKind::Platform(_) => FiberError::state_conflict(),
    }
}

const fn fiber_error_from_host(error: FiberHostError) -> FiberError {
    match error.kind() {
        FiberHostErrorKind::Unsupported => FiberError::unsupported(),
        FiberHostErrorKind::Invalid => FiberError::invalid(),
        FiberHostErrorKind::ResourceExhausted => FiberError::resource_exhausted(),
        FiberHostErrorKind::StateConflict | FiberHostErrorKind::Platform(_) => {
            FiberError::state_conflict()
        }
    }
}
