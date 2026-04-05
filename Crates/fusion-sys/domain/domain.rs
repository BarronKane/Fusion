//! fusion-sys domain registry and native domain/courier/context demonstration.

#[path = "context/context.rs"]
/// Visible-context contracts that belong inside the domain/courier model.
pub mod context;

pub use fusion_pal::sys::domain::*;

use crate::claims::{
    ClaimAwareness,
    ClaimContextId,
    LocalAdmissionSeal,
    PrincipalId,
};
use self::context::{
    ContextBaseContract,
    ContextCaps,
    ContextId,
    ContextImplementationKind,
    ContextKind,
    ContextProjectionKind,
    ContextSupport,
};
use crate::courier::{
    ChildCourierLaunchRecord,
    ChildCourierRegistry,
    CourierAppMetadataStore,
    CourierBaseContract,
    CourierCaps,
    CourierChildLaunchRequest,
    CourierFiberClass,
    CourierFiberLedger,
    CourierFiberRecord,
    CourierImplementationKind,
    CourierLaunchControl,
    CourierLaunchControlError,
    CourierLaunchControlVTable,
    CourierMetadata,
    CourierMetadataEntry,
    CourierMetadataSubject,
    CourierObligationBinding,
    CourierObligationId,
    CourierObligationRecord,
    CourierObligationRegistry,
    CourierObligationSpec,
    CourierPlan,
    CourierResponsiveness,
    CourierRuntimeLedger,
    CourierRuntimeSink,
    CourierRuntimeSinkError,
    CourierRuntimeSinkVTable,
    CourierSupport,
    CourierVisibility,
    CourierVisibilityControlContract,
    FiberMetadataAttachment,
    FiberTerminalStatus,
};
use crate::fiber::{
    FiberId,
    ManagedFiberSnapshot,
};

/// Static descriptor used to construct one native domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DomainDescriptor<'a> {
    pub id: DomainId,
    pub name: &'a str,
    pub kind: DomainKind,
    pub caps: DomainCaps,
}

/// Static descriptor used to construct one courier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CourierDescriptor<'a> {
    pub id: CourierId,
    pub name: &'a str,
    pub caps: CourierCaps,
    pub visibility: CourierVisibility,
    pub claim_awareness: ClaimAwareness,
    pub claim_context: Option<ClaimContextId>,
    pub plan: CourierPlan,
}

