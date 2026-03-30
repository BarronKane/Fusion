//! Tier-aware green-thread orchestration.

use core::sync::atomic::{AtomicU32, Ordering};

use crate::sync::{Mutex as SyncMutex, SyncError};
use fusion_pal::contract::pal::HardwareTopologyQuery as _;
use fusion_pal::sys::cpu::system_cpu;
use fusion_sys::fiber::FiberError;
use fusion_sys::thread::ThreadGuarantee;
use fusion_sys::vector::{
    IrqSlot,
    SealedVectorTable,
    VectorDispatchCookie,
    VectorDispatchLane,
    VectorError,
    VectorPriority,
    VectorTableBuilder,
};

use super::{
    ExplicitFiberTask,
    FiberTaskAttributes,
    FiberTaskPriority,
    GeneratedExplicitFiberTask,
    GreenHandle,
    GreenPool,
    GreenPoolConfig,
    ThreadPool,
    ThreadPoolConfig,
    ThreadPoolError,
};

use super::{GeneratedExplicitFiberTaskContract, generated_explicit_task_contract_attributes};

const TIERED_VECTOR_TARGET_CAPACITY: usize = 128;

#[derive(Debug, Clone, Copy)]
struct TieredVectorDispatchTarget {
    task: TieredTaskAttributes,
    job: fn(),
}

static TIERED_VECTOR_TARGETS: SyncMutex<
    [Option<TieredVectorDispatchTarget>; TIERED_VECTOR_TARGET_CAPACITY],
> = SyncMutex::new([None; TIERED_VECTOR_TARGET_CAPACITY]);
static TIERED_VECTOR_DISPATCH_COUNTS: [AtomicU32; TIERED_VECTOR_TARGET_CAPACITY] =
    [const { AtomicU32::new(0) }; TIERED_VECTOR_TARGET_CAPACITY];

/// Carrier execution tier used for multiplexed green work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CarrierTier {
    /// Throughput-biased carrier lane, intended for performance cores.
    Performance,
    /// Background / efficiency-biased carrier lane.
    Efficiency,
}

/// Placement target for one green task relative to the tiered carrier lanes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TaskPlacement {
    /// Let the configured auto-placement heuristic choose the carrier tier.
    #[default]
    Auto,
    /// Route work explicitly to the selected carrier tier.
    Tier(CarrierTier),
}

/// Auto-placement policy for routing green work across carrier tiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AutoCarrierPolicy {
    /// Tasks at or below this priority route to the efficiency tier under `Auto`.
    pub efficiency_priority_ceiling: FiberTaskPriority,
}

impl AutoCarrierPolicy {
    /// Returns the default policy: negative-priority work routes to blue/efficiency carriers.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            efficiency_priority_ceiling: FiberTaskPriority::new(-1),
        }
    }
}

impl Default for AutoCarrierPolicy {
    fn default() -> Self {
        Self::new()
    }
}

/// Task-side routing metadata kept separate from stack and queue priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TieredTaskAttributes {
    /// Fiber-side admission metadata.
    pub fiber: FiberTaskAttributes,
    /// Carrier-tier placement policy.
    pub placement: TaskPlacement,
}

impl TieredTaskAttributes {
    /// Creates one tiered task bundle with automatic carrier placement.
    #[must_use]
    pub const fn new(fiber: FiberTaskAttributes) -> Self {
        Self {
            fiber,
            placement: TaskPlacement::Auto,
        }
    }

    /// Returns one copy of this bundle with explicit carrier placement.
    #[must_use]
    pub const fn with_placement(mut self, placement: TaskPlacement) -> Self {
        self.placement = placement;
        self
    }
}

/// Static carrier-tier support snapshot for the current backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CarrierTierSupport {
    /// Whether machine topology exposes more than one heterogeneous core class.
    pub asymmetric_core_classes: bool,
    /// Strength of carrier-tier partitioning by core class.
    pub tier_partitioning: ThreadGuarantee,
    /// Strength of core-class affinity controls.
    pub core_class_affinity: ThreadGuarantee,
    /// Strength of logical-CPU affinity controls.
    pub logical_cpu_affinity: ThreadGuarantee,
}

/// Public configuration for the tiered green-thread wrapper.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TieredGreenPoolConfig<'a> {
    /// Carrier pool backing the performance tier.
    pub performance_carrier: ThreadPoolConfig<'a>,
    /// Green pool riding the performance carriers.
    pub performance_green: GreenPoolConfig<'a>,
    /// Carrier pool backing the efficiency tier.
    pub efficiency_carrier: ThreadPoolConfig<'a>,
    /// Green pool riding the efficiency carriers.
    pub efficiency_green: GreenPoolConfig<'a>,
    /// Automatic task-placement heuristic.
    pub auto_policy: AutoCarrierPolicy,
}

