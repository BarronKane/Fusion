//! fusion-sys courier contracts plus claim-aware mediation helpers.
//!
//! TODO: When `fusion-kernel` grows its real root-courier authority surface, teach the kernel
//! courier path to observe and use architecture privilege/state bits such as CS / mPRIV /
//! `CurrentEL` directly. That work belongs here at the courier authority boundary, because the
//! kernel courier will want to reason about current execution privilege honestly instead of
//! treating it as vague ambient kernel magic.
//! Intended ring model once that work lands:
//! - Ring 0: kernel core
//! - Ring 1: drivers
//! - Ring 2: system resources and operating services
//! - Ring 3: user/context/application space

#[path = "local.rs"]
pub mod local;

use core::hash::{
    Hash,
    Hasher,
};

pub use fusion_pal::sys::courier::*;

use crate::claims::{
    ClaimAwareness,
    ClaimContextId,
    ClaimsError,
    LocalAdmissionSeal,
    PrincipalId,
};
use crate::domain::context::ContextId as RuntimeContextId;
use crate::fiber::{
    FiberErrorKind,
    FiberId,
    FiberReturn,
    FiberState,
    ManagedFiberSnapshot,
};
pub use crate::fiber::{
    current_context_id,
    current_courier_id,
    current_fiber_id,
};

/// Scope role carried by one courier within the visible context tree.
///
/// Every courier is still just a courier. The role only determines whether it establishes a new
/// visible context-root boundary for descendant naming and scoping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CourierScopeRole {
    /// Ordinary courier that does not establish a new visible context root.
    #[default]
    Leaf,
    /// Courier that becomes a visible context-root for descendant naming and scoping.
    ContextRoot,
}

impl CourierScopeRole {
    #[must_use]
    pub const fn is_context_root(self) -> bool {
        matches!(self, Self::ContextRoot)
    }
}

/// Snapshot of one courier's public identity and support surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CourierMetadata<'a> {
    pub id: CourierId,
    pub name: &'a str,
    pub scope_role: CourierScopeRole,
    pub support: CourierSupport,
}

impl CourierMetadata<'_> {
    #[must_use]
    pub const fn domain_id(self) -> crate::domain::DomainId {
        self.support.domain_id()
    }

    #[must_use]
    pub const fn visibility(self) -> CourierVisibility {
        self.support.visibility()
    }

    #[must_use]
    pub const fn claim_metadata(self) -> CourierClaimMetadata {
        CourierClaimMetadata {
            awareness: self.support.claim_awareness(),
            context: self.support.claim_context(),
        }
    }

    #[must_use]
    pub const fn is_context_root(self) -> bool {
        self.scope_role.is_context_root()
    }
}

/// Claim-facing snapshot of one courier's current mediation state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CourierClaimMetadata {
    pub awareness: ClaimAwareness,
    pub context: Option<ClaimContextId>,
}

impl CourierClaimMetadata {
    #[must_use]
    pub const fn is_enabled(self) -> bool {
        self.awareness.is_black() && self.context.is_some()
    }
}

/// Coarse responsiveness classification for child couriers and fibers supervised by a courier.
///
/// This is demand-driven state, not ambient heartbeat theology. A running courier/fiber with no
/// externally visible service obligations may remain `Responsive` indefinitely without emitting any
/// periodic liveness pulse. Escalation to `Stale` or `NonResponsive` should happen only when one
/// required observable interaction (for example, one courier/channel acknowledgment path) stops
/// making progress within policy bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CourierResponsiveness {
    #[default]
    Responsive,
    Stale,
    NonResponsive,
}

impl CourierResponsiveness {
    #[must_use]
    pub const fn is_responsive(self) -> bool {
        matches!(self, Self::Responsive)
    }
}

/// Scheduler policy exposed by one courier-facing runtime lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CourierSchedulingPolicy {
    /// Cooperative FIFO-style execution with no strict priority ordering.
    CooperativeRoundRobin,
    /// Cooperative execution with explicit priority-aware ordering.
    #[default]
    CooperativePriority,
    /// Cooperative queues distributed across carriers with work stealing.
    CooperativeWorkStealing,
    /// Outer-layer time slicing is active for the owning courier.
    TimeSliced { quantum_ticks: u64 },
}

impl CourierSchedulingPolicy {
    #[must_use]
    pub const fn time_slice_ticks(self) -> Option<u64> {
        match self {
            Self::TimeSliced { quantum_ticks } => Some(quantum_ticks),
            Self::CooperativeRoundRobin
            | Self::CooperativePriority
            | Self::CooperativeWorkStealing => None,
        }
    }
}

/// Kind of runnable work surfaced by one courier-local scheduler.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RunnableUnitKind {
    Fiber,
    AsyncTask,
    Control,
}

/// Coarse run-state exposed by one courier-facing runtime surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CourierRunState {
    #[default]
    Idle,
    Runnable,
    Running,
    Stale,
    NonResponsive,
}

impl CourierRunState {
    #[must_use]
    pub const fn is_active(self) -> bool {
        matches!(self, Self::Runnable | Self::Running)
    }
}

/// One courier-local runnable lane summary such as fibers or async tasks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CourierLaneSummary {
    pub kind: RunnableUnitKind,
    pub active_units: usize,
    pub runnable_units: usize,
    pub running_units: usize,
    pub blocked_units: usize,
    pub available_slots: usize,
}

impl CourierLaneSummary {
    #[must_use]
    pub const fn new(kind: RunnableUnitKind) -> Self {
        Self {
            kind,
            active_units: 0,
            runnable_units: 0,
            running_units: 0,
            blocked_units: 0,
            available_slots: 0,
        }
    }

    #[must_use]
    pub const fn total_known_units(self) -> usize {
        self.active_units
    }
}

/// Aggregate courier-facing runtime summary for one supervised execution bundle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct CourierRuntimeSummary {
    pub policy: CourierSchedulingPolicy,
    pub run_state: CourierRunState,
    pub responsiveness: CourierResponsiveness,
    pub fiber_lane: Option<CourierLaneSummary>,
    pub async_lane: Option<CourierLaneSummary>,
    pub control_lane: Option<CourierLaneSummary>,
}

/// Static admission class for one fiber under one courier runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CourierFiberClass {
    Planned,
    Dynamic,
}

/// One typed current-context record owned by a courier runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CourierRuntimeContextRecord {
    pub context: RuntimeContextId,
    pub updated_tick: u64,
}

/// One typed runtime-summary snapshot cached by the courier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CourierRuntimeSummaryRecord {
    pub summary: CourierRuntimeSummary,
    pub updated_tick: u64,
}

/// Typed courier-owned runtime ledger.
///
/// This is the fixed substrate record lane. App-defined metadata still belongs in the overlay
/// store below, but runtime truth such as current context, lane summary, and admission counts
/// lives here so it stops competing with stringly application notes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct CourierRuntimeLedger {
    pub current_context: Option<CourierRuntimeContextRecord>,
    pub summary: Option<CourierRuntimeSummaryRecord>,
    pub active_planned_fibers: usize,
    pub active_dynamic_fibers: usize,
    pub active_async_tasks: usize,
    pub active_runnable_units: usize,
}