/// Static descriptor used to construct one visible context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextDescriptor<'a> {
    pub id: ContextId,
    pub name: &'a str,
    pub kind: ContextKind,
    pub caps: ContextCaps,
    pub claim_context: Option<ClaimContextId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DomainRecord<'a> {
    descriptor: DomainDescriptor<'a>,
    support: DomainSupport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ContextGrant {
    context: ContextId,
    projection: ContextProjectionKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CourierRecord<
    'a,
    const MAX_VISIBLE: usize,
    const MAX_CHILDREN: usize,
    const MAX_FIBERS: usize,
    const MAX_METADATA: usize,
> {
    descriptor: CourierDescriptor<'a>,
    support: CourierSupport,
    parent: Option<CourierId>,
    // Parent-facing launch truth is cached on the child too so child inspection remains possible
    // even if the user's root/main fiber wedges itself into silence later.
    launch: Option<ChildCourierLaunchRecord<'a>>,
    visible: [Option<ContextGrant>; MAX_VISIBLE],
    children: ChildCourierRegistry<'a, MAX_CHILDREN>,
    // TODO: Feed this ledger from the real scheduler/runtime paths. Today the domain substrate can
    // supervise authoritative fiber truth, but the higher `fusion-std` runtime still needs to
    // publish into it instead of carrying its own parallel worldview.
    fibers: CourierFiberLedger<MAX_FIBERS>,
    runtime: CourierRuntimeLedger,
    app_metadata: CourierAppMetadataStore<'a, MAX_METADATA>,
    obligations: CourierObligationRegistry<'a, MAX_METADATA>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ContextRecord<'a> {
    descriptor: ContextDescriptor<'a>,
    support: ContextSupport,
}

/// Fixed-capacity native domain registry used to prove the first native object model.
#[derive(Debug, Clone, Copy)]
pub struct DomainRegistry<
    'a,
    const MAX_COURIERS: usize,
    const MAX_CONTEXTS: usize,
    const MAX_VISIBLE: usize,
    const MAX_CHILDREN: usize = 4,
    const MAX_FIBERS: usize = 8,
    const MAX_METADATA: usize = 32,
> {
    domain: DomainRecord<'a>,
    couriers: [Option<CourierRecord<'a, MAX_VISIBLE, MAX_CHILDREN, MAX_FIBERS, MAX_METADATA>>;
        MAX_COURIERS],
    contexts: [Option<ContextRecord<'a>>; MAX_CONTEXTS],
}

impl<
    'a,
    const MAX_COURIERS: usize,
    const MAX_CONTEXTS: usize,
    const MAX_VISIBLE: usize,
    const MAX_CHILDREN: usize,
    const MAX_FIBERS: usize,
    const MAX_METADATA: usize,
>
    DomainRegistry<
        'a,
        MAX_COURIERS,
        MAX_CONTEXTS,
        MAX_VISIBLE,
        MAX_CHILDREN,
        MAX_FIBERS,
        MAX_METADATA,
    >
{
    /// Creates one fixed-capacity domain registry.
    #[must_use]
    pub fn new(descriptor: DomainDescriptor<'a>) -> Self {
        Self {
            domain: DomainRecord {
                descriptor,
                support: DomainSupport {
                    caps: descriptor.caps,
                    implementation: DomainImplementationKind::Native,
                    kind: descriptor.kind,
                },
            },
            couriers: [None; MAX_COURIERS],
            contexts: [None; MAX_CONTEXTS],
        }
    }

    /// Registers one courier inside the domain.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the ID already exists or storage is exhausted.
    pub fn register_courier(
        &mut self,
        descriptor: CourierDescriptor<'a>,
    ) -> Result<(), DomainError> {
        self.validate_courier_descriptor(descriptor)?;
        self.insert_courier(CourierRecord {
            descriptor,
            support: CourierSupport {
                caps: descriptor.caps,
                implementation: CourierImplementationKind::Native,
                domain: self.domain.descriptor.id,
                visibility: descriptor.visibility,
                claim_awareness: descriptor.claim_awareness,
                claim_context: descriptor.claim_context,
            },
            parent: None,
            launch: None,
            visible: [None; MAX_VISIBLE],
            children: ChildCourierRegistry::new(),
            fibers: CourierFiberLedger::new(),
            runtime: CourierRuntimeLedger::new(),
            app_metadata: CourierAppMetadataStore::new(),
            obligations: CourierObligationRegistry::new(),
        })
    }

    /// Registers one child courier under the supplied parent and records the parent's launch truth.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the parent or child is invalid, storage is exhausted, or the
    /// parent's declared courier plan cannot admit another child.
    pub fn register_child_courier(
        &mut self,
        parent: CourierId,
        descriptor: CourierDescriptor<'a>,
        principal: PrincipalId<'a>,
        image_seal: LocalAdmissionSeal,
        launch_epoch: u64,
        launched_at_tick: u64,
        root_fiber: FiberId,
    ) -> Result<(), DomainError> {
        // Child couriers publish launch truth to the parent immediately. The parent keeps this
        // ledger even if the child's root/main fiber later becomes non-responsive or simply stops
        // servicing one required externally visible interaction.
        self.validate_courier_descriptor(descriptor)?;
        let Some(parent_index) = self.index_of_courier(parent) else {
            return Err(DomainError::not_found());
        };
        if self.find_courier(descriptor.id).is_some() {
            return Err(DomainError::state_conflict());
        }
        if self.couriers.iter().all(Option::is_some) {
            return Err(DomainError::resource_exhausted());
        }
        let parent_record = self.couriers[parent_index]
            .as_ref()
            .expect("courier index should point at a live parent courier");
        if parent_record.children.len() >= parent_record.descriptor.plan.max_child_couriers {
            return Err(DomainError::resource_exhausted());
        }

        let launch = ChildCourierLaunchRecord::new(
            descriptor.id,
            parent,
            principal,
            image_seal,
            crate::courier::CourierClaimMetadata {
                awareness: descriptor.claim_awareness,
                context: descriptor.claim_context,
            },
            launch_epoch,
            launched_at_tick,
            root_fiber,
        );

        self.couriers[parent_index]
            .as_mut()
            .expect("courier index should point at a live parent courier")
            .children
            .register(launch)?;
        self.insert_courier(CourierRecord {
            descriptor,
            support: CourierSupport {
                caps: descriptor.caps,
                implementation: CourierImplementationKind::Native,
                domain: self.domain.descriptor.id,
                visibility: descriptor.visibility,
                claim_awareness: descriptor.claim_awareness,
                claim_context: descriptor.claim_context,
            },
            parent: Some(parent),
            launch: Some(launch),
            visible: [None; MAX_VISIBLE],
            children: ChildCourierRegistry::new(),
            fibers: CourierFiberLedger::new(),
            runtime: CourierRuntimeLedger::new(),
            app_metadata: CourierAppMetadataStore::new(),
            obligations: CourierObligationRegistry::new(),
        })
    }

    /// Records one observed child-courier progress event into the parent-owned launch ledger.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the child does not exist, is not parented by `parent`, or the
    /// parent has no launch record for that child.
    pub fn record_child_progress(
        &mut self,
        parent: CourierId,
        child: CourierId,
        tick: u64,
    ) -> Result<(), DomainError> {
        let Some(child_index) = self.index_of_courier(child) else {
            return Err(DomainError::not_found());
        };
        let Some(parent_index) = self.index_of_courier(parent) else {
            return Err(DomainError::not_found());
        };
        if self.couriers[child_index]
            .as_ref()
            .expect("courier index should point at a live child courier")
            .parent
            != Some(parent)
        {
            return Err(DomainError::permission_denied());
        }
        {
            let child_record = self.couriers[child_index]
                .as_mut()
                .expect("courier index should point at a live child courier");
            let Some(launch) = child_record.launch.as_mut() else {
                return Err(DomainError::state_conflict());
            };
            launch.last_progress_tick = tick;
            launch.responsiveness = CourierResponsiveness::Responsive;
        }
        self.couriers[parent_index]
            .as_mut()
            .expect("courier index should point at a live parent courier")
            .children
            .record_progress(child, tick)
    }

    /// Marks one child courier as stale in both the child-owned and parent-owned launch state.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the supplied parent/child relationship does not exist.
    pub fn mark_child_stale(
        &mut self,
        parent: CourierId,
        child: CourierId,
    ) -> Result<(), DomainError> {
        self.mark_child_responsiveness(parent, child, CourierResponsiveness::Stale)
    }

    /// Marks one child courier as non-responsive in both the child-owned and parent-owned launch
    /// state.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the supplied parent/child relationship does not exist.
    pub fn mark_child_non_responsive(
        &mut self,
        parent: CourierId,
        child: CourierId,
    ) -> Result<(), DomainError> {
        self.mark_child_responsiveness(parent, child, CourierResponsiveness::NonResponsive)
    }

    /// Registers one fiber under the owning courier's authoritative supervision ledger.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the courier does not exist, its declared plan cannot admit
    /// another live fiber, or the record conflicts with the existing ledger.
    pub fn register_fiber(
        &mut self,
        courier: CourierId,
        snapshot: ManagedFiberSnapshot,
        generation: u64,
        is_root: bool,
        metadata_attachment: Option<FiberMetadataAttachment>,
        tick: u64,
    ) -> Result<(), DomainError> {
        self.register_fiber_with_class(
            courier,
            snapshot,
            generation,
            CourierFiberClass::Dynamic,
            is_root,
            metadata_attachment,
            tick,
        )
    }

    /// Registers one fiber under the owning courier's authoritative supervision ledger with one
    /// explicit admission class.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the courier does not exist, its declared plan cannot admit
    /// another live fiber of the requested class, or the record conflicts with the existing
    /// ledger.
    pub fn register_fiber_with_class(
        &mut self,
        courier: CourierId,
        snapshot: ManagedFiberSnapshot,
        generation: u64,
        class: CourierFiberClass,
        is_root: bool,
        metadata_attachment: Option<FiberMetadataAttachment>,
        tick: u64,
    ) -> Result<(), DomainError> {
        // The metadata attachment points at optional richer fiber-local published detail, but the
        // courier ledger remains the authoritative substrate truth for lifecycle and observed
        // service progress.
        let record = self
            .find_courier_mut(courier)
            .ok_or_else(DomainError::not_found)?;
        if record.fibers.len() >= record.descriptor.plan.max_live_fibers {
            return Err(DomainError::resource_exhausted());
        }
        match class {
            CourierFiberClass::Planned
                if record.runtime.active_planned_fibers
                    >= record.descriptor.plan.planned_fiber_capacity =>
            {
                return Err(DomainError::resource_exhausted());
            }
            CourierFiberClass::Dynamic
                if record.runtime.active_dynamic_fibers
                    >= record.descriptor.plan.dynamic_fiber_capacity =>
            {
                return Err(DomainError::resource_exhausted());
            }
            CourierFiberClass::Planned | CourierFiberClass::Dynamic => {}
        }
        if is_root
            && record
                .fibers
                .iter()
                .any(|existing| existing.is_root && existing.fiber != snapshot.id)
        {
            return Err(DomainError::state_conflict());
        }
        if let Some(launch) = record.launch {
            if snapshot.id == launch.root_fiber && !is_root {
                return Err(DomainError::state_conflict());
            }
            if is_root && snapshot.id != launch.root_fiber {
                return Err(DomainError::state_conflict());
            }
        }
        record.fibers.register(
            snapshot,
            generation,
            class,
            is_root,
            metadata_attachment,
            tick,
        )?;
        record.runtime.register_fiber(class);
        Ok(())
    }

    /// Updates one authoritative courier-owned fiber snapshot.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the courier or fiber does not exist.
    pub fn update_fiber_snapshot(
        &mut self,
        courier: CourierId,
        snapshot: ManagedFiberSnapshot,
        tick: u64,
    ) -> Result<(), DomainError> {
        self.find_courier_mut(courier)
            .ok_or_else(DomainError::not_found)?
            .fibers
            .update_from_snapshot(snapshot, tick)
    }

    /// Marks one courier-owned fiber record terminal.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the courier or fiber does not exist.
    pub fn mark_fiber_terminal(
        &mut self,
        courier: CourierId,
        fiber: FiberId,
        terminal: FiberTerminalStatus,
        tick: u64,
    ) -> Result<(), DomainError> {
        let record = self
            .find_courier_mut(courier)
            .ok_or_else(DomainError::not_found)?;
        let class = record
            .fibers
            .fiber(fiber)
            .ok_or_else(DomainError::not_found)?
            .class;
        record.fibers.mark_terminal(fiber, terminal, tick)?;
        record.runtime.release_fiber(class);
        Ok(())
    }

    /// Records one typed current-context association for the courier runtime.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the courier does not exist.
    pub fn record_runtime_context(
        &mut self,
        courier: CourierId,
        context: ContextId,
        tick: u64,
    ) -> Result<(), DomainError> {
        let parent = {
            let record = self
                .find_courier_mut(courier)
                .ok_or_else(DomainError::not_found)?;
            record.runtime.record_context(context, tick);
            record.parent
        };
        self.record_courier_progress(parent, courier, tick)
    }

    /// Records one typed courier-facing runtime summary.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the courier does not exist.
    pub fn record_runtime_summary(
        &mut self,
        courier: CourierId,
        summary: crate::courier::CourierRuntimeSummary,
        tick: u64,
    ) -> Result<(), DomainError> {
        let (parent, derived) = {
            let record = self
                .find_courier_mut(courier)
                .ok_or_else(DomainError::not_found)?;
            let derived = record.obligations.evaluate(tick)?;
            record
                .runtime
                .record_summary(summary.with_responsiveness(derived), tick);
            if let Some(launch) = record.launch.as_mut() {
                launch.responsiveness = derived;
            }
            (record.parent, derived)
        };
        if let Some(parent) = parent {
            match derived {
                CourierResponsiveness::Responsive => {
                    self.record_child_progress(parent, courier, tick)
                }
                CourierResponsiveness::Stale => self.mark_child_stale(parent, courier),
                CourierResponsiveness::NonResponsive => {
                    self.mark_child_non_responsive(parent, courier)
                }
            }
        } else {
            Ok(())
        }
    }

    /// Returns one copy of the courier-owned runtime ledger.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the courier does not exist.
    pub fn runtime_ledger(&self, courier: CourierId) -> Result<CourierRuntimeLedger, DomainError> {
        self.find_courier(courier)
            .map(|record| record.runtime)
            .ok_or_else(DomainError::not_found)
    }

    /// Returns one copy of one supervised fiber record.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the courier does not exist.
    pub fn fiber_record(
        &self,
        courier: CourierId,
        fiber: FiberId,
    ) -> Result<Option<CourierFiberRecord>, DomainError> {
        self.find_courier(courier)
            .map(|record| record.fibers.fiber(fiber).copied())
            .ok_or_else(DomainError::not_found)
    }

    /// Marks one fiber stale under the owning courier ledger.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the courier or fiber does not exist.
    pub fn mark_fiber_stale(
        &mut self,
        courier: CourierId,
        fiber: FiberId,
    ) -> Result<(), DomainError> {
        self.find_courier_mut(courier)
            .ok_or_else(DomainError::not_found)?
            .fibers
            .mark_stale(fiber)
    }

    /// Marks one fiber non-responsive under the owning courier ledger.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the courier or fiber does not exist.
    pub fn mark_fiber_non_responsive(
        &mut self,
        courier: CourierId,
        fiber: FiberId,
    ) -> Result<(), DomainError> {
        self.find_courier_mut(courier)
            .ok_or_else(DomainError::not_found)?
            .fibers
            .mark_non_responsive(fiber)
    }

    /// Upserts one authoritative courier-owned metadata entry on the courier itself.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the courier does not exist or its metadata store is full.
    pub fn upsert_courier_metadata(
        &mut self,
        courier: CourierId,
        key: &'a str,
        value: &'a str,
        tick: u64,
    ) -> Result<(), DomainError> {
        let parent = {
            let record = self
                .find_courier_mut(courier)
                .ok_or_else(DomainError::not_found)?;
            if record
                .app_metadata
                .entry(CourierMetadataSubject::Courier, key)
                .is_none()
                && record.app_metadata.len() >= record.descriptor.plan.app_metadata_capacity
            {
                return Err(DomainError::resource_exhausted());
            }
            record
                .app_metadata
                .upsert(CourierMetadataSubject::Courier, key, value, tick)?;
            record.parent
        };
        self.record_courier_progress(parent, courier, tick)
    }

    /// Upserts one authoritative courier-owned metadata entry on one supervised fiber.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the courier or fiber does not exist, or metadata storage is
    /// exhausted.
    pub fn upsert_fiber_metadata(
        &mut self,
        courier: CourierId,
        fiber: FiberId,
        key: &'a str,
        value: &'a str,
        tick: u64,
    ) -> Result<(), DomainError> {
        let parent = {
            let record = self
                .find_courier_mut(courier)
                .ok_or_else(DomainError::not_found)?;
            let Some(fiber_record) = record.fibers.fiber(fiber).copied() else {
                return Err(DomainError::not_found());
            };
            if record
                .app_metadata
                .entry(CourierMetadataSubject::Fiber(fiber), key)
                .is_none()
                && record.app_metadata.len() >= record.descriptor.plan.app_metadata_capacity
            {
                return Err(DomainError::resource_exhausted());
            }
            record
                .app_metadata
                .upsert(CourierMetadataSubject::Fiber(fiber), key, value, tick)?;
            record.fibers.update_from_snapshot(
                ManagedFiberSnapshot {
                    id: fiber_record.fiber,
                    state: fiber_record.state,
                    started: fiber_record.started,
                    claim_awareness: fiber_record.claim_awareness,
                    claim_context: fiber_record.claim_context,
                },
                tick,
            )?;
            record.parent
        };
        self.record_courier_progress(parent, courier, tick)
    }

    /// Upserts one authoritative parent-owned metadata entry about one child courier.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the parent/child relationship does not exist or the parent's
    /// metadata storage is exhausted.
    pub fn upsert_child_courier_metadata(
        &mut self,
        parent: CourierId,
        child: CourierId,
        key: &'a str,
        value: &'a str,
        tick: u64,
    ) -> Result<(), DomainError> {
        let child_record = self
            .find_courier(child)
            .ok_or_else(DomainError::not_found)?;
        if child_record.parent != Some(parent) {
            return Err(DomainError::permission_denied());
        }
        {
            let record = self
                .find_courier_mut(parent)
                .ok_or_else(DomainError::not_found)?;
            if record
                .app_metadata
                .entry(CourierMetadataSubject::ChildCourier(child), key)
                .is_none()
                && record.app_metadata.len() >= record.descriptor.plan.app_metadata_capacity
            {
                return Err(DomainError::resource_exhausted());
            }
            record.app_metadata.upsert(
                CourierMetadataSubject::ChildCourier(child),
                key,
                value,
                tick,
            )?;
        }
        self.record_child_progress(parent, child, tick)
    }

    /// Upserts one authoritative courier-owned metadata entry on one owned context.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the context does not exist, its owner cannot be found, or
    /// metadata storage is exhausted.
    pub fn upsert_context_metadata(
        &mut self,
        context: ContextId,
        key: &'a str,
        value: &'a str,
        tick: u64,
    ) -> Result<(), DomainError> {
        let owner = self
            .find_context(context)
            .ok_or_else(DomainError::not_found)?
            .support
            .owner;
        let parent = {
            let record = self
                .find_courier_mut(owner)
                .ok_or_else(DomainError::not_found)?;
            if record
                .app_metadata
                .entry(CourierMetadataSubject::Context(context), key)
                .is_none()
                && record.app_metadata.len() >= record.descriptor.plan.app_metadata_capacity
            {
                return Err(DomainError::resource_exhausted());
            }
            record.app_metadata.upsert(
                CourierMetadataSubject::Context(context),
                key,
                value,
                tick,
            )?;
            record.parent
        };
        self.record_courier_progress(parent, owner, tick)
    }

    /// Upserts one authoritative courier-owned app metadata entry on the async lane.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the courier does not exist or its app-metadata budget is
    /// exhausted.
    pub fn upsert_async_metadata(
        &mut self,
        courier: CourierId,
        key: &'a str,
        value: &'a str,
        tick: u64,
    ) -> Result<(), DomainError> {
        let parent = {
            let record = self
                .find_courier_mut(courier)
                .ok_or_else(DomainError::not_found)?;
            if record
                .app_metadata
                .entry(CourierMetadataSubject::AsyncLane, key)
                .is_none()
                && record.app_metadata.len() >= record.descriptor.plan.app_metadata_capacity
            {
                return Err(DomainError::resource_exhausted());
            }
            record
                .app_metadata
                .upsert(CourierMetadataSubject::AsyncLane, key, value, tick)?;
            record.parent
        };
        self.record_courier_progress(parent, courier, tick)
    }

    /// Removes one authoritative courier-owned metadata entry.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the courier does not exist or the entry is absent.
    pub fn remove_metadata(
        &mut self,
        courier: CourierId,
        subject: CourierMetadataSubject,
        key: &str,
    ) -> Result<(), DomainError> {
        self.find_courier_mut(courier)
            .ok_or_else(DomainError::not_found)?
            .app_metadata
            .remove(subject, key)
    }

    /// Removes one authoritative parent-owned child-courier metadata entry.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the parent/child relationship does not exist or the entry is
    /// absent.
    pub fn remove_child_courier_metadata(
        &mut self,
        parent: CourierId,
        child: CourierId,
        key: &str,
    ) -> Result<(), DomainError> {
        let child_record = self
            .find_courier(child)
            .ok_or_else(DomainError::not_found)?;
        if child_record.parent != Some(parent) {
            return Err(DomainError::permission_denied());
        }
        self.find_courier_mut(parent)
            .ok_or_else(DomainError::not_found)?
            .app_metadata
            .remove(CourierMetadataSubject::ChildCourier(child), key)
    }

    /// Registers one externally visible courier obligation.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the courier does not exist, its obligation plan is exhausted,
    /// or the aging thresholds are invalid.
    pub fn register_obligation(
        &mut self,
        courier: CourierId,
        subject: CourierMetadataSubject,
        label: &'a str,
        stale_after_ticks: u64,
        non_responsive_after_ticks: u64,
        tick: u64,
    ) -> Result<CourierObligationId, DomainError> {
        self.register_obligation_spec(
            courier,
            CourierObligationSpec::custom(
                subject,
                label,
                stale_after_ticks,
                non_responsive_after_ticks,
            ),
            tick,
        )
    }

    /// Registers one typed externally visible courier obligation.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the courier does not exist, its obligation plan is exhausted,
    /// or the supplied aging policy is invalid.
    pub fn register_obligation_spec(
        &mut self,
        courier: CourierId,
        spec: CourierObligationSpec<'a>,
        tick: u64,
    ) -> Result<CourierObligationId, DomainError> {
        let record = self
            .find_courier_mut(courier)
            .ok_or_else(DomainError::not_found)?;
        if record.obligations.len() >= record.descriptor.plan.obligation_capacity {
            return Err(DomainError::resource_exhausted());
        }
        record.obligations.register(spec, tick)
    }

    /// Registers one channel-shaped externally visible courier obligation.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the courier does not exist, its obligation budget is
    /// exhausted, or the supplied aging policy is invalid.
    pub fn register_channel_obligation(
        &mut self,
        courier: CourierId,
        subject: CourierMetadataSubject,
        channel: &'a str,
        stale_after_ticks: u64,
        non_responsive_after_ticks: u64,
        tick: u64,
    ) -> Result<CourierObligationId, DomainError> {
        self.register_obligation_spec(
            courier,
            CourierObligationSpec::new(
                subject,
                CourierObligationBinding::Channel(channel),
                stale_after_ticks,
                non_responsive_after_ticks,
            ),
            tick,
        )
    }

    /// Registers one service-shaped externally visible courier obligation.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the courier does not exist, its obligation budget is
    /// exhausted, or the supplied aging policy is invalid.
    pub fn register_service_obligation(
        &mut self,
        courier: CourierId,
        subject: CourierMetadataSubject,
        service: &'a str,
        stale_after_ticks: u64,
        non_responsive_after_ticks: u64,
        tick: u64,
    ) -> Result<CourierObligationId, DomainError> {
        self.register_obligation_spec(
            courier,
            CourierObligationSpec::new(
                subject,
                CourierObligationBinding::Service(service),
                stale_after_ticks,
                non_responsive_after_ticks,
            ),
            tick,
        )
    }

    /// Registers one input-shaped externally visible courier obligation.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the courier does not exist, its obligation budget is
    /// exhausted, or the supplied aging policy is invalid.
    pub fn register_input_obligation(
        &mut self,
        courier: CourierId,
        subject: CourierMetadataSubject,
        input: &'a str,
        stale_after_ticks: u64,
        non_responsive_after_ticks: u64,
        tick: u64,
    ) -> Result<CourierObligationId, DomainError> {
        self.register_obligation_spec(
            courier,
            CourierObligationSpec::new(
                subject,
                CourierObligationBinding::Input(input),
                stale_after_ticks,
                non_responsive_after_ticks,
            ),
            tick,
        )
    }

    /// Records one externally visible courier-obligation progress event.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the courier or obligation does not exist.
    pub fn record_obligation_progress(
        &mut self,
        courier: CourierId,
        obligation: CourierObligationId,
        tick: u64,
    ) -> Result<(), DomainError> {
        let parent = {
            let record = self
                .find_courier_mut(courier)
                .ok_or_else(DomainError::not_found)?;
            record.obligations.record_progress(obligation, tick)?;
            if let Some(launch) = record.launch.as_mut() {
                launch.responsiveness = CourierResponsiveness::Responsive;
                launch.last_progress_tick = tick;
            }
            record.parent
        };
        self.record_courier_progress(parent, courier, tick)
    }

    /// Removes one externally visible courier obligation.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the courier or obligation does not exist.
    pub fn remove_obligation(
        &mut self,
        courier: CourierId,
        obligation: CourierObligationId,
    ) -> Result<(), DomainError> {
        self.find_courier_mut(courier)
            .ok_or_else(DomainError::not_found)?
            .obligations
            .remove(obligation)
    }

    /// Evaluates the courier's active obligations and derives one honest responsiveness state.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the courier does not exist.
    pub fn evaluate_courier_responsiveness(
        &mut self,
        courier: CourierId,
        now_tick: u64,
    ) -> Result<CourierResponsiveness, DomainError> {
        let (parent, responsiveness) = {
            let record = self
                .find_courier_mut(courier)
                .ok_or_else(DomainError::not_found)?;
            let responsiveness = record.obligations.evaluate(now_tick)?;
            if let Some(launch) = record.launch.as_mut() {
                launch.responsiveness = responsiveness;
            }
            (record.parent, responsiveness)
        };
        if let Some(parent) = parent {
            match responsiveness {
                CourierResponsiveness::Responsive => {}
                CourierResponsiveness::Stale => self.mark_child_stale(parent, courier)?,
                CourierResponsiveness::NonResponsive => {
                    self.mark_child_non_responsive(parent, courier)?
                }
            }
        }
        Ok(responsiveness)
    }

    /// Returns one honest responsiveness classification for the supplied courier.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the courier does not exist.
    pub fn courier_responsiveness(
        &mut self,
        courier: CourierId,
        now_tick: u64,
    ) -> Result<CourierResponsiveness, DomainError> {
        self.evaluate_courier_responsiveness(courier, now_tick)
    }

    /// Registers one owned context under the supplied owning courier.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the owner does not exist, the ID already exists, or storage is
    /// exhausted.
    pub fn register_context(
        &mut self,
        owner: CourierId,
        descriptor: ContextDescriptor<'a>,
    ) -> Result<(), DomainError> {
        if self.find_courier(owner).is_none() {
            return Err(DomainError::not_found());
        }
        if self.find_context(descriptor.id).is_some() {
            return Err(DomainError::state_conflict());
        }
        let Some(slot) = self.contexts.iter_mut().find(|slot| slot.is_none()) else {
            return Err(DomainError::resource_exhausted());
        };
        *slot = Some(ContextRecord {
            descriptor,
            support: ContextSupport {
                caps: descriptor.caps,
                implementation: ContextImplementationKind::Native,
                domain: self.domain.descriptor.id,
                owner,
                kind: descriptor.kind,
                projection: ContextProjectionKind::Owned,
                claim_context: descriptor.claim_context,
            },
        });
        Ok(())
    }

    /// Grants one projected context into the target courier.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the courier or context does not exist, the context is already
    /// visible, or the visibility table is exhausted.
    pub fn grant_context(
        &mut self,
        courier: CourierId,
        context: ContextId,
        projection: ContextProjectionKind,
    ) -> Result<(), DomainError> {
        if self.find_context(context).is_none() {
            return Err(DomainError::not_found());
        }
        let courier_record = self
            .find_courier_mut(courier)
            .ok_or_else(DomainError::not_found)?;
        if courier_record
            .visible
            .iter()
            .any(|slot| slot.is_some_and(|grant| grant.context == context))
        {
            return Err(DomainError::state_conflict());
        }
        let Some(slot) = courier_record
            .visible
            .iter_mut()
            .find(|slot| slot.is_none())
        else {
            return Err(DomainError::resource_exhausted());
        };
        *slot = Some(ContextGrant {
            context,
            projection,
        });
        Ok(())
    }

    /// Returns a handle to one registered courier.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the courier does not exist.
    pub fn courier(
        &self,
        courier: CourierId,
    ) -> Result<
        CourierHandle<
            '_,
            'a,
            MAX_COURIERS,
            MAX_CONTEXTS,
            MAX_VISIBLE,
            MAX_CHILDREN,
            MAX_FIBERS,
            MAX_METADATA,
        >,
        DomainError,
    > {
        let Some(index) = self.index_of_courier(courier) else {
            return Err(DomainError::not_found());
        };
        Ok(CourierHandle {
            registry: self,
            index,
        })
    }

    /// Returns one registered context handle.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the context does not exist.
    pub fn context(
        &self,
        context: ContextId,
    ) -> Result<
        ContextHandle<
            '_,
            'a,
            MAX_COURIERS,
            MAX_CONTEXTS,
            MAX_VISIBLE,
            MAX_CHILDREN,
            MAX_FIBERS,
            MAX_METADATA,
        >,
        DomainError,
    > {
        let Some(index) = self.index_of_context(context) else {
            return Err(DomainError::not_found());
        };
        Ok(ContextHandle {
            registry: self,
            index,
            projection: ContextProjectionKind::Owned,
        })
    }

    /// Returns one generic runtime sink view over this registry.
    #[must_use]
    pub fn runtime_sink(&mut self) -> CourierRuntimeSink {
        CourierRuntimeSink::new(
            self as *mut Self as *mut (),
            runtime_sink_vtable::<
                MAX_COURIERS,
                MAX_CONTEXTS,
                MAX_VISIBLE,
                MAX_CHILDREN,
                MAX_FIBERS,
                MAX_METADATA,
            >(),
        )
    }

    /// Returns one generic launch-control view over this registry.
    #[must_use]
    pub fn launch_control(&mut self) -> CourierLaunchControl<'a> {
        CourierLaunchControl::new(
            self as *mut Self as *mut (),
            launch_control_vtable::<
                'a,
                MAX_COURIERS,
                MAX_CONTEXTS,
                MAX_VISIBLE,
                MAX_CHILDREN,
                MAX_FIBERS,
                MAX_METADATA,
            >(),
        )
    }

    fn insert_courier(
        &mut self,
        record: CourierRecord<'a, MAX_VISIBLE, MAX_CHILDREN, MAX_FIBERS, MAX_METADATA>,
    ) -> Result<(), DomainError> {
        if self.find_courier(record.descriptor.id).is_some() {
            return Err(DomainError::state_conflict());
        }
        let Some(slot) = self.couriers.iter_mut().find(|slot| slot.is_none()) else {
            return Err(DomainError::resource_exhausted());
        };
        *slot = Some(record);
        Ok(())
    }

    fn validate_courier_descriptor(
        &self,
        descriptor: CourierDescriptor<'a>,
    ) -> Result<(), DomainError> {
        if !descriptor.plan.is_valid() {
            return Err(DomainError::invalid());
        }
        if !descriptor.plan.fits_within(MAX_CHILDREN, MAX_FIBERS) {
            return Err(DomainError::resource_exhausted());
        }
        if descriptor.plan.app_metadata_capacity > MAX_METADATA
            || descriptor.plan.obligation_capacity > MAX_METADATA
        {
            return Err(DomainError::resource_exhausted());
        }
        Ok(())
    }

    fn mark_child_responsiveness(
        &mut self,
        parent: CourierId,
        child: CourierId,
        responsiveness: CourierResponsiveness,
    ) -> Result<(), DomainError> {
        let Some(child_index) = self.index_of_courier(child) else {
            return Err(DomainError::not_found());
        };
        let Some(parent_index) = self.index_of_courier(parent) else {
            return Err(DomainError::not_found());
        };
        if self.couriers[child_index]
            .as_ref()
            .expect("courier index should point at a live child courier")
            .parent
            != Some(parent)
        {
            return Err(DomainError::permission_denied());
        }
        {
            let child_record = self.couriers[child_index]
                .as_mut()
                .expect("courier index should point at a live child courier");
            let Some(launch) = child_record.launch.as_mut() else {
                return Err(DomainError::state_conflict());
            };
            launch.responsiveness = responsiveness;
        }
        let parent_record = self.couriers[parent_index]
            .as_mut()
            .expect("courier index should point at a live parent courier");
        // Child progress should flow from real observed work, not fake periodic pulses.
        // `Responsive` recovery therefore belongs on the explicit progress-recording path only.
        match responsiveness {
            CourierResponsiveness::Responsive => Err(DomainError::invalid()),
            CourierResponsiveness::Stale => parent_record.children.mark_stale(child),
            CourierResponsiveness::NonResponsive => {
                parent_record.children.mark_non_responsive(child)
            }
        }
    }

    fn find_courier(
        &self,
        courier: CourierId,
    ) -> Option<&CourierRecord<'a, MAX_VISIBLE, MAX_CHILDREN, MAX_FIBERS, MAX_METADATA>> {
        self.couriers
            .iter()
            .flatten()
            .find(|record| record.descriptor.id == courier)
    }

    fn find_courier_mut(
        &mut self,
        courier: CourierId,
    ) -> Option<&mut CourierRecord<'a, MAX_VISIBLE, MAX_CHILDREN, MAX_FIBERS, MAX_METADATA>> {
        self.couriers
            .iter_mut()
            .flatten()
            .find(|record| record.descriptor.id == courier)
    }

    fn index_of_courier(&self, courier: CourierId) -> Option<usize> {
        self.couriers
            .iter()
            .position(|slot| slot.is_some_and(|record| record.descriptor.id == courier))
    }

    fn find_context(&self, context: ContextId) -> Option<&ContextRecord<'a>> {
        self.contexts
            .iter()
            .flatten()
            .find(|record| record.descriptor.id == context)
    }

    fn index_of_context(&self, context: ContextId) -> Option<usize> {
        self.contexts
            .iter()
            .position(|slot| slot.is_some_and(|record| record.descriptor.id == context))
    }

    fn record_courier_progress(
        &mut self,
        parent: Option<CourierId>,
        courier: CourierId,
        tick: u64,
    ) -> Result<(), DomainError> {
        if let Some(parent) = parent {
            self.record_child_progress(parent, courier, tick)
        } else {
            Ok(())
        }
    }
}

impl<
    'a,
    const MAX_COURIERS: usize,
    const MAX_CONTEXTS: usize,
    const MAX_VISIBLE: usize,
    const MAX_CHILDREN: usize,
    const MAX_FIBERS: usize,
    const MAX_METADATA: usize,
> DomainBaseContract
    for DomainRegistry<
        'a,
        MAX_COURIERS,
        MAX_CONTEXTS,
        MAX_VISIBLE,
        MAX_CHILDREN,
        MAX_FIBERS,
        MAX_METADATA,
    >
{
    fn domain_id(&self) -> DomainId {
        self.domain.descriptor.id
    }

    fn name(&self) -> &str {
        self.domain.descriptor.name
    }

    fn domain_support(&self) -> DomainSupport {
        self.domain.support
    }
}

mod handles;
pub use handles::*;

mod sink;
use sink::{
    launch_control_vtable,
    runtime_sink_vtable,
};