impl TieredGreenPoolConfig<'static> {
    /// Returns a minimal dual-tier configuration with separate green/blue carrier domains.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            performance_carrier: ThreadPoolConfig {
                name_prefix: Some("fusion-green"),
                ..ThreadPoolConfig::new()
            },
            performance_green: GreenPoolConfig::new(),
            efficiency_carrier: ThreadPoolConfig {
                name_prefix: Some("fusion-blue"),
                ..ThreadPoolConfig::new()
            },
            efficiency_green: GreenPoolConfig::new(),
            auto_policy: AutoCarrierPolicy::new(),
        }
    }
}

impl<'a> TieredGreenPoolConfig<'a> {
    /// Returns one copy with an explicit auto-placement policy.
    #[must_use]
    pub const fn with_auto_policy(mut self, auto_policy: AutoCarrierPolicy) -> Self {
        self.auto_policy = auto_policy;
        self
    }

    /// Returns one copy with an explicit performance-tier carrier pool.
    #[must_use]
    pub const fn with_performance_carrier(
        mut self,
        performance_carrier: ThreadPoolConfig<'a>,
    ) -> Self {
        self.performance_carrier = performance_carrier;
        self
    }

    /// Returns one copy with an explicit performance-tier green pool.
    #[must_use]
    pub const fn with_performance_green(mut self, performance_green: GreenPoolConfig<'a>) -> Self {
        self.performance_green = performance_green;
        self
    }

    /// Returns one copy with an explicit efficiency-tier carrier pool.
    #[must_use]
    pub const fn with_efficiency_carrier(
        mut self,
        efficiency_carrier: ThreadPoolConfig<'a>,
    ) -> Self {
        self.efficiency_carrier = efficiency_carrier;
        self
    }

    /// Returns one copy with an explicit efficiency-tier green pool.
    #[must_use]
    pub const fn with_efficiency_green(mut self, efficiency_green: GreenPoolConfig<'a>) -> Self {
        self.efficiency_green = efficiency_green;
        self
    }
}

impl Default for TieredGreenPoolConfig<'static> {
    fn default() -> Self {
        Self::new()
    }
}

/// Error returned by the tiered green-thread wrapper.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TieredGreenPoolError {
    /// The requested tiered behavior cannot be realized honestly.
    Unsupported,
    /// Carrier-pool creation or control failed.
    ThreadPool(ThreadPoolError),
    /// Green-pool admission or execution failed.
    Green(FiberError),
    /// Vector binding or observation failed.
    Vector(VectorError),
    /// Synchronization over the tiered dispatch registry failed.
    Sync(SyncError),
}

impl TieredGreenPoolError {
    /// Returns the coarse error class.
    #[must_use]
    pub const fn unsupported() -> Self {
        Self::Unsupported
    }
}

impl From<ThreadPoolError> for TieredGreenPoolError {
    fn from(value: ThreadPoolError) -> Self {
        Self::ThreadPool(value)
    }
}

impl From<FiberError> for TieredGreenPoolError {
    fn from(value: FiberError) -> Self {
        Self::Green(value)
    }
}

impl From<VectorError> for TieredGreenPoolError {
    fn from(value: VectorError) -> Self {
        Self::Vector(value)
    }
}

impl From<SyncError> for TieredGreenPoolError {
    fn from(value: SyncError) -> Self {
        Self::Sync(value)
    }
}

/// Tier-aware scheduler snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TieredGreenPoolStats {
    /// Active performance-tier carrier workers.
    pub performance_workers: usize,
    /// Active efficiency-tier carrier workers.
    pub efficiency_workers: usize,
    /// Queued work on performance-tier carriers.
    pub performance_queued: usize,
    /// Queued work on efficiency-tier carriers.
    pub efficiency_queued: usize,
    /// Active green tasks on the performance tier.
    pub performance_green_threads: usize,
    /// Active green tasks on the efficiency tier.
    pub efficiency_green_threads: usize,
}

/// Green-thread wrapper exposing green/performance and blue/efficiency carrier tiers.
#[derive(Debug)]
pub struct TieredGreenPool {
    performance_carrier: ThreadPool,
    performance_green: GreenPool,
    efficiency_carrier: ThreadPool,
    efficiency_green: GreenPool,
    auto_policy: AutoCarrierPolicy,
    support: CarrierTierSupport,
}