impl CourierRuntimeLedger {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            current_context: None,
            summary: None,
            active_planned_fibers: 0,
            active_dynamic_fibers: 0,
            active_async_tasks: 0,
            active_runnable_units: 0,
        }
    }

    pub fn record_context(&mut self, context: RuntimeContextId, updated_tick: u64) {
        self.current_context = Some(CourierRuntimeContextRecord {
            context,
            updated_tick,
        });
    }

    pub fn register_fiber(&mut self, class: CourierFiberClass) {
        match class {
            CourierFiberClass::Planned => {
                self.active_planned_fibers = self.active_planned_fibers.saturating_add(1);
            }
            CourierFiberClass::Dynamic => {
                self.active_dynamic_fibers = self.active_dynamic_fibers.saturating_add(1);
            }
        }
        self.recompute_runnable_units();
    }

    pub fn release_fiber(&mut self, class: CourierFiberClass) {
        match class {
            CourierFiberClass::Planned => {
                self.active_planned_fibers = self.active_planned_fibers.saturating_sub(1);
            }
            CourierFiberClass::Dynamic => {
                self.active_dynamic_fibers = self.active_dynamic_fibers.saturating_sub(1);
            }
        }
        self.recompute_runnable_units();
    }

    pub fn record_async_lane(
        &mut self,
        lane: CourierLaneSummary,
        updated_tick: u64,
        policy: CourierSchedulingPolicy,
        responsiveness: CourierResponsiveness,
    ) {
        self.active_async_tasks = lane.active_units;
        self.summary = Some(CourierRuntimeSummaryRecord {
            summary: CourierRuntimeSummary::new(policy, responsiveness).with_async_lane(lane),
            updated_tick,
        });
        self.recompute_runnable_units();
    }

    pub fn record_summary(&mut self, summary: CourierRuntimeSummary, updated_tick: u64) {
        self.active_async_tasks = summary.async_lane.map_or(0, |lane| lane.active_units);
        self.summary = Some(CourierRuntimeSummaryRecord {
            summary,
            updated_tick,
        });
        self.recompute_runnable_units();
    }

    fn recompute_runnable_units(&mut self) {
        self.active_runnable_units = self
            .active_planned_fibers
            .saturating_add(self.active_dynamic_fibers)
            .saturating_add(self.active_async_tasks);
    }
}

/// Coarse sink error surfaced when one runtime publishes into courier-owned truth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CourierRuntimeSinkError {
    Unsupported,
    Invalid,
    NotFound,
    ResourceExhausted,
    StateConflict,
    Busy,
}

impl From<crate::domain::DomainError> for CourierRuntimeSinkError {
    fn from(value: crate::domain::DomainError) -> Self {
        use crate::domain::DomainErrorKind;
        match value.kind() {
            DomainErrorKind::Unsupported => Self::Unsupported,
            DomainErrorKind::Invalid => Self::Invalid,
            DomainErrorKind::NotFound => Self::NotFound,
            DomainErrorKind::NotVisible => Self::Busy,
            DomainErrorKind::ResourceExhausted => Self::ResourceExhausted,
            DomainErrorKind::StateConflict => Self::StateConflict,
            DomainErrorKind::Busy
            | DomainErrorKind::PermissionDenied
            | DomainErrorKind::Platform(_) => Self::Busy,
        }
    }
}

/// One generic runtime-to-courier sink surface.
///
/// This keeps runtime wiring composable: the runtime publishes courier truth through this narrow
/// surface, while concrete consumers such as `DomainRegistry` or `fusion-kernel` decide where that
/// truth is stored and how it is synchronized.
#[derive(Debug, Clone, Copy)]
pub struct CourierRuntimeSink {
    context: *mut (),
    vtable: &'static CourierRuntimeSinkVTable,
}

// SAFETY: the sink itself is just an opaque pointer plus immutable function table. Concrete
// providers are responsible for supplying backing state that is actually synchronized correctly for
// whatever runtime they hand this to.
unsafe impl Send for CourierRuntimeSink {}
// SAFETY: same reasoning as above; synchronization correctness belongs to the sink provider.
unsafe impl Sync for CourierRuntimeSink {}

impl PartialEq for CourierRuntimeSink {
    fn eq(&self, other: &Self) -> bool {
        self.context == other.context
            && core::ptr::eq(self.vtable as *const _, other.vtable as *const _)
    }
}

impl Eq for CourierRuntimeSink {}

impl Hash for CourierRuntimeSink {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.context.hash(state);
        (self.vtable as *const CourierRuntimeSinkVTable).hash(state);
    }
}

impl CourierRuntimeSink {
    #[must_use]
    pub const fn new(context: *mut (), vtable: &'static CourierRuntimeSinkVTable) -> Self {
        Self { context, vtable }
    }

    pub fn record_context(
        self,
        courier: CourierId,
        context: RuntimeContextId,
        tick: u64,
    ) -> Result<(), CourierRuntimeSinkError> {
        // SAFETY: the sink provider guarantees the pointed backing outlives the runtime using it.
        unsafe { (self.vtable.record_context)(self.context, courier, context, tick) }
    }

    pub fn register_fiber(
        self,
        courier: CourierId,
        snapshot: ManagedFiberSnapshot,
        generation: u64,
        class: CourierFiberClass,
        is_root: bool,
        metadata_attachment: Option<FiberMetadataAttachment>,
        tick: u64,
    ) -> Result<(), CourierRuntimeSinkError> {
        // SAFETY: the sink provider guarantees the pointed backing outlives the runtime using it.
        unsafe {
            (self.vtable.register_fiber)(
                self.context,
                courier,
                snapshot,
                generation,
                class,
                is_root,
                metadata_attachment,
                tick,
            )
        }
    }

    pub fn update_fiber(
        self,
        courier: CourierId,
        snapshot: ManagedFiberSnapshot,
        tick: u64,
    ) -> Result<(), CourierRuntimeSinkError> {
        // SAFETY: the sink provider guarantees the pointed backing outlives the runtime using it.
        unsafe { (self.vtable.update_fiber)(self.context, courier, snapshot, tick) }
    }

    pub fn mark_fiber_terminal(
        self,
        courier: CourierId,
        fiber: FiberId,
        terminal: FiberTerminalStatus,
        tick: u64,
    ) -> Result<(), CourierRuntimeSinkError> {
        // SAFETY: the sink provider guarantees the pointed backing outlives the runtime using it.
        unsafe { (self.vtable.mark_fiber_terminal)(self.context, courier, fiber, terminal, tick) }
    }

    pub fn record_runtime_summary(
        self,
        courier: CourierId,
        summary: CourierRuntimeSummary,
        tick: u64,
    ) -> Result<(), CourierRuntimeSinkError> {
        // SAFETY: the sink provider guarantees the pointed backing outlives the runtime using it.
        unsafe { (self.vtable.record_runtime_summary)(self.context, courier, summary, tick) }
    }

    pub fn runtime_ledger(
        self,
        courier: CourierId,
    ) -> Result<CourierRuntimeLedger, CourierRuntimeSinkError> {
        // SAFETY: the sink provider guarantees the pointed backing outlives the runtime using it.
        unsafe { (self.vtable.runtime_ledger)(self.context, courier) }
    }

    pub fn fiber_record(
        self,
        courier: CourierId,
        fiber: FiberId,
    ) -> Result<Option<CourierFiberRecord>, CourierRuntimeSinkError> {
        // SAFETY: the sink provider guarantees the pointed backing outlives the runtime using it.
        unsafe { (self.vtable.fiber_record)(self.context, courier, fiber) }
    }

    pub fn evaluate_responsiveness(
        self,
        courier: CourierId,
        tick: u64,
    ) -> Result<CourierResponsiveness, CourierRuntimeSinkError> {
        // SAFETY: the sink provider guarantees the pointed backing outlives the runtime using it.
        unsafe { (self.vtable.evaluate_responsiveness)(self.context, courier, tick) }
    }