impl TieredGreenPool {
    /// Reports the current backend's carrier-tier support surface.
    #[must_use]
    pub fn support() -> CarrierTierSupport {
        let thread = ThreadPool::support();
        let summary = system_cpu().topology_summary().ok();
        let asymmetric_core_classes = summary
            .and_then(|summary| summary.core_class_count)
            .is_some_and(|count| count > 1);
        let tier_partitioning = if asymmetric_core_classes {
            thread.placement.core_class_affinity
        } else {
            ThreadGuarantee::Unsupported
        };
        CarrierTierSupport {
            asymmetric_core_classes,
            tier_partitioning,
            core_class_affinity: thread.placement.core_class_affinity,
            logical_cpu_affinity: thread.placement.logical_cpu_affinity,
        }
    }

    /// Builds one dual-tier green-thread scheduler.
    ///
    /// # Errors
    ///
    /// Returns any honest carrier-pool or green-pool construction failure.
    pub fn new(config: &TieredGreenPoolConfig<'_>) -> Result<Self, TieredGreenPoolError> {
        let performance_carrier = ThreadPool::new(&config.performance_carrier)?;
        let performance_green =
            match GreenPool::new(&config.performance_green, &performance_carrier) {
                Ok(pool) => pool,
                Err(error) => {
                    let _ = performance_carrier.shutdown();
                    return Err(error.into());
                }
            };

        let efficiency_carrier = match ThreadPool::new(&config.efficiency_carrier) {
            Ok(pool) => pool,
            Err(error) => {
                let _ = performance_green.shutdown();
                let _ = performance_carrier.shutdown();
                return Err(error.into());
            }
        };
        let efficiency_green = match GreenPool::new(&config.efficiency_green, &efficiency_carrier) {
            Ok(pool) => pool,
            Err(error) => {
                let _ = efficiency_carrier.shutdown();
                let _ = performance_green.shutdown();
                let _ = performance_carrier.shutdown();
                return Err(error.into());
            }
        };

        Ok(Self {
            performance_carrier,
            performance_green,
            efficiency_carrier,
            efficiency_green,
            auto_policy: config.auto_policy,
            support: Self::support(),
        })
    }

    /// Attempts to clone one additional tiered scheduler handle.
    ///
    /// # Errors
    ///
    /// Returns an error when any shared carrier or green pool cannot be retained honestly.
    pub fn try_clone(&self) -> Result<Self, TieredGreenPoolError> {
        Ok(Self {
            performance_carrier: self.performance_carrier.try_clone()?,
            performance_green: self.performance_green.try_clone()?,
            efficiency_carrier: self.efficiency_carrier.try_clone()?,
            efficiency_green: self.efficiency_green.try_clone()?,
            auto_policy: self.auto_policy,
            support: self.support,
        })
    }

    /// Returns the static carrier-tier support snapshot captured at build time.
    #[must_use]
    pub const fn tier_support(&self) -> CarrierTierSupport {
        self.support
    }

    /// Returns the configured auto-placement policy.
    #[must_use]
    pub const fn auto_policy(&self) -> AutoCarrierPolicy {
        self.auto_policy
    }

    /// Returns the performance-tier green pool.
    #[must_use]
    pub const fn performance_pool(&self) -> &GreenPool {
        &self.performance_green
    }

    /// Returns the efficiency-tier green pool.
    #[must_use]
    pub const fn efficiency_pool(&self) -> &GreenPool {
        &self.efficiency_green
    }

    /// Resolves one tiered task bundle into the effective carrier tier.
    #[must_use]
    pub const fn resolve_tier(&self, task: TieredTaskAttributes) -> CarrierTier {
        self.resolve_tier_for_priority(task.placement, task.fiber.priority)
    }

    /// Resolves one placement request against the configured automatic-priority heuristic.
    #[must_use]
    pub const fn resolve_tier_for_priority(
        &self,
        placement: TaskPlacement,
        priority: FiberTaskPriority,
    ) -> CarrierTier {
        match placement {
            TaskPlacement::Tier(tier) => tier,
            TaskPlacement::Auto => {
                if priority.get() <= self.auto_policy.efficiency_priority_ceiling.get() {
                    CarrierTier::Efficiency
                } else {
                    CarrierTier::Performance
                }
            }
        }
    }

    const fn green_pool_for_tier(&self, tier: CarrierTier) -> &GreenPool {
        match tier {
            CarrierTier::Performance => &self.performance_green,
            CarrierTier::Efficiency => &self.efficiency_green,
        }
    }

    const fn vector_lane_for_tier(tier: CarrierTier) -> VectorDispatchLane {
        match tier {
            CarrierTier::Performance => VectorDispatchLane::DeferredPrimary,
            CarrierTier::Efficiency => VectorDispatchLane::DeferredSecondary,
        }
    }

    /// Returns one live scheduler snapshot.
    ///
    /// # Errors
    ///
    /// Returns any honest carrier-pool observation failure.
    pub fn stats(&self) -> Result<TieredGreenPoolStats, TieredGreenPoolError> {
        let performance = self.performance_carrier.stats()?;
        let efficiency = self.efficiency_carrier.stats()?;
        Ok(TieredGreenPoolStats {
            performance_workers: performance.active_workers,
            efficiency_workers: efficiency.active_workers,
            performance_queued: performance.queued_items,
            efficiency_queued: efficiency.queued_items,
            performance_green_threads: self.performance_green.active_count(),
            efficiency_green_threads: self.efficiency_green.active_count(),
        })
    }

    /// Spawns one green job using the default automatic tier policy.
    ///
    /// Default-priority work routes to the performance tier.
    ///
    /// # Errors
    ///
    /// Returns any honest lower-tier spawn failure.
    pub fn spawn<F, T>(&self, job: F) -> Result<GreenHandle<T>, TieredGreenPoolError>
    where
        F: FnOnce() -> T + Send + 'static,
        T: 'static,
    {
        self.performance_green.spawn(job).map_err(Into::into)
    }

    /// Spawns one green job onto the explicitly requested carrier tier.
    ///
    /// # Errors
    ///
    /// Returns any honest lower-tier spawn failure.
    pub fn spawn_on<F, T>(
        &self,
        placement: TaskPlacement,
        job: F,
    ) -> Result<GreenHandle<T>, TieredGreenPoolError>
    where
        F: FnOnce() -> T + Send + 'static,
        T: 'static,
    {
        let tier = self.resolve_tier_for_priority(placement, FiberTaskPriority::DEFAULT);
        self.green_pool_for_tier(tier)
            .spawn(job)
            .map_err(Into::into)
    }

    /// Spawns one green job with explicit stack-class, priority, and placement metadata.
    ///
    /// # Errors
    ///
    /// Returns any honest lower-tier spawn failure.
    pub fn spawn_with_task<F, T>(
        &self,
        task: TieredTaskAttributes,
        job: F,
    ) -> Result<GreenHandle<T>, TieredGreenPoolError>
    where
        F: FnOnce() -> T + Send + 'static,
        T: 'static,
    {
        let tier = self.resolve_tier(task);
        self.green_pool_for_tier(tier)
            .spawn_with_attrs(task.fiber, job)
            .map_err(Into::into)
    }

    /// Spawns one explicit stack-budgeted fiber task using automatic tier routing.
    ///
    /// # Errors
    ///
    /// Returns any honest lower-tier spawn failure.
    pub fn spawn_explicit<T>(&self, task: T) -> Result<GreenHandle<T::Output>, TieredGreenPoolError>
    where
        T: ExplicitFiberTask,
    {
        let attributes = TieredTaskAttributes::new(T::task_attributes()?);
        let tier = self.resolve_tier(attributes);
        self.green_pool_for_tier(tier)
            .spawn_explicit(task)
            .map_err(Into::into)
    }

    /// Spawns one build-generated explicit task using automatic tier routing.
    ///
    /// # Errors
    ///
    /// Returns any honest lower-tier spawn failure.
    #[cfg(not(feature = "critical-safe"))]
    pub fn spawn_generated<T>(
        &self,
        task: T,
    ) -> Result<GreenHandle<T::Output>, TieredGreenPoolError>
    where
        T: GeneratedExplicitFiberTask,
    {
        let attributes = TieredTaskAttributes::new(T::task_attributes()?);
        let tier = self.resolve_tier(attributes);
        self.green_pool_for_tier(tier)
            .spawn_generated(task)
            .map_err(Into::into)
    }

    /// Spawns one build-generated task using its compile-time generated contract directly.
    ///
    /// This is the cross-crate contract-first path: callers do not need to override
    /// `GeneratedExplicitFiberTask::task_attributes()` just to avoid runtime metadata lookup.
    ///
    /// # Errors
    ///
    /// Returns any honest lower-tier spawn failure.
    pub fn spawn_generated_contract<T>(
        &self,
        task: T,
    ) -> Result<GreenHandle<T::Output>, TieredGreenPoolError>
    where
        T: GeneratedExplicitFiberTask + GeneratedExplicitFiberTaskContract,
    {
        let attributes =
            TieredTaskAttributes::new(generated_explicit_task_contract_attributes::<T>());
        let tier = self.resolve_tier(attributes);
        self.green_pool_for_tier(tier)
            .spawn_generated_contract(task)
            .map_err(Into::into)
    }