    pub fn upsert_metadata(
        self,
        courier: CourierId,
        subject: CourierMetadataSubject,
        key: &'static str,
        value: &'static str,
        tick: u64,
    ) -> Result<(), CourierRuntimeSinkError> {
        // SAFETY: the sink provider guarantees the pointed backing outlives the runtime using it.
        unsafe { (self.vtable.upsert_metadata)(self.context, courier, subject, key, value, tick) }
    }

    pub fn remove_metadata(
        self,
        courier: CourierId,
        subject: CourierMetadataSubject,
        key: &str,
    ) -> Result<(), CourierRuntimeSinkError> {
        // SAFETY: the sink provider guarantees the pointed backing outlives the runtime using it.
        unsafe { (self.vtable.remove_metadata)(self.context, courier, subject, key) }
    }

    pub fn register_obligation(
        self,
        courier: CourierId,
        spec: CourierObligationSpec<'static>,
        tick: u64,
    ) -> Result<CourierObligationId, CourierRuntimeSinkError> {
        // SAFETY: the sink provider guarantees the pointed backing outlives the runtime using it.
        unsafe { (self.vtable.register_obligation)(self.context, courier, spec, tick) }
    }

    pub fn record_obligation_progress(
        self,
        courier: CourierId,
        obligation: CourierObligationId,
        tick: u64,
    ) -> Result<(), CourierRuntimeSinkError> {
        // SAFETY: the sink provider guarantees the pointed backing outlives the runtime using it.
        unsafe { (self.vtable.record_obligation_progress)(self.context, courier, obligation, tick) }
    }

    pub fn remove_obligation(
        self,
        courier: CourierId,
        obligation: CourierObligationId,
    ) -> Result<(), CourierRuntimeSinkError> {
        // SAFETY: the sink provider guarantees the pointed backing outlives the runtime using it.
        unsafe { (self.vtable.remove_obligation)(self.context, courier, obligation) }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct CourierRuntimeSinkVTable {
    pub record_context:
        unsafe fn(*mut (), CourierId, RuntimeContextId, u64) -> Result<(), CourierRuntimeSinkError>,
    pub register_fiber: unsafe fn(
        *mut (),
        CourierId,
        ManagedFiberSnapshot,
        u64,
        CourierFiberClass,
        bool,
        Option<FiberMetadataAttachment>,
        u64,
    ) -> Result<(), CourierRuntimeSinkError>,
    pub update_fiber: unsafe fn(
        *mut (),
        CourierId,
        ManagedFiberSnapshot,
        u64,
    ) -> Result<(), CourierRuntimeSinkError>,
    pub mark_fiber_terminal: unsafe fn(
        *mut (),
        CourierId,
        FiberId,
        FiberTerminalStatus,
        u64,
    ) -> Result<(), CourierRuntimeSinkError>,
    pub record_runtime_summary: unsafe fn(
        *mut (),
        CourierId,
        CourierRuntimeSummary,
        u64,
    ) -> Result<(), CourierRuntimeSinkError>,
    pub runtime_ledger:
        unsafe fn(*mut (), CourierId) -> Result<CourierRuntimeLedger, CourierRuntimeSinkError>,
    pub fiber_record: unsafe fn(
        *mut (),
        CourierId,
        FiberId,
    ) -> Result<Option<CourierFiberRecord>, CourierRuntimeSinkError>,
    pub evaluate_responsiveness:
        unsafe fn(
            *mut (),
            CourierId,
            u64,
        ) -> Result<CourierResponsiveness, CourierRuntimeSinkError>,
    pub upsert_metadata: unsafe fn(
        *mut (),
        CourierId,
        CourierMetadataSubject,
        &'static str,
        &'static str,
        u64,
    ) -> Result<(), CourierRuntimeSinkError>,
    pub remove_metadata: unsafe fn(
        *mut (),
        CourierId,
        CourierMetadataSubject,
        &str,
    ) -> Result<(), CourierRuntimeSinkError>,
    pub register_obligation: unsafe fn(
        *mut (),
        CourierId,
        CourierObligationSpec<'static>,
        u64,
    ) -> Result<CourierObligationId, CourierRuntimeSinkError>,
    pub record_obligation_progress: unsafe fn(
        *mut (),
        CourierId,
        CourierObligationId,
        u64,
    ) -> Result<(), CourierRuntimeSinkError>,
    pub remove_obligation:
        unsafe fn(*mut (), CourierId, CourierObligationId) -> Result<(), CourierRuntimeSinkError>,
}

impl CourierRuntimeSummary {
    #[must_use]
    pub const fn new(
        policy: CourierSchedulingPolicy,
        responsiveness: CourierResponsiveness,
    ) -> Self {
        Self {
            policy,
            run_state: CourierRunState::Idle,
            responsiveness,
            fiber_lane: None,
            async_lane: None,
            control_lane: None,
        }
    }

    #[must_use]
    pub fn with_fiber_lane(mut self, lane: CourierLaneSummary) -> Self {
        self.fiber_lane = Some(lane);
        self.run_state = self.derive_run_state();
        self
    }

    #[must_use]
    pub fn with_async_lane(mut self, lane: CourierLaneSummary) -> Self {
        self.async_lane = Some(lane);
        self.run_state = self.derive_run_state();
        self
    }

    #[must_use]
    pub fn with_control_lane(mut self, lane: CourierLaneSummary) -> Self {
        self.control_lane = Some(lane);
        self.run_state = self.derive_run_state();
        self
    }

    #[must_use]
    pub fn total_active_units(self) -> usize {
        self.fiber_lane.map_or(0, |lane| lane.active_units)
            + self.async_lane.map_or(0, |lane| lane.active_units)
            + self.control_lane.map_or(0, |lane| lane.active_units)
    }

    #[must_use]
    pub fn total_runnable_units(self) -> usize {
        self.fiber_lane.map_or(0, |lane| lane.runnable_units)
            + self.async_lane.map_or(0, |lane| lane.runnable_units)
            + self.control_lane.map_or(0, |lane| lane.runnable_units)
    }

    #[must_use]
    pub fn total_running_units(self) -> usize {
        self.fiber_lane.map_or(0, |lane| lane.running_units)
            + self.async_lane.map_or(0, |lane| lane.running_units)
            + self.control_lane.map_or(0, |lane| lane.running_units)
    }

    #[must_use]
    pub fn is_idle(self) -> bool {
        self.total_active_units() == 0
    }

    #[must_use]
    pub fn with_responsiveness(mut self, responsiveness: CourierResponsiveness) -> Self {
        self.responsiveness = responsiveness;
        self.run_state = self.derive_run_state();
        self
    }

    fn derive_run_state(self) -> CourierRunState {
        match self.responsiveness {
            CourierResponsiveness::NonResponsive => CourierRunState::NonResponsive,
            CourierResponsiveness::Stale => CourierRunState::Stale,
            CourierResponsiveness::Responsive => {
                if self.total_running_units() != 0 {
                    CourierRunState::Running
                } else if self.total_runnable_units() != 0 {
                    CourierRunState::Runnable
                } else {
                    CourierRunState::Idle
                }
            }
        }
    }
}

/// Opaque locator for one optional richer fiber-local published-detail lane observed by the
/// courier.
///
/// This is not the authoritative substrate metadata surface. Courier-owned metadata remains the
/// truth, and any per-fiber channel-visible detail is optional only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FiberMetadataAttachment(usize);

impl FiberMetadataAttachment {
    #[must_use]
    pub const fn new(value: usize) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> usize {
        self.0
    }
}

/// One app-defined in-memory metadata subject supervised by one courier.
///
/// Fixed substrate truth such as child launch state, fiber ledgers, and runtime summaries lives in
/// dedicated typed records above. This subject space is the overlay lane for app/service-defined
/// metadata that still belongs to the courier but should not compete with the substrate's fixed
/// schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CourierAppMetadataSubject {
    Courier,
    ChildCourier(CourierId),
    Fiber(FiberId),
    Context(crate::domain::context::ContextId),
    AsyncLane,
}

pub type CourierMetadataSubject = CourierAppMetadataSubject;

/// One courier-owned app metadata entry.
///
/// The first implementation stays deliberately narrow: one subject, one string key, one string
/// value, and one update tick. That is enough to prove the app-overlay lane without prematurely
/// locking in one more elaborate schema.
///
/// TODO: Lift values beyond `&str` once the kernel/user metadata vocabulary settles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CourierAppMetadataEntry<'a> {
    pub subject: CourierAppMetadataSubject,
    pub key: &'a str,
    pub value: &'a str,
    pub updated_tick: u64,
}