    /// Binds one vector slot to one deferred green/blue runtime task target.
    ///
    /// The lane is derived from the resolved carrier tier:
    /// performance-tier work binds to `DeferredPrimary`, efficiency-tier work binds to
    /// `DeferredSecondary`.
    ///
    /// # Errors
    ///
    /// Returns any honest vector-binding, registry, or lower-tier scheduling failure.
    pub fn bind_vector_task(
        &self,
        builder: &mut VectorTableBuilder,
        slot: IrqSlot,
        priority: Option<VectorPriority>,
        task: TieredTaskAttributes,
        job: fn(),
    ) -> Result<VectorDispatchCookie, TieredGreenPoolError> {
        let cookie = builder.register_deferred_callback(tiered_vector_dispatch_callback)?;
        self.bind_vector_task_registered_cookie(builder, slot, priority, cookie, task, job)?;
        Ok(cookie)
    }

    /// Binds one vector slot to one deferred green/blue runtime task target using one explicit
    /// preselected deferred cookie.
    ///
    /// This is the static-contract path for generated red fallback cookies and other cases where
    /// the callback identity must be chosen before runtime monotonic allocation gets a vote.
    ///
    /// # Errors
    ///
    /// Returns any honest vector-binding, registry, or lower-tier scheduling failure.
    pub fn bind_vector_task_with_cookie(
        &self,
        builder: &mut VectorTableBuilder,
        slot: IrqSlot,
        priority: Option<VectorPriority>,
        cookie: VectorDispatchCookie,
        task: TieredTaskAttributes,
        job: fn(),
    ) -> Result<(), TieredGreenPoolError> {
        if let Err(error) =
            builder.register_deferred_callback_with_cookie(cookie, tiered_vector_dispatch_callback)
        {
            return Err(error.into());
        }
        self.bind_vector_task_registered_cookie(builder, slot, priority, cookie, task, job)
    }

    fn bind_vector_task_registered_cookie(
        &self,
        builder: &mut VectorTableBuilder,
        slot: IrqSlot,
        priority: Option<VectorPriority>,
        cookie: VectorDispatchCookie,
        task: TieredTaskAttributes,
        job: fn(),
    ) -> Result<(), TieredGreenPoolError> {
        builder.bind_reserved_pendsv_dispatch(Some(VectorPriority(u8::MAX)))?;
        let tier = self.resolve_tier(task);
        let lane = Self::vector_lane_for_tier(tier);
        if let Err(error) =
            register_tiered_vector_target(cookie, TieredVectorDispatchTarget { task, job })
        {
            let _ = builder.unregister_deferred_callback(cookie);
            clear_tiered_vector_target(cookie);
            return Err(error);
        }
        if let Err(error) = builder.bind_deferred(slot, lane, priority, cookie) {
            let _ = builder.unregister_deferred_callback(cookie);
            clear_tiered_vector_target(cookie);
            return Err(error.into());
        }
        Ok(())
    }

    /// Unbinds one previously registered tiered vector task target and releases its callback slot.
    ///
    /// # Errors
    ///
    /// Returns any honest vector unbind or registry-release failure.
    pub fn unbind_vector_task(
        &self,
        builder: &mut VectorTableBuilder,
        slot: IrqSlot,
        cookie: VectorDispatchCookie,
    ) -> Result<(), TieredGreenPoolError> {
        builder.unbind(slot)?;
        if let Err(error) = builder.unregister_deferred_callback(cookie) {
            clear_tiered_vector_target(cookie);
            return Err(error.into());
        }
        clear_tiered_vector_target(cookie);
        Ok(())
    }

    /// Drains deferred vector callbacks already surfaced through the sealed table and routes the
    /// resulting work into the green/blue carrier tiers.
    ///
    /// # Errors
    ///
    /// Returns any honest vector observation or lower-tier spawn failure.
    pub fn dispatch_vector_pending(
        &self,
        table: &SealedVectorTable,
    ) -> Result<usize, TieredGreenPoolError> {
        let _ = table.dispatch_pending_primary()?;
        let _ = table.dispatch_pending_secondary()?;
        self.drain_vector_dispatch()
    }

    /// Drains the primary deferred vector lane and routes the resulting work into the green tier.
    ///
    /// # Errors
    ///
    /// Returns any honest vector observation or lower-tier spawn failure.
    pub fn dispatch_vector_primary(
        &self,
        table: &SealedVectorTable,
    ) -> Result<usize, TieredGreenPoolError> {
        let _ = table.dispatch_pending_primary()?;
        self.drain_vector_dispatch()
    }

    /// Drains the secondary deferred vector lane and routes the resulting work into the blue tier.
    ///
    /// # Errors
    ///
    /// Returns any honest vector observation or lower-tier spawn failure.
    pub fn dispatch_vector_secondary(
        &self,
        table: &SealedVectorTable,
    ) -> Result<usize, TieredGreenPoolError> {
        let _ = table.dispatch_pending_secondary()?;
        self.drain_vector_dispatch()
    }

    /// Drains the runtime's tiered vector-dispatch staging counters into the green/blue carrier
    /// pools.
    ///
    /// # Errors
    ///
    /// Returns any honest lower-tier spawn failure.
    pub fn drain_vector_dispatch(&self) -> Result<usize, TieredGreenPoolError> {
        let targets = *TIERED_VECTOR_TARGETS.lock()?;
        let mut dispatched = 0_usize;

        for (index, target) in targets.into_iter().enumerate() {
            let mut remaining = TIERED_VECTOR_DISPATCH_COUNTS[index].swap(0, Ordering::AcqRel);
            let Some(target) = target else {
                continue;
            };
            while remaining != 0 {
                let tier = self.resolve_tier(target.task);
                if let Err(error) = self
                    .green_pool_for_tier(tier)
                    .spawn_with_attrs(target.task.fiber, target.job)
                {
                    restore_tiered_vector_dispatch_count(index, remaining);
                    return Err(error.into());
                }
                remaining -= 1;
                dispatched += 1;
            }
        }

        Ok(dispatched)
    }

    /// Spawns one build-generated explicit task using automatic tier routing.
    ///
    /// In strict generated-contract builds, routing comes from the compile-time generated
    /// contract attributes instead of runtime metadata lookup.
    ///
    /// # Errors
    ///
    /// Returns any honest lower-tier spawn failure.
    #[cfg(feature = "critical-safe")]
    pub fn spawn_generated<T>(
        &self,
        task: T,
    ) -> Result<GreenHandle<T::Output>, TieredGreenPoolError>
    where
        T: GeneratedExplicitFiberTask + GeneratedExplicitFiberTaskContract,
    {
        let attributes =
            TieredTaskAttributes::new(generated_explicit_task_contract_attributes::<T>());
        let tier = self.resolve_tier(attributes);
        self.green_pool_for_tier(tier)
            .spawn_generated(task)
            .map_err(Into::into)
    }

    /// Requests shutdown across both carrier tiers.
    ///
    /// # Errors
    ///
    /// Returns the first honest failure encountered while draining the tiered schedulers, after
    /// attempting teardown across both tiers.
    pub fn shutdown(&self) -> Result<(), TieredGreenPoolError> {
        let mut first_error = None;

        if let Err(error) = self.performance_green.shutdown() {
            first_error = Some(error.into());
        }
        let efficiency_green_shutdown = self.efficiency_green.shutdown();
        if let Err(error) = efficiency_green_shutdown
            && first_error.is_none()
        {
            first_error = Some(error.into());
        }
        match self.performance_carrier.try_clone() {
            Ok(carrier) => {
                let carrier_shutdown = carrier.shutdown();
                if let Err(error) = carrier_shutdown
                    && first_error.is_none()
                {
                    first_error = Some(error.into());
                }
            }
            Err(error) => {
                if first_error.is_none() {
                    first_error = Some(error.into());
                }
            }
        }
        match self.efficiency_carrier.try_clone() {
            Ok(carrier) => {
                let carrier_shutdown = carrier.shutdown();
                if let Err(error) = carrier_shutdown
                    && first_error.is_none()
                {
                    first_error = Some(error.into());
                }
            }
            Err(error) => {
                if first_error.is_none() {
                    first_error = Some(error.into());
                }
            }
        }

        if let Some(error) = first_error {
            return Err(error);
        }
        Ok(())
    }
}

fn tiered_vector_cookie_index(cookie: VectorDispatchCookie) -> Option<usize> {
    cookie
        .0
        .checked_sub(1)
        .and_then(|index| usize::try_from(index).ok())
        .filter(|index| *index < TIERED_VECTOR_TARGET_CAPACITY)
}