pub type CourierMetadataEntry<'a> = CourierAppMetadataEntry<'a>;

impl<'a> CourierAppMetadataEntry<'a> {
    #[must_use]
    pub const fn new(
        subject: CourierAppMetadataSubject,
        key: &'a str,
        value: &'a str,
        updated_tick: u64,
    ) -> Self {
        Self {
            subject,
            key,
            value,
            updated_tick,
        }
    }
}

/// Fixed-capacity courier-owned app metadata store.
///
/// This is authoritative courier-owned overlay state, not an event stream. Fixed substrate truth
/// remains in typed ledgers; fibers, child couriers, contexts, and async lanes may still attach
/// richer app/service-defined notes here directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CourierAppMetadataStore<'a, const MAX_RECORDS: usize> {
    records: [Option<CourierAppMetadataEntry<'a>>; MAX_RECORDS],
}

pub type CourierMetadataStore<'a, const MAX_RECORDS: usize> =
    CourierAppMetadataStore<'a, MAX_RECORDS>;

impl<'a, const MAX_RECORDS: usize> CourierAppMetadataStore<'a, MAX_RECORDS> {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            records: [None; MAX_RECORDS],
        }
    }

    pub fn upsert(
        &mut self,
        subject: CourierAppMetadataSubject,
        key: &'a str,
        value: &'a str,
        updated_tick: u64,
    ) -> Result<(), crate::domain::DomainError> {
        if let Some(entry) = self
            .records
            .iter_mut()
            .flatten()
            .find(|entry| entry.subject == subject && entry.key == key)
        {
            *entry = CourierAppMetadataEntry::new(subject, key, value, updated_tick);
            return Ok(());
        }
        let Some(slot) = self.records.iter_mut().find(|slot| slot.is_none()) else {
            return Err(crate::domain::DomainError::resource_exhausted());
        };
        *slot = Some(CourierAppMetadataEntry::new(
            subject,
            key,
            value,
            updated_tick,
        ));
        Ok(())
    }

    pub fn remove(
        &mut self,
        subject: CourierAppMetadataSubject,
        key: &str,
    ) -> Result<(), crate::domain::DomainError> {
        let Some(slot) = self
            .records
            .iter_mut()
            .find(|slot| slot.is_some_and(|entry| entry.subject == subject && entry.key == key))
        else {
            return Err(crate::domain::DomainError::not_found());
        };
        *slot = None;
        Ok(())
    }

    #[must_use]
    pub fn entry(
        &self,
        subject: CourierAppMetadataSubject,
        key: &str,
    ) -> Option<&CourierAppMetadataEntry<'a>> {
        self.records
            .iter()
            .flatten()
            .find(|entry| entry.subject == subject && entry.key == key)
    }

    pub fn entries(
        &self,
        subject: CourierAppMetadataSubject,
    ) -> impl Iterator<Item = &CourierAppMetadataEntry<'a>> {
        self.records
            .iter()
            .flatten()
            .filter(move |entry| entry.subject == subject)
    }

    pub fn iter(&self) -> impl Iterator<Item = &CourierAppMetadataEntry<'a>> {
        self.records.iter().flatten()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.iter().count()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl<'a, const MAX_RECORDS: usize> Default for CourierAppMetadataStore<'a, MAX_RECORDS> {
    fn default() -> Self {
        Self::new()
    }
}

/// Kind of externally visible service contract supervised by one courier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CourierObligationKind {
    Channel,
    ChannelAttachment,
    Service,
    Input,
    Device,
    Custom,
}

/// Binding descriptor for one externally visible courier obligation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CourierObligationBinding<'a> {
    Channel(&'a str),
    ChannelAttachment { channel: &'a str, attachment: usize },
    Service(&'a str),
    Input(&'a str),
    Device(&'a str),
    Custom(&'a str),
}

impl<'a> CourierObligationBinding<'a> {
    #[must_use]
    pub const fn kind(self) -> CourierObligationKind {
        match self {
            Self::Channel(_) => CourierObligationKind::Channel,
            Self::ChannelAttachment { .. } => CourierObligationKind::ChannelAttachment,
            Self::Service(_) => CourierObligationKind::Service,
            Self::Input(_) => CourierObligationKind::Input,
            Self::Device(_) => CourierObligationKind::Device,
            Self::Custom(_) => CourierObligationKind::Custom,
        }
    }

    #[must_use]
    pub const fn label(self) -> &'a str {
        match self {
            Self::Channel(label)
            | Self::Service(label)
            | Self::Input(label)
            | Self::Device(label)
            | Self::Custom(label) => label,
            Self::ChannelAttachment { channel, .. } => channel,
        }
    }
}

/// One externally visible courier obligation request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CourierObligationSpec<'a> {
    pub subject: CourierMetadataSubject,
    pub binding: CourierObligationBinding<'a>,
    pub stale_after_ticks: u64,
    pub non_responsive_after_ticks: u64,
}

impl<'a> CourierObligationSpec<'a> {
    #[must_use]
    pub const fn new(
        subject: CourierMetadataSubject,
        binding: CourierObligationBinding<'a>,
        stale_after_ticks: u64,
        non_responsive_after_ticks: u64,
    ) -> Self {
        Self {
            subject,
            binding,
            stale_after_ticks,
            non_responsive_after_ticks,
        }
    }

    #[must_use]
    pub const fn custom(
        subject: CourierMetadataSubject,
        label: &'a str,
        stale_after_ticks: u64,
        non_responsive_after_ticks: u64,
    ) -> Self {
        Self::new(
            subject,
            CourierObligationBinding::Custom(label),
            stale_after_ticks,
            non_responsive_after_ticks,
        )
    }
}

/// Stable identifier for one externally visible courier obligation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CourierObligationId(usize);

impl CourierObligationId {
    #[must_use]
    pub const fn new(value: usize) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> usize {
        self.0
    }
}

/// One externally visible service obligation supervised by a courier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CourierObligationRecord<'a> {
    pub id: CourierObligationId,
    pub subject: CourierMetadataSubject,
    pub kind: CourierObligationKind,
    pub binding: CourierObligationBinding<'a>,
    pub label: &'a str,
    pub stale_after_ticks: u64,
    pub non_responsive_after_ticks: u64,
    pub last_progress_tick: u64,
    pub responsiveness: CourierResponsiveness,
}

impl<'a> CourierObligationRecord<'a> {
    #[must_use]
    pub const fn new(
        id: CourierObligationId,
        spec: CourierObligationSpec<'a>,
        last_progress_tick: u64,
    ) -> Self {
        Self {
            id,
            subject: spec.subject,
            kind: spec.binding.kind(),
            binding: spec.binding,
            label: spec.binding.label(),
            stale_after_ticks: spec.stale_after_ticks,
            non_responsive_after_ticks: spec.non_responsive_after_ticks,
            last_progress_tick,
            responsiveness: CourierResponsiveness::Responsive,
        }
    }

    #[must_use]
    pub fn evaluate_at(mut self, now_tick: u64) -> Self {
        let elapsed = now_tick.saturating_sub(self.last_progress_tick);
        self.responsiveness =
            if self.non_responsive_after_ticks != 0 && elapsed >= self.non_responsive_after_ticks {
                CourierResponsiveness::NonResponsive
            } else if self.stale_after_ticks != 0 && elapsed >= self.stale_after_ticks {
                CourierResponsiveness::Stale
            } else {
                CourierResponsiveness::Responsive
            };
        self
    }
}

/// Fixed-capacity courier-owned externally visible obligation registry.
///
/// Obligations are the truthful basis for `Stale` / `NonResponsive`. Silence without any active
/// obligation means nothing; an obligation aging out means something very specific broke.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CourierObligationRegistry<'a, const MAX_OBLIGATIONS: usize> {
    next_id: usize,
    records: [Option<CourierObligationRecord<'a>>; MAX_OBLIGATIONS],
}

impl<'a, const MAX_OBLIGATIONS: usize> CourierObligationRegistry<'a, MAX_OBLIGATIONS> {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            next_id: 1,
            records: [None; MAX_OBLIGATIONS],
        }
    }

    pub fn register(
        &mut self,
        spec: CourierObligationSpec<'a>,
        tick: u64,
    ) -> Result<CourierObligationId, crate::domain::DomainError> {
        if spec.stale_after_ticks == 0 && spec.non_responsive_after_ticks == 0 {
            return Err(crate::domain::DomainError::invalid());
        }
        if spec.stale_after_ticks != 0
            && spec.non_responsive_after_ticks != 0
            && spec.non_responsive_after_ticks < spec.stale_after_ticks
        {
            return Err(crate::domain::DomainError::invalid());
        }
        let Some(slot) = self.records.iter_mut().find(|slot| slot.is_none()) else {
            return Err(crate::domain::DomainError::resource_exhausted());
        };
        let id = CourierObligationId::new(self.next_id);
        self.next_id = self.next_id.saturating_add(1).max(1);
        *slot = Some(CourierObligationRecord::new(id, spec, tick));
        Ok(id)
    }

    pub fn record_progress(
        &mut self,
        obligation: CourierObligationId,
        tick: u64,
    ) -> Result<(), crate::domain::DomainError> {
        let Some(record) = self
            .records
            .iter_mut()
            .flatten()
            .find(|record| record.id == obligation)
        else {
            return Err(crate::domain::DomainError::not_found());
        };
        record.last_progress_tick = tick;
        record.responsiveness = CourierResponsiveness::Responsive;
        Ok(())
    }

    pub fn remove(
        &mut self,
        obligation: CourierObligationId,
    ) -> Result<(), crate::domain::DomainError> {
        let Some(slot) = self
            .records
            .iter_mut()
            .find(|slot| slot.is_some_and(|record| record.id == obligation))
        else {
            return Err(crate::domain::DomainError::not_found());
        };
        *slot = None;
        Ok(())
    }

    pub fn evaluate(
        &mut self,
        now_tick: u64,
    ) -> Result<CourierResponsiveness, crate::domain::DomainError> {
        let mut worst = CourierResponsiveness::Responsive;
        for record in self.records.iter_mut().flatten() {
            *record = record.evaluate_at(now_tick);
            match record.responsiveness {
                CourierResponsiveness::NonResponsive => {
                    worst = CourierResponsiveness::NonResponsive
                }
                CourierResponsiveness::Stale
                    if !matches!(worst, CourierResponsiveness::NonResponsive) =>
                {
                    worst = CourierResponsiveness::Stale;
                }
                CourierResponsiveness::Responsive | CourierResponsiveness::Stale => {}
            }
        }
        Ok(worst)
    }

    #[must_use]
    pub fn obligation(
        &self,
        obligation: CourierObligationId,
    ) -> Option<&CourierObligationRecord<'a>> {
        self.records
            .iter()
            .flatten()
            .find(|record| record.id == obligation)
    }

    pub fn iter(&self) -> impl Iterator<Item = &CourierObligationRecord<'a>> {
        self.records.iter().flatten()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.iter().count()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl<'a, const MAX_OBLIGATIONS: usize> Default for CourierObligationRegistry<'a, MAX_OBLIGATIONS> {
    fn default() -> Self {
        Self::new()
    }
}

/// Terminal summary retained by the courier after one fiber finishes or faults.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FiberTerminalStatus {
    Completed(FiberReturn),
    Faulted(FiberErrorKind),
    Abandoned(FiberState),
}

/// Parent-owned launch and responsiveness truth for one child courier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChildCourierLaunchRecord<'a> {
    pub child: CourierId,
    pub child_name: &'a str,
    pub child_scope_role: CourierScopeRole,
    pub parent: CourierId,
    pub principal: PrincipalId<'a>,
    pub image_seal: LocalAdmissionSeal,
    pub claim_metadata: CourierClaimMetadata,
    pub launch_epoch: u64,
    pub launched_at_tick: u64,
    pub root_fiber: FiberId,
    pub responsiveness: CourierResponsiveness,
    pub last_progress_tick: u64,
}

impl<'a> ChildCourierLaunchRecord<'a> {
    #[must_use]
    pub const fn new(
        child: CourierId,
        child_name: &'a str,
        child_scope_role: CourierScopeRole,
        parent: CourierId,
        principal: PrincipalId<'a>,
        image_seal: LocalAdmissionSeal,
        claim_metadata: CourierClaimMetadata,
        launch_epoch: u64,
        launched_at_tick: u64,
        root_fiber: FiberId,
    ) -> Self {
        Self {
            child,
            child_name,
            child_scope_role,
            parent,
            principal,
            image_seal,
            claim_metadata,
            launch_epoch,
            launched_at_tick,
            root_fiber,
            responsiveness: CourierResponsiveness::Responsive,
            last_progress_tick: launched_at_tick,
        }
    }

    #[must_use]
    pub const fn with_last_progress_tick(mut self, tick: u64) -> Self {
        self.last_progress_tick = tick;
        self.responsiveness = CourierResponsiveness::Responsive;
        self
    }
}

/// One lineage hop in a courier ancestry chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CourierPedigreeRecord<'a> {
    pub courier: CourierId,
    pub name: &'a str,
    pub scope_role: CourierScopeRole,
    pub parent: Option<CourierId>,
    pub launch: Option<ChildCourierLaunchRecord<'a>>,
}

impl<'a> CourierPedigreeRecord<'a> {
    #[must_use]
    pub const fn new(
        courier: CourierId,
        name: &'a str,
        scope_role: CourierScopeRole,
        parent: Option<CourierId>,
        launch: Option<ChildCourierLaunchRecord<'a>>,
    ) -> Self {
        Self {
            courier,
            name,
            scope_role,
            parent,
            launch,
        }
    }
}

/// Fixed-capacity courier ancestry chain.
///
/// The first entry is always the queried courier itself. Subsequent entries walk upward through
/// parents until the root is reached or the fixed review budget is exhausted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CourierPedigree<'a, const MAX_DEPTH: usize> {
    depth: usize,
    records: [Option<CourierPedigreeRecord<'a>>; MAX_DEPTH],
}