fn register_tiered_vector_target(
    cookie: VectorDispatchCookie,
    target: TieredVectorDispatchTarget,
) -> Result<(), TieredGreenPoolError> {
    let Some(index) = tiered_vector_cookie_index(cookie) else {
        return Err(TieredGreenPoolError::unsupported());
    };
    let mut targets = TIERED_VECTOR_TARGETS.lock()?;
    if targets[index].is_some() {
        return Err(TieredGreenPoolError::unsupported());
    }
    targets[index] = Some(target);
    Ok(())
}

fn clear_tiered_vector_target(cookie: VectorDispatchCookie) {
    let Some(index) = tiered_vector_cookie_index(cookie) else {
        return;
    };
    if let Ok(mut targets) = TIERED_VECTOR_TARGETS.lock() {
        targets[index] = None;
    }
    TIERED_VECTOR_DISPATCH_COUNTS[index].store(0, Ordering::Release);
}

fn restore_tiered_vector_dispatch_count(index: usize, count: u32) {
    let counter = &TIERED_VECTOR_DISPATCH_COUNTS[index];
    let _ = counter.fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
        Some(current.saturating_add(count))
    });
}

fn tiered_vector_dispatch_callback(cookie: VectorDispatchCookie) {
    let Some(index) = tiered_vector_cookie_index(cookie) else {
        return;
    };
    let counter = &TIERED_VECTOR_DISPATCH_COUNTS[index];
    let _ = counter.fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
        Some(current.saturating_add(1))
    });
}

#[cfg(all(test, feature = "std", not(target_os = "none")))]
mod tests {
    extern crate std;

    use core::sync::atomic::{AtomicU32, Ordering};

    use super::*;
    use crate::thread::FiberStackClass;

    static PERFORMANCE_RUNS: AtomicU32 = AtomicU32::new(0);
    static EFFICIENCY_RUNS: AtomicU32 = AtomicU32::new(0);

    fn performance_job() {
        PERFORMANCE_RUNS.fetch_add(1, Ordering::AcqRel);
    }

    fn efficiency_job() {
        EFFICIENCY_RUNS.fetch_add(1, Ordering::AcqRel);
    }

    fn tiered_pool_is_unsupported(error: TieredGreenPoolError) -> bool {
        matches!(error, TieredGreenPoolError::Unsupported)
            || matches!(
                error,
                TieredGreenPoolError::ThreadPool(thread_error)
                    if thread_error.kind() == fusion_sys::thread::ThreadErrorKind::Unsupported
            )
            || matches!(
                error,
                TieredGreenPoolError::Green(fiber_error)
                    if fiber_error.kind() == fusion_sys::fiber::FiberErrorKind::Unsupported
            )
    }

    #[test]
    fn vector_lane_mapping_tracks_resolved_tier() {
        let pool = match TieredGreenPool::new(&TieredGreenPoolConfig::new()) {
            Ok(pool) => pool,
            Err(error) if tiered_pool_is_unsupported(error) => return,
            Err(error) => panic!("tiered pool should build: {error:?}"),
        };

        let performance = TieredTaskAttributes::new(
            FiberTaskAttributes::new(FiberStackClass::MIN)
                .with_priority(FiberTaskPriority::DEFAULT),
        );
        let efficiency = TieredTaskAttributes::new(
            FiberTaskAttributes::new(FiberStackClass::MIN)
                .with_priority(FiberTaskPriority::new(-1)),
        );

        assert_eq!(
            TieredGreenPool::vector_lane_for_tier(pool.resolve_tier(performance)),
            VectorDispatchLane::DeferredPrimary
        );
        assert_eq!(
            TieredGreenPool::vector_lane_for_tier(pool.resolve_tier(efficiency)),
            VectorDispatchLane::DeferredSecondary
        );

        pool.shutdown().expect("tiered pool should shut down");
    }

    #[test]
    fn draining_vector_dispatch_routes_registered_targets() {
        PERFORMANCE_RUNS.store(0, Ordering::Release);
        EFFICIENCY_RUNS.store(0, Ordering::Release);

        let pool = match TieredGreenPool::new(&TieredGreenPoolConfig::new()) {
            Ok(pool) => pool,
            Err(error) if tiered_pool_is_unsupported(error) => return,
            Err(error) => panic!("tiered pool should build: {error:?}"),
        };

        let performance = TieredTaskAttributes::new(
            FiberTaskAttributes::new(FiberStackClass::MIN)
                .with_priority(FiberTaskPriority::DEFAULT),
        );
        let efficiency = TieredTaskAttributes::new(
            FiberTaskAttributes::new(FiberStackClass::MIN)
                .with_priority(FiberTaskPriority::new(-1)),
        );

        let performance_cookie = VectorDispatchCookie(1);
        let efficiency_cookie = VectorDispatchCookie(2);
        register_tiered_vector_target(
            performance_cookie,
            TieredVectorDispatchTarget {
                task: performance,
                job: performance_job,
            },
        )
        .expect("performance target should register");
        register_tiered_vector_target(
            efficiency_cookie,
            TieredVectorDispatchTarget {
                task: efficiency,
                job: efficiency_job,
            },
        )
        .expect("efficiency target should register");

        tiered_vector_dispatch_callback(performance_cookie);
        tiered_vector_dispatch_callback(performance_cookie);
        tiered_vector_dispatch_callback(efficiency_cookie);

        let drained = pool
            .drain_vector_dispatch()
            .expect("draining vector dispatch should succeed");
        assert_eq!(drained, 3);

        pool.shutdown().expect("tiered pool should shut down");

        assert_eq!(PERFORMANCE_RUNS.load(Ordering::Acquire), 2);
        assert_eq!(EFFICIENCY_RUNS.load(Ordering::Acquire), 1);

        clear_tiered_vector_target(performance_cookie);
        clear_tiered_vector_target(efficiency_cookie);
    }

    #[test]
    fn draining_vector_dispatch_handles_burst_callbacks() {
        PERFORMANCE_RUNS.store(0, Ordering::Release);
        EFFICIENCY_RUNS.store(0, Ordering::Release);

        let pool = match TieredGreenPool::new(&TieredGreenPoolConfig::new()) {
            Ok(pool) => pool,
            Err(error) if tiered_pool_is_unsupported(error) => return,
            Err(error) => panic!("tiered pool should build: {error:?}"),
        };

        let performance = TieredTaskAttributes::new(
            FiberTaskAttributes::new(FiberStackClass::MIN)
                .with_priority(FiberTaskPriority::DEFAULT),
        );
        let efficiency = TieredTaskAttributes::new(
            FiberTaskAttributes::new(FiberStackClass::MIN)
                .with_priority(FiberTaskPriority::new(-1)),
        );

        let performance_cookie = VectorDispatchCookie(3);
        let efficiency_cookie = VectorDispatchCookie(4);
        register_tiered_vector_target(
            performance_cookie,
            TieredVectorDispatchTarget {
                task: performance,
                job: performance_job,
            },
        )
        .expect("performance target should register");
        register_tiered_vector_target(
            efficiency_cookie,
            TieredVectorDispatchTarget {
                task: efficiency,
                job: efficiency_job,
            },
        )
        .expect("efficiency target should register");

        for _ in 0..64 {
            tiered_vector_dispatch_callback(performance_cookie);
        }
        for _ in 0..33 {
            tiered_vector_dispatch_callback(efficiency_cookie);
        }

        let drained = pool
            .drain_vector_dispatch()
            .expect("burst vector dispatch should succeed");
        assert_eq!(drained, 97);

        pool.shutdown().expect("tiered pool should shut down");

        assert_eq!(PERFORMANCE_RUNS.load(Ordering::Acquire), 64);
        assert_eq!(EFFICIENCY_RUNS.load(Ordering::Acquire), 33);

        clear_tiered_vector_target(performance_cookie);
        clear_tiered_vector_target(efficiency_cookie);
    }

    #[test]
    fn clear_tiered_vector_target_removes_registered_target_and_pending_count() {
        let performance = TieredTaskAttributes::new(
            FiberTaskAttributes::new(FiberStackClass::MIN)
                .with_priority(FiberTaskPriority::DEFAULT),
        );
        let cookie = VectorDispatchCookie(5);
        register_tiered_vector_target(
            cookie,
            TieredVectorDispatchTarget {
                task: performance,
                job: performance_job,
            },
        )
        .expect("target should register");
        tiered_vector_dispatch_callback(cookie);
        tiered_vector_dispatch_callback(cookie);

        clear_tiered_vector_target(cookie);

        let index = tiered_vector_cookie_index(cookie).expect("cookie should map into registry");
        let targets = TIERED_VECTOR_TARGETS
            .lock()
            .expect("registry lock should succeed");
        assert!(targets[index].is_none());
        drop(targets);
        assert_eq!(
            TIERED_VECTOR_DISPATCH_COUNTS[index].load(Ordering::Acquire),
            0
        );
    }
}