impl<'a, const MAX_DEPTH: usize> CourierPedigree<'a, MAX_DEPTH> {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            depth: 0,
            records: [None; MAX_DEPTH],
        }
    }

    pub fn push(
        &mut self,
        record: CourierPedigreeRecord<'a>,
    ) -> Result<(), crate::domain::DomainError> {
        if self.depth >= MAX_DEPTH {
            return Err(crate::domain::DomainError::resource_exhausted());
        }
        self.records[self.depth] = Some(record);
        self.depth += 1;
        Ok(())
    }

    #[must_use]
    pub const fn len(&self) -> usize {
        self.depth
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.depth == 0
    }

    #[must_use]
    pub fn iter(&self) -> impl Iterator<Item = &CourierPedigreeRecord<'a>> {
        self.records[..self.depth].iter().flatten()
    }

    #[must_use]
    pub fn leaf(&self) -> Option<&CourierPedigreeRecord<'a>> {
        self.records.first().and_then(Option::as_ref)
    }

    #[must_use]
    pub fn root(&self) -> Option<&CourierPedigreeRecord<'a>> {
        if self.depth == 0 {
            None
        } else {
            self.records[self.depth - 1].as_ref()
        }
    }
}

impl<'a, const MAX_DEPTH: usize> Default for CourierPedigree<'a, MAX_DEPTH> {
    fn default() -> Self {
        Self::new()
    }
}

/// Authoritative courier-owned snapshot for one live or recently terminal fiber.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CourierFiberRecord {
    pub fiber: FiberId,
    pub generation: u64,
    pub class: CourierFiberClass,
    pub state: FiberState,
    pub started: bool,
    pub claim_awareness: ClaimAwareness,
    pub claim_context: Option<ClaimContextId>,
    pub is_root: bool,
    pub last_transition_tick: u64,
    pub last_progress_tick: u64,
    pub terminal: Option<FiberTerminalStatus>,
    pub metadata_attachment: Option<FiberMetadataAttachment>,
    pub responsiveness: CourierResponsiveness,
}

impl CourierFiberRecord {
    #[must_use]
    pub const fn from_snapshot(
        snapshot: ManagedFiberSnapshot,
        generation: u64,
        class: CourierFiberClass,
        is_root: bool,
        metadata_attachment: Option<FiberMetadataAttachment>,
        tick: u64,
    ) -> Self {
        Self {
            fiber: snapshot.id,
            generation,
            class,
            state: snapshot.state,
            started: snapshot.started,
            claim_awareness: snapshot.claim_awareness,
            claim_context: snapshot.claim_context,
            is_root,
            last_transition_tick: tick,
            last_progress_tick: tick,
            terminal: None,
            metadata_attachment,
            responsiveness: CourierResponsiveness::Responsive,
        }
    }

    pub fn update_from_snapshot(&mut self, snapshot: ManagedFiberSnapshot, tick: u64) {
        if self.state != snapshot.state
            || self.started != snapshot.started
            || self.claim_awareness != snapshot.claim_awareness
            || self.claim_context != snapshot.claim_context
        {
            self.last_transition_tick = tick;
        }
        self.state = snapshot.state;
        self.started = snapshot.started;
        self.claim_awareness = snapshot.claim_awareness;
        self.claim_context = snapshot.claim_context;
        self.last_progress_tick = tick;
        self.responsiveness = CourierResponsiveness::Responsive;
    }

    pub fn mark_terminal(&mut self, terminal: FiberTerminalStatus, tick: u64) {
        self.last_transition_tick = tick;
        self.last_progress_tick = tick;
        self.responsiveness = CourierResponsiveness::Responsive;
        self.state = match terminal {
            FiberTerminalStatus::Completed(_) => FiberState::Completed,
            FiberTerminalStatus::Abandoned(lifecycle) => lifecycle,
            FiberTerminalStatus::Faulted(_) => self.state,
        };
        self.terminal = Some(terminal);
    }
}

/// Fixed-capacity parent-owned registry of child courier launch truth.
// TODO: thread this through the live runtime/domain substrate so parent/child registration stops
// living only in isolated demo registries and authority helpers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChildCourierRegistry<'a, const MAX_CHILDREN: usize> {
    records: [Option<ChildCourierLaunchRecord<'a>>; MAX_CHILDREN],
}

impl<'a, const MAX_CHILDREN: usize> ChildCourierRegistry<'a, MAX_CHILDREN> {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            records: [None; MAX_CHILDREN],
        }
    }

    pub fn register(
        &mut self,
        record: ChildCourierLaunchRecord<'a>,
    ) -> Result<(), crate::domain::DomainError> {
        if self
            .records
            .iter()
            .flatten()
            .any(|existing| existing.child == record.child)
        {
            return Err(crate::domain::DomainError::state_conflict());
        }
        let Some(slot) = self.records.iter_mut().find(|slot| slot.is_none()) else {
            return Err(crate::domain::DomainError::resource_exhausted());
        };
        *slot = Some(record);
        Ok(())
    }

    #[must_use]
    pub fn child(&self, courier: CourierId) -> Option<&ChildCourierLaunchRecord<'a>> {
        self.records
            .iter()
            .flatten()
            .find(|record| record.child == courier)
    }

    pub fn record_progress(
        &mut self,
        courier: CourierId,
        tick: u64,
    ) -> Result<(), crate::domain::DomainError> {
        let Some(record) = self
            .records
            .iter_mut()
            .flatten()
            .find(|record| record.child == courier)
        else {
            return Err(crate::domain::DomainError::not_found());
        };
        record.last_progress_tick = tick;
        record.responsiveness = CourierResponsiveness::Responsive;
        Ok(())
    }

    pub fn mark_stale(&mut self, courier: CourierId) -> Result<(), crate::domain::DomainError> {
        let Some(record) = self
            .records
            .iter_mut()
            .flatten()
            .find(|record| record.child == courier)
        else {
            return Err(crate::domain::DomainError::not_found());
        };
        record.responsiveness = CourierResponsiveness::Stale;
        Ok(())
    }

    pub fn mark_non_responsive(
        &mut self,
        courier: CourierId,
    ) -> Result<(), crate::domain::DomainError> {
        let Some(record) = self
            .records
            .iter_mut()
            .flatten()
            .find(|record| record.child == courier)
        else {
            return Err(crate::domain::DomainError::not_found());
        };
        record.responsiveness = CourierResponsiveness::NonResponsive;
        Ok(())
    }

    pub fn iter(&self) -> impl Iterator<Item = &ChildCourierLaunchRecord<'a>> {
        self.records.iter().flatten()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.iter().count()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl<'a, const MAX_CHILDREN: usize> Default for ChildCourierRegistry<'a, MAX_CHILDREN> {
    fn default() -> Self {
        Self::new()
    }
}

/// Fixed-capacity authoritative fiber ledger supervised by one courier.
// TODO: feed this directly from the real `fusion-std` fiber runtime/task records so the courier
// ledger becomes the actual substrate truth instead of one isolated supervision helper.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CourierFiberLedger<const MAX_FIBERS: usize> {
    records: [Option<CourierFiberRecord>; MAX_FIBERS],
}

impl<const MAX_FIBERS: usize> CourierFiberLedger<MAX_FIBERS> {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            records: [None; MAX_FIBERS],
        }
    }

    pub fn register(
        &mut self,
        snapshot: ManagedFiberSnapshot,
        generation: u64,
        class: CourierFiberClass,
        is_root: bool,
        metadata_attachment: Option<FiberMetadataAttachment>,
        tick: u64,
    ) -> Result<(), crate::domain::DomainError> {
        if self
            .records
            .iter()
            .flatten()
            .any(|record| record.fiber == snapshot.id)
        {
            return Err(crate::domain::DomainError::state_conflict());
        }
        let Some(slot) = self.records.iter_mut().find(|slot| slot.is_none()) else {
            return Err(crate::domain::DomainError::resource_exhausted());
        };
        *slot = Some(CourierFiberRecord::from_snapshot(
            snapshot,
            generation,
            class,
            is_root,
            metadata_attachment,
            tick,
        ));
        Ok(())
    }

    #[must_use]
    pub fn fiber(&self, fiber: FiberId) -> Option<&CourierFiberRecord> {
        self.records
            .iter()
            .flatten()
            .find(|record| record.fiber == fiber)
    }

    pub fn update_from_snapshot(
        &mut self,
        snapshot: ManagedFiberSnapshot,
        tick: u64,
    ) -> Result<(), crate::domain::DomainError> {
        let Some(record) = self
            .records
            .iter_mut()
            .flatten()
            .find(|record| record.fiber == snapshot.id)
        else {
            return Err(crate::domain::DomainError::not_found());
        };
        record.update_from_snapshot(snapshot, tick);
        Ok(())
    }

    pub fn mark_terminal(
        &mut self,
        fiber: FiberId,
        terminal: FiberTerminalStatus,
        tick: u64,
    ) -> Result<(), crate::domain::DomainError> {
        let Some(record) = self
            .records
            .iter_mut()
            .flatten()
            .find(|record| record.fiber == fiber)
        else {
            return Err(crate::domain::DomainError::not_found());
        };
        record.mark_terminal(terminal, tick);
        Ok(())
    }

    pub fn mark_stale(&mut self, fiber: FiberId) -> Result<(), crate::domain::DomainError> {
        let Some(record) = self
            .records
            .iter_mut()
            .flatten()
            .find(|record| record.fiber == fiber)
        else {
            return Err(crate::domain::DomainError::not_found());
        };
        record.responsiveness = CourierResponsiveness::Stale;
        Ok(())
    }

    pub fn mark_non_responsive(
        &mut self,
        fiber: FiberId,
    ) -> Result<(), crate::domain::DomainError> {
        let Some(record) = self
            .records
            .iter_mut()
            .flatten()
            .find(|record| record.fiber == fiber)
        else {
            return Err(crate::domain::DomainError::not_found());
        };
        record.responsiveness = CourierResponsiveness::NonResponsive;
        Ok(())
    }

    pub fn iter(&self) -> impl Iterator<Item = &CourierFiberRecord> {
        self.records.iter().flatten()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.iter().count()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl<const MAX_FIBERS: usize> Default for CourierFiberLedger<MAX_FIBERS> {
    fn default() -> Self {
        Self::new()
    }
}

/// Static courier descriptor carried by one runtime launch request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CourierLaunchDescriptor<'a> {
    pub id: CourierId,
    pub name: &'a str,
    pub scope_role: CourierScopeRole,
    pub caps: CourierCaps,
    pub visibility: CourierVisibility,
    pub claim_awareness: ClaimAwareness,
    pub claim_context: Option<ClaimContextId>,
    pub plan: CourierPlan,
}

/// Parent/child launch request carried by one runtime before the root fiber is admitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CourierChildLaunchRequest<'a> {
    pub parent: CourierId,
    pub descriptor: CourierLaunchDescriptor<'a>,
    pub principal: PrincipalId<'a>,
    pub image_seal: LocalAdmissionSeal,
    pub launch_epoch: u64,
}

/// Coarse launch-control error surfaced when one runtime asks the courier substrate to realize one
/// child launch tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CourierLaunchControlError {
    Unsupported,
    Invalid,
    NotFound,
    ResourceExhausted,
    StateConflict,
    Busy,
}

impl From<crate::domain::DomainError> for CourierLaunchControlError {
    fn from(value: crate::domain::DomainError) -> Self {
        use crate::domain::DomainErrorKind;
        match value.kind() {
            DomainErrorKind::Unsupported => Self::Unsupported,
            DomainErrorKind::Invalid => Self::Invalid,
            DomainErrorKind::NotFound => Self::NotFound,
            DomainErrorKind::ResourceExhausted => Self::ResourceExhausted,
            DomainErrorKind::StateConflict => Self::StateConflict,
            DomainErrorKind::NotVisible
            | DomainErrorKind::Busy
            | DomainErrorKind::PermissionDenied
            | DomainErrorKind::Platform(_) => Self::Busy,
        }
    }
}

/// Generic launch-control surface used by runtimes to realize parent/child courier truth once the
/// root fiber identity is actually known.
#[derive(Debug, Clone, Copy)]
pub struct CourierLaunchControl<'a> {
    context: *mut (),
    vtable: CourierLaunchControlVTable<'a>,
}

unsafe impl Send for CourierLaunchControl<'_> {}
unsafe impl Sync for CourierLaunchControl<'_> {}

impl PartialEq for CourierLaunchControl<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.context == other.context
            && self.vtable.register_child_courier as usize
                == other.vtable.register_child_courier as usize
    }
}

impl Eq for CourierLaunchControl<'_> {}

impl Hash for CourierLaunchControl<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.context.hash(state);
        (self.vtable.register_child_courier as usize).hash(state);
    }
}

impl<'a> CourierLaunchControl<'a> {
    #[must_use]
    pub const fn new(context: *mut (), vtable: CourierLaunchControlVTable<'a>) -> Self {
        Self { context, vtable }
    }

    pub fn register_child_courier(
        self,
        request: CourierChildLaunchRequest<'a>,
        launched_at_tick: u64,
        root_fiber: FiberId,
    ) -> Result<(), CourierLaunchControlError> {
        // SAFETY: the launch-control provider guarantees the pointed backing outlives the runtime
        // using it.
        unsafe {
            (self.vtable.register_child_courier)(
                self.context,
                request,
                launched_at_tick,
                root_fiber,
            )
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct CourierLaunchControlVTable<'a> {
    pub register_child_courier: unsafe fn(
        *mut (),
        CourierChildLaunchRequest<'a>,
        u64,
        FiberId,
    ) -> Result<(), CourierLaunchControlError>,
}

/// Static courier metadata plan for exact-static builds and reviewable footprints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CourierPlan {
    pub max_child_couriers: usize,
    pub max_live_fibers: usize,
    pub planned_fiber_capacity: usize,
    pub dynamic_fiber_capacity: usize,
    pub max_async_tasks: usize,
    pub max_runnable_units: usize,
    pub app_metadata_capacity: usize,
    pub obligation_capacity: usize,
    pub recent_dead_depth: usize,
    pub time_slice_ticks: Option<u64>,
}

impl CourierPlan {
    #[must_use]
    pub const fn new(max_child_couriers: usize, max_live_fibers: usize) -> Self {
        Self {
            max_child_couriers,
            max_live_fibers,
            planned_fiber_capacity: max_live_fibers,
            dynamic_fiber_capacity: max_live_fibers,
            max_async_tasks: max_live_fibers,
            max_runnable_units: max_live_fibers,
            app_metadata_capacity: max_live_fibers,
            obligation_capacity: max_live_fibers,
            recent_dead_depth: 0,
            time_slice_ticks: None,
        }
    }

    #[must_use]
    pub const fn with_planned_fiber_capacity(mut self, planned_fiber_capacity: usize) -> Self {
        self.planned_fiber_capacity = planned_fiber_capacity;
        self
    }

    #[must_use]
    pub const fn with_dynamic_fiber_capacity(mut self, dynamic_fiber_capacity: usize) -> Self {
        self.dynamic_fiber_capacity = dynamic_fiber_capacity;
        self
    }

    #[must_use]
    pub const fn with_async_capacity(mut self, max_async_tasks: usize) -> Self {
        self.max_async_tasks = max_async_tasks;
        self
    }

    #[must_use]
    pub const fn with_runnable_capacity(mut self, max_runnable_units: usize) -> Self {
        self.max_runnable_units = max_runnable_units;
        self
    }

    #[must_use]
    pub const fn with_app_metadata_capacity(mut self, capacity: usize) -> Self {
        self.app_metadata_capacity = capacity;
        self
    }

    #[must_use]
    pub const fn with_obligation_capacity(mut self, capacity: usize) -> Self {
        self.obligation_capacity = capacity;
        self
    }

    #[must_use]
    pub const fn with_fiber_metadata_capacity(self, capacity: usize) -> Self {
        self.with_app_metadata_capacity(capacity)
    }

    #[must_use]
    pub const fn with_child_observation_capacity(self, capacity: usize) -> Self {
        self.with_obligation_capacity(capacity)
    }

    #[must_use]
    pub const fn with_recent_dead_depth(mut self, depth: usize) -> Self {
        self.recent_dead_depth = depth;
        self
    }

    #[must_use]
    pub const fn with_time_slice_ticks(mut self, time_slice_ticks: u64) -> Self {
        self.time_slice_ticks = Some(time_slice_ticks);
        self
    }

    #[must_use]
    pub const fn is_valid(self) -> bool {
        let total_lane_capacity = self.max_live_fibers.saturating_add(self.max_async_tasks);
        self.max_live_fibers > 0
            && self.max_runnable_units > 0
            && self.planned_fiber_capacity <= self.max_live_fibers
            && self.dynamic_fiber_capacity <= self.max_live_fibers
            && self.max_async_tasks <= self.max_runnable_units
            // `max_runnable_units` is one aggregate active-work cap. It may be lower than the
            // sum of fiber+async lane capacities, but it should not exceed the total work those
            // lanes could ever admit.
            && self.max_runnable_units <= total_lane_capacity
    }

    #[must_use]
    pub const fn fits_within(self, max_child_couriers: usize, max_live_fibers: usize) -> bool {
        self.max_child_couriers <= max_child_couriers && self.max_live_fibers <= max_live_fibers
    }
}

/// One compile-time planned fiber participating in a courier's exact counted envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PlannedFiberSpec {
    pub is_root: bool,
    pub claim_awareness: ClaimAwareness,
}

impl PlannedFiberSpec {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            is_root: false,
            claim_awareness: ClaimAwareness::Blind,
        }
    }

    #[must_use]
    pub const fn root(mut self, is_root: bool) -> Self {
        self.is_root = is_root;
        self
    }

    #[must_use]
    pub const fn with_claim_awareness(mut self, claim_awareness: ClaimAwareness) -> Self {
        self.claim_awareness = claim_awareness;
        self
    }
}

impl Default for PlannedFiberSpec {
    fn default() -> Self {
        Self::new()
    }
}

/// Claim-facing helper surface for any courier implementation.
pub trait CourierClaims: CourierBaseContract {
    /// Returns whether this courier is currently claim-blind or black/claim-enabled.
    fn claim_awareness(&self) -> ClaimAwareness {
        self.courier_support().claim_awareness()
    }

    /// Returns the active claim-context identifier carried by this courier, if any.
    fn claim_context(&self) -> Option<ClaimContextId> {
        self.courier_support().claim_context()
    }

    /// Returns one compact snapshot of the courier's current claim mediation state.
    fn claim_metadata(&self) -> CourierClaimMetadata {
        CourierClaimMetadata {
            awareness: self.claim_awareness(),
            context: self.claim_context(),
        }
    }

    /// Returns whether this courier can currently mediate claim requests.
    fn is_claim_enabled(&self) -> bool {
        self.claim_metadata().is_enabled()
    }

    /// Returns the live claim-context ID or one honest denial when the courier cannot mediate.
    fn require_claim_context(&self) -> Result<ClaimContextId, ClaimsError> {
        if self.claim_awareness().is_blind() {
            return Err(ClaimsError::permission_denied());
        }
        self.claim_context()
            .ok_or_else(ClaimsError::permission_denied)
    }

    /// Validates that this courier is mediating the supplied claim context.
    fn validate_claim_context(&self, expected: ClaimContextId) -> Result<(), ClaimsError> {
        validate_courier_claim_context(self.courier_support(), expected)
    }

    /// Validates that one black fiber is running under this courier's current claim context.
    fn validate_fiber_claim_context(
        &self,
        fiber_awareness: ClaimAwareness,
        fiber_claim_context: Option<ClaimContextId>,
    ) -> Result<ClaimContextId, ClaimsError> {
        validate_fiber_claim_context(self.courier_support(), fiber_awareness, fiber_claim_context)
    }
}

impl<T: CourierBaseContract> CourierClaims for T {}

/// Readable metadata/introspection surface for one courier.
pub trait CourierIntrospection: CourierClaims {
    /// Returns the courier's visible scope role.
    fn scope_role(&self) -> CourierScopeRole {
        CourierScopeRole::Leaf
    }

    /// Returns one stable metadata snapshot for this courier.
    fn metadata(&self) -> CourierMetadata<'_> {
        CourierMetadata {
            id: self.courier_id(),
            name: self.name(),
            scope_role: self.scope_role(),
            support: self.courier_support(),
        }
    }

    /// Returns the owning domain identifier for this courier.
    fn domain_id(&self) -> crate::domain::DomainId {
        self.metadata().domain_id()
    }

    /// Returns the implementation kind for this courier.
    fn implementation_kind(&self) -> CourierImplementationKind {
        self.courier_support().implementation
    }

    /// Returns the courier's capabilities.
    fn caps(&self) -> CourierCaps {
        self.courier_support().caps
    }

    /// Returns the courier's visibility mode.
    fn visibility(&self) -> CourierVisibility {
        self.metadata().visibility()
    }

    /// Returns whether the courier exposes full domain-wide context visibility.
    fn is_full_visibility(&self) -> bool {
        self.courier_support().is_full_visibility()
    }

    /// Returns whether the courier is scoped to its explicit visible-context set.
    fn is_scoped_visibility(&self) -> bool {
        self.courier_support().is_scoped_visibility()
    }
}

impl<T: CourierBaseContract> CourierIntrospection for T {}

/// Validates that one courier can mediate claims for the supplied claim context.
///
/// # Errors
///
/// Returns an honest denial when the courier is claim-blind or carries a different claim context.
pub fn validate_courier_claim_context(
    support: CourierSupport,
    expected: ClaimContextId,
) -> Result<(), ClaimsError> {
    if support.claim_awareness().is_blind() {
        return Err(ClaimsError::permission_denied());
    }
    if support.claim_context() != Some(expected) {
        return Err(ClaimsError::permission_denied());
    }
    Ok(())
}

/// Validates that one black fiber is still running under one courier-mediated claim context.
///
/// # Errors
///
/// Returns an honest denial when either side is claim-blind or the fiber points at a different
/// claim context than the courier currently mediates.
pub fn validate_fiber_claim_context(
    support: CourierSupport,
    fiber_awareness: ClaimAwareness,
    fiber_claim_context: Option<ClaimContextId>,
) -> Result<ClaimContextId, ClaimsError> {
    if !fiber_awareness.is_black() || support.claim_awareness().is_blind() {
        return Err(ClaimsError::permission_denied());
    }
    let context = support
        .claim_context()
        .ok_or_else(ClaimsError::permission_denied)?;
    if fiber_claim_context != Some(context) {
        return Err(ClaimsError::permission_denied());
    }
    Ok(context)
}

#[cfg(all(test, feature = "std", not(target_os = "none")))]
mod tests;
