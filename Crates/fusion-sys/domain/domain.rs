//! fusion-sys domain registry and native domain/courier/context demonstration.

pub use fusion_pal::sys::domain::*;

use crate::claims::{ClaimAwareness, ClaimContextId, LocalAdmissionSeal, PrincipalId};
use crate::context::{
    ContextBase,
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
    CourierBase,
    CourierCaps,
    CourierFiberClass,
    CourierFiberLedger,
    CourierFiberRecord,
    CourierImplementationKind,
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
    CourierVisibilityControl,
    FiberMetadataAttachment,
    FiberTerminalStatus,
};
use crate::fiber::{FiberId, ManagedFiberSnapshot};

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
> DomainBase
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

/// Borrowed view of one courier inside the fixed-capacity domain registry.
#[derive(Debug, Clone, Copy)]
pub struct CourierHandle<
    'registry,
    'a,
    const MAX_COURIERS: usize,
    const MAX_CONTEXTS: usize,
    const MAX_VISIBLE: usize,
    const MAX_CHILDREN: usize,
    const MAX_FIBERS: usize,
    const MAX_METADATA: usize,
> {
    registry: &'registry DomainRegistry<
        'a,
        MAX_COURIERS,
        MAX_CONTEXTS,
        MAX_VISIBLE,
        MAX_CHILDREN,
        MAX_FIBERS,
        MAX_METADATA,
    >,
    index: usize,
}

impl<
    'registry,
    'a,
    const MAX_COURIERS: usize,
    const MAX_CONTEXTS: usize,
    const MAX_VISIBLE: usize,
    const MAX_CHILDREN: usize,
    const MAX_FIBERS: usize,
    const MAX_METADATA: usize,
>
    CourierHandle<
        'registry,
        'a,
        MAX_COURIERS,
        MAX_CONTEXTS,
        MAX_VISIBLE,
        MAX_CHILDREN,
        MAX_FIBERS,
        MAX_METADATA,
    >
{
    #[must_use]
    pub fn visible_contexts(
        self,
    ) -> VisibleContexts<
        'registry,
        'a,
        MAX_COURIERS,
        MAX_CONTEXTS,
        MAX_VISIBLE,
        MAX_CHILDREN,
        MAX_FIBERS,
        MAX_METADATA,
    > {
        VisibleContexts {
            courier: self,
            next_visible: 0,
            next_context: 0,
        }
    }

    #[must_use]
    pub fn plan(self) -> CourierPlan {
        self.record().descriptor.plan
    }

    #[must_use]
    pub fn parent_courier(self) -> Option<CourierId> {
        self.record().parent
    }

    #[must_use]
    pub fn launch_record(self) -> Option<&'registry ChildCourierLaunchRecord<'a>> {
        let record: &'registry CourierRecord<
            'a,
            MAX_VISIBLE,
            MAX_CHILDREN,
            MAX_FIBERS,
            MAX_METADATA,
        > = self.registry.couriers[self.index]
            .as_ref()
            .expect("courier handle should only point at live couriers");
        record.launch.as_ref()
    }

    #[must_use]
    pub fn metadata_entries(self) -> impl Iterator<Item = &'registry CourierMetadataEntry<'a>> {
        let record: &'registry CourierRecord<
            'a,
            MAX_VISIBLE,
            MAX_CHILDREN,
            MAX_FIBERS,
            MAX_METADATA,
        > = self.registry.couriers[self.index]
            .as_ref()
            .expect("courier handle should only point at live couriers");
        record.app_metadata.iter()
    }

    #[must_use]
    pub fn courier_metadata(self) -> impl Iterator<Item = &'registry CourierMetadataEntry<'a>> {
        self.metadata_entries_for(CourierMetadataSubject::Courier)
    }

    #[must_use]
    pub fn fiber_metadata(
        self,
        fiber: FiberId,
    ) -> impl Iterator<Item = &'registry CourierMetadataEntry<'a>> {
        self.metadata_entries_for(CourierMetadataSubject::Fiber(fiber))
    }

    #[must_use]
    pub fn child_courier_metadata(
        self,
        child: CourierId,
    ) -> impl Iterator<Item = &'registry CourierMetadataEntry<'a>> {
        self.metadata_entries_for(CourierMetadataSubject::ChildCourier(child))
    }

    #[must_use]
    pub fn context_metadata(
        self,
        context: ContextId,
    ) -> impl Iterator<Item = &'registry CourierMetadataEntry<'a>> {
        self.metadata_entries_for(CourierMetadataSubject::Context(context))
    }

    #[must_use]
    pub fn async_metadata(self) -> impl Iterator<Item = &'registry CourierMetadataEntry<'a>> {
        self.metadata_entries_for(CourierMetadataSubject::AsyncLane)
    }

    #[must_use]
    pub fn metadata_entries_for(
        self,
        subject: CourierMetadataSubject,
    ) -> impl Iterator<Item = &'registry CourierMetadataEntry<'a>> {
        let record: &'registry CourierRecord<
            'a,
            MAX_VISIBLE,
            MAX_CHILDREN,
            MAX_FIBERS,
            MAX_METADATA,
        > = self.registry.couriers[self.index]
            .as_ref()
            .expect("courier handle should only point at live couriers");
        record.app_metadata.entries(subject)
    }

    #[must_use]
    pub fn metadata_entry_for(
        self,
        subject: CourierMetadataSubject,
        key: &str,
    ) -> Option<&'registry CourierMetadataEntry<'a>> {
        let record: &'registry CourierRecord<
            'a,
            MAX_VISIBLE,
            MAX_CHILDREN,
            MAX_FIBERS,
            MAX_METADATA,
        > = self.registry.couriers[self.index]
            .as_ref()
            .expect("courier handle should only point at live couriers");
        record.app_metadata.entry(subject, key)
    }

    #[must_use]
    pub fn courier_metadata_entry(self, key: &str) -> Option<&'registry CourierMetadataEntry<'a>> {
        self.metadata_entry_for(CourierMetadataSubject::Courier, key)
    }

    #[must_use]
    pub fn fiber_metadata_entry(
        self,
        fiber: FiberId,
        key: &str,
    ) -> Option<&'registry CourierMetadataEntry<'a>> {
        self.metadata_entry_for(CourierMetadataSubject::Fiber(fiber), key)
    }

    #[must_use]
    pub fn child_courier_metadata_entry(
        self,
        child: CourierId,
        key: &str,
    ) -> Option<&'registry CourierMetadataEntry<'a>> {
        self.metadata_entry_for(CourierMetadataSubject::ChildCourier(child), key)
    }

    #[must_use]
    pub fn context_metadata_entry(
        self,
        context: ContextId,
        key: &str,
    ) -> Option<&'registry CourierMetadataEntry<'a>> {
        self.metadata_entry_for(CourierMetadataSubject::Context(context), key)
    }

    #[must_use]
    pub fn async_metadata_entry(self, key: &str) -> Option<&'registry CourierMetadataEntry<'a>> {
        self.metadata_entry_for(CourierMetadataSubject::AsyncLane, key)
    }

    #[must_use]
    pub fn child_courier_count(self) -> usize {
        self.record().children.len()
    }

    #[must_use]
    pub fn obligation_count(self) -> usize {
        self.record().obligations.len()
    }

    pub fn obligations(self) -> impl Iterator<Item = &'registry CourierObligationRecord<'a>> {
        let record: &'registry CourierRecord<
            'a,
            MAX_VISIBLE,
            MAX_CHILDREN,
            MAX_FIBERS,
            MAX_METADATA,
        > = self.registry.couriers[self.index]
            .as_ref()
            .expect("courier handle should only point at live couriers");
        record.obligations.iter()
    }

    #[must_use]
    pub fn obligation(
        self,
        obligation: CourierObligationId,
    ) -> Option<&'registry CourierObligationRecord<'a>> {
        let record: &'registry CourierRecord<
            'a,
            MAX_VISIBLE,
            MAX_CHILDREN,
            MAX_FIBERS,
            MAX_METADATA,
        > = self.registry.couriers[self.index]
            .as_ref()
            .expect("courier handle should only point at live couriers");
        record.obligations.obligation(obligation)
    }

    pub fn child_couriers(self) -> impl Iterator<Item = &'registry ChildCourierLaunchRecord<'a>> {
        let record: &'registry CourierRecord<
            'a,
            MAX_VISIBLE,
            MAX_CHILDREN,
            MAX_FIBERS,
            MAX_METADATA,
        > = self.registry.couriers[self.index]
            .as_ref()
            .expect("courier handle should only point at live couriers");
        record.children.iter()
    }

    #[must_use]
    pub fn fiber_count(self) -> usize {
        self.record().fibers.len()
    }

    pub fn fibers(self) -> impl Iterator<Item = &'registry CourierFiberRecord> {
        let record: &'registry CourierRecord<
            'a,
            MAX_VISIBLE,
            MAX_CHILDREN,
            MAX_FIBERS,
            MAX_METADATA,
        > = self.registry.couriers[self.index]
            .as_ref()
            .expect("courier handle should only point at live couriers");
        record.fibers.iter()
    }

    #[must_use]
    pub fn runtime_ledger(self) -> crate::courier::CourierRuntimeLedger {
        self.record().runtime
    }

    #[must_use]
    pub fn fiber(self, fiber: FiberId) -> Option<&'registry CourierFiberRecord> {
        let record: &'registry CourierRecord<
            'a,
            MAX_VISIBLE,
            MAX_CHILDREN,
            MAX_FIBERS,
            MAX_METADATA,
        > = self.registry.couriers[self.index]
            .as_ref()
            .expect("courier handle should only point at live couriers");
        record.fibers.fiber(fiber)
    }

    #[must_use]
    pub fn metadata(self) -> CourierMetadata<'registry> {
        CourierMetadata {
            id: self.record().descriptor.id,
            name: self.record().descriptor.name,
            support: self.record().support,
        }
    }

    fn record(&self) -> &CourierRecord<'a, MAX_VISIBLE, MAX_CHILDREN, MAX_FIBERS, MAX_METADATA> {
        self.registry.couriers[self.index]
            .as_ref()
            .expect("courier handle should only point at live couriers")
    }
}

impl<
    'registry,
    'a,
    const MAX_COURIERS: usize,
    const MAX_CONTEXTS: usize,
    const MAX_VISIBLE: usize,
    const MAX_CHILDREN: usize,
    const MAX_FIBERS: usize,
    const MAX_METADATA: usize,
> CourierBase
    for CourierHandle<
        'registry,
        'a,
        MAX_COURIERS,
        MAX_CONTEXTS,
        MAX_VISIBLE,
        MAX_CHILDREN,
        MAX_FIBERS,
        MAX_METADATA,
    >
{
    fn courier_id(&self) -> CourierId {
        self.record().descriptor.id
    }

    fn name(&self) -> &str {
        self.record().descriptor.name
    }

    fn courier_support(&self) -> CourierSupport {
        self.record().support
    }
}

impl<
    'registry,
    'a,
    const MAX_COURIERS: usize,
    const MAX_CONTEXTS: usize,
    const MAX_VISIBLE: usize,
    const MAX_CHILDREN: usize,
    const MAX_FIBERS: usize,
    const MAX_METADATA: usize,
> CourierVisibilityControl
    for CourierHandle<
        'registry,
        'a,
        MAX_COURIERS,
        MAX_CONTEXTS,
        MAX_VISIBLE,
        MAX_CHILDREN,
        MAX_FIBERS,
        MAX_METADATA,
    >
{
    fn visible_context_count(&self) -> usize {
        match self.record().support.visibility {
            CourierVisibility::Full => self.registry.contexts.iter().flatten().count(),
            CourierVisibility::Scoped => self.record().visible.iter().flatten().count(),
        }
    }

    fn can_observe_context(&self, context: ContextId) -> bool {
        match self.record().support.visibility {
            CourierVisibility::Full => self.registry.find_context(context).is_some(),
            CourierVisibility::Scoped => self
                .record()
                .visible
                .iter()
                .any(|slot| slot.is_some_and(|grant| grant.context == context)),
        }
    }
}

/// Borrowed view of one context inside the fixed-capacity domain registry.
#[derive(Debug, Clone, Copy)]
pub struct ContextHandle<
    'registry,
    'a,
    const MAX_COURIERS: usize,
    const MAX_CONTEXTS: usize,
    const MAX_VISIBLE: usize,
    const MAX_CHILDREN: usize,
    const MAX_FIBERS: usize,
    const MAX_METADATA: usize,
> {
    registry: &'registry DomainRegistry<
        'a,
        MAX_COURIERS,
        MAX_CONTEXTS,
        MAX_VISIBLE,
        MAX_CHILDREN,
        MAX_FIBERS,
        MAX_METADATA,
    >,
    index: usize,
    projection: ContextProjectionKind,
}

impl<
    'registry,
    'a,
    const MAX_COURIERS: usize,
    const MAX_CONTEXTS: usize,
    const MAX_VISIBLE: usize,
    const MAX_CHILDREN: usize,
    const MAX_FIBERS: usize,
    const MAX_METADATA: usize,
>
    ContextHandle<
        'registry,
        'a,
        MAX_COURIERS,
        MAX_CONTEXTS,
        MAX_VISIBLE,
        MAX_CHILDREN,
        MAX_FIBERS,
        MAX_METADATA,
    >
{
    fn record(&self) -> &ContextRecord<'a> {
        self.registry.contexts[self.index]
            .as_ref()
            .expect("context handle should only point at live contexts")
    }
}

impl<
    'registry,
    'a,
    const MAX_COURIERS: usize,
    const MAX_CONTEXTS: usize,
    const MAX_VISIBLE: usize,
    const MAX_CHILDREN: usize,
    const MAX_FIBERS: usize,
    const MAX_METADATA: usize,
> ContextBase
    for ContextHandle<
        'registry,
        'a,
        MAX_COURIERS,
        MAX_CONTEXTS,
        MAX_VISIBLE,
        MAX_CHILDREN,
        MAX_FIBERS,
        MAX_METADATA,
    >
{
    fn context_id(&self) -> ContextId {
        self.record().descriptor.id
    }

    fn name(&self) -> &str {
        self.record().descriptor.name
    }

    fn context_support(&self) -> ContextSupport {
        let mut support = self.record().support;
        support.projection = self.projection;
        support
    }
}

/// Iterator over only the contexts visible to one courier.
pub struct VisibleContexts<
    'registry,
    'a,
    const MAX_COURIERS: usize,
    const MAX_CONTEXTS: usize,
    const MAX_VISIBLE: usize,
    const MAX_CHILDREN: usize,
    const MAX_FIBERS: usize,
    const MAX_METADATA: usize,
> {
    courier: CourierHandle<
        'registry,
        'a,
        MAX_COURIERS,
        MAX_CONTEXTS,
        MAX_VISIBLE,
        MAX_CHILDREN,
        MAX_FIBERS,
        MAX_METADATA,
    >,
    next_visible: usize,
    next_context: usize,
}

impl<
    'registry,
    'a,
    const MAX_COURIERS: usize,
    const MAX_CONTEXTS: usize,
    const MAX_VISIBLE: usize,
    const MAX_CHILDREN: usize,
    const MAX_FIBERS: usize,
    const MAX_METADATA: usize,
> Iterator
    for VisibleContexts<
        'registry,
        'a,
        MAX_COURIERS,
        MAX_CONTEXTS,
        MAX_VISIBLE,
        MAX_CHILDREN,
        MAX_FIBERS,
        MAX_METADATA,
    >
{
    type Item = ContextHandle<
        'registry,
        'a,
        MAX_COURIERS,
        MAX_CONTEXTS,
        MAX_VISIBLE,
        MAX_CHILDREN,
        MAX_FIBERS,
        MAX_METADATA,
    >;

    fn next(&mut self) -> Option<Self::Item> {
        match self.courier.record().support.visibility {
            CourierVisibility::Full => {
                while self.next_context < self.courier.registry.contexts.len() {
                    let index = self.next_context;
                    self.next_context += 1;
                    if self.courier.registry.contexts[index].is_some() {
                        return Some(ContextHandle {
                            registry: self.courier.registry,
                            index,
                            projection: ContextProjectionKind::Owned,
                        });
                    }
                }
                None
            }
            CourierVisibility::Scoped => {
                while self.next_visible < self.courier.record().visible.len() {
                    let slot_index = self.next_visible;
                    self.next_visible += 1;
                    let Some(grant) = self.courier.record().visible[slot_index] else {
                        continue;
                    };
                    let Some(index) = self.courier.registry.index_of_context(grant.context) else {
                        continue;
                    };
                    return Some(ContextHandle {
                        registry: self.courier.registry,
                        index,
                        projection: grant.projection,
                    });
                }
                None
            }
        }
    }
}

fn runtime_sink_vtable<
    'a,
    const MAX_COURIERS: usize,
    const MAX_CONTEXTS: usize,
    const MAX_VISIBLE: usize,
    const MAX_CHILDREN: usize,
    const MAX_FIBERS: usize,
    const MAX_METADATA: usize,
>() -> &'static CourierRuntimeSinkVTable {
    &CourierRuntimeSinkVTable {
        record_context: runtime_sink_record_context::<
            MAX_COURIERS,
            MAX_CONTEXTS,
            MAX_VISIBLE,
            MAX_CHILDREN,
            MAX_FIBERS,
            MAX_METADATA,
        >,
        register_fiber: runtime_sink_register_fiber::<
            MAX_COURIERS,
            MAX_CONTEXTS,
            MAX_VISIBLE,
            MAX_CHILDREN,
            MAX_FIBERS,
            MAX_METADATA,
        >,
        update_fiber: runtime_sink_update_fiber::<
            MAX_COURIERS,
            MAX_CONTEXTS,
            MAX_VISIBLE,
            MAX_CHILDREN,
            MAX_FIBERS,
            MAX_METADATA,
        >,
        mark_fiber_terminal: runtime_sink_mark_fiber_terminal::<
            MAX_COURIERS,
            MAX_CONTEXTS,
            MAX_VISIBLE,
            MAX_CHILDREN,
            MAX_FIBERS,
            MAX_METADATA,
        >,
        record_runtime_summary: runtime_sink_record_runtime_summary::<
            MAX_COURIERS,
            MAX_CONTEXTS,
            MAX_VISIBLE,
            MAX_CHILDREN,
            MAX_FIBERS,
            MAX_METADATA,
        >,
    }
}

unsafe fn runtime_sink_record_context<
    'a,
    const MAX_COURIERS: usize,
    const MAX_CONTEXTS: usize,
    const MAX_VISIBLE: usize,
    const MAX_CHILDREN: usize,
    const MAX_FIBERS: usize,
    const MAX_METADATA: usize,
>(
    context: *mut (),
    courier: CourierId,
    runtime_context: ContextId,
    tick: u64,
) -> Result<(), CourierRuntimeSinkError> {
    let registry = unsafe {
        &mut *context.cast::<DomainRegistry<
            'a,
            MAX_COURIERS,
            MAX_CONTEXTS,
            MAX_VISIBLE,
            MAX_CHILDREN,
            MAX_FIBERS,
            MAX_METADATA,
        >>()
    };
    registry
        .record_runtime_context(courier, runtime_context, tick)
        .map_err(Into::into)
}

unsafe fn runtime_sink_register_fiber<
    'a,
    const MAX_COURIERS: usize,
    const MAX_CONTEXTS: usize,
    const MAX_VISIBLE: usize,
    const MAX_CHILDREN: usize,
    const MAX_FIBERS: usize,
    const MAX_METADATA: usize,
>(
    context: *mut (),
    courier: CourierId,
    snapshot: ManagedFiberSnapshot,
    generation: u64,
    class: CourierFiberClass,
    is_root: bool,
    metadata_attachment: Option<FiberMetadataAttachment>,
    tick: u64,
) -> Result<(), CourierRuntimeSinkError> {
    let registry = unsafe {
        &mut *context.cast::<DomainRegistry<
            'a,
            MAX_COURIERS,
            MAX_CONTEXTS,
            MAX_VISIBLE,
            MAX_CHILDREN,
            MAX_FIBERS,
            MAX_METADATA,
        >>()
    };
    registry
        .register_fiber_with_class(
            courier,
            snapshot,
            generation,
            class,
            is_root,
            metadata_attachment,
            tick,
        )
        .map_err(Into::into)
}

unsafe fn runtime_sink_update_fiber<
    'a,
    const MAX_COURIERS: usize,
    const MAX_CONTEXTS: usize,
    const MAX_VISIBLE: usize,
    const MAX_CHILDREN: usize,
    const MAX_FIBERS: usize,
    const MAX_METADATA: usize,
>(
    context: *mut (),
    courier: CourierId,
    snapshot: ManagedFiberSnapshot,
    tick: u64,
) -> Result<(), CourierRuntimeSinkError> {
    let registry = unsafe {
        &mut *context.cast::<DomainRegistry<
            'a,
            MAX_COURIERS,
            MAX_CONTEXTS,
            MAX_VISIBLE,
            MAX_CHILDREN,
            MAX_FIBERS,
            MAX_METADATA,
        >>()
    };
    registry
        .update_fiber_snapshot(courier, snapshot, tick)
        .map_err(Into::into)
}

unsafe fn runtime_sink_mark_fiber_terminal<
    'a,
    const MAX_COURIERS: usize,
    const MAX_CONTEXTS: usize,
    const MAX_VISIBLE: usize,
    const MAX_CHILDREN: usize,
    const MAX_FIBERS: usize,
    const MAX_METADATA: usize,
>(
    context: *mut (),
    courier: CourierId,
    fiber: FiberId,
    terminal: FiberTerminalStatus,
    tick: u64,
) -> Result<(), CourierRuntimeSinkError> {
    let registry = unsafe {
        &mut *context.cast::<DomainRegistry<
            'a,
            MAX_COURIERS,
            MAX_CONTEXTS,
            MAX_VISIBLE,
            MAX_CHILDREN,
            MAX_FIBERS,
            MAX_METADATA,
        >>()
    };
    registry
        .mark_fiber_terminal(courier, fiber, terminal, tick)
        .map_err(Into::into)
}

unsafe fn runtime_sink_record_runtime_summary<
    'a,
    const MAX_COURIERS: usize,
    const MAX_CONTEXTS: usize,
    const MAX_VISIBLE: usize,
    const MAX_CHILDREN: usize,
    const MAX_FIBERS: usize,
    const MAX_METADATA: usize,
>(
    context: *mut (),
    courier: CourierId,
    summary: crate::courier::CourierRuntimeSummary,
    tick: u64,
) -> Result<(), CourierRuntimeSinkError> {
    let registry = unsafe {
        &mut *context.cast::<DomainRegistry<
            'a,
            MAX_COURIERS,
            MAX_CONTEXTS,
            MAX_VISIBLE,
            MAX_CHILDREN,
            MAX_FIBERS,
            MAX_METADATA,
        >>()
    };
    registry
        .record_runtime_summary(courier, summary, tick)
        .map_err(Into::into)
}

#[cfg(all(test, feature = "std", not(target_os = "none")))]
mod tests {
    use super::*;
    use crate::claims::{ClaimsDigest, ImageSealId};

    fn demo_plan(max_child_couriers: usize, max_live_fibers: usize) -> CourierPlan {
        CourierPlan::new(max_child_couriers, max_live_fibers)
            .with_fiber_metadata_capacity(1)
            .with_child_observation_capacity(1)
            .with_recent_dead_depth(4)
    }

    const DOMAIN_ID: DomainId = DomainId::new(0x5056_4153);
    const PRIMARY_COURIER: CourierId = CourierId::new(1);
    const SCOPED_COURIER: CourierId = CourierId::new(2);
    const FIBER_CONTEXT: ContextId = ContextId::new(0x100);
    const BLOCK_CONTEXT: ContextId = ContextId::new(0x101);

    #[test]
    fn couriers_enumerate_only_their_visible_contexts() {
        let mut registry: DomainRegistry<'_, 4, 8, 4> = DomainRegistry::new(DomainDescriptor {
            id: DOMAIN_ID,
            name: "pvas",
            kind: DomainKind::NativeSubstrate,
            caps: DomainCaps::COURIER_REGISTRY
                | DomainCaps::CONTEXT_REGISTRY
                | DomainCaps::COURIER_VISIBILITY,
        });

        registry
            .register_courier(CourierDescriptor {
                id: PRIMARY_COURIER,
                name: "primary",
                caps: CourierCaps::ENUMERATE_VISIBLE_CONTEXTS
                    | CourierCaps::PROJECT_CONTEXTS
                    | CourierCaps::SPAWN_SUB_FIBERS
                    | CourierCaps::DEBUG_CHANNEL,
                visibility: CourierVisibility::Full,
                claim_awareness: ClaimAwareness::Black,
                claim_context: Some(ClaimContextId::new(0xAAA0)),
                plan: demo_plan(2, 4),
            })
            .expect("primary courier should register");
        registry
            .register_context(
                PRIMARY_COURIER,
                ContextDescriptor {
                    id: FIBER_CONTEXT,
                    name: "primary.main",
                    kind: ContextKind::FiberMetadata,
                    caps: ContextCaps::PROJECTABLE | ContextCaps::CONTROL_ENDPOINT,
                    claim_context: Some(ClaimContextId::new(0xAAA0)),
                },
            )
            .expect("fiber metadata context should register");
        registry
            .register_context(
                PRIMARY_COURIER,
                ContextDescriptor {
                    id: BLOCK_CONTEXT,
                    name: "nvme0n1p1",
                    kind: ContextKind::StorageEndpoint,
                    caps: ContextCaps::PROJECTABLE | ContextCaps::CHANNEL_BACKED,
                    claim_context: None,
                },
            )
            .expect("block context should register");
        registry
            .register_courier(CourierDescriptor {
                id: SCOPED_COURIER,
                name: "scoped",
                caps: CourierCaps::ENUMERATE_VISIBLE_CONTEXTS,
                visibility: CourierVisibility::Scoped,
                claim_awareness: ClaimAwareness::Blind,
                claim_context: None,
                plan: demo_plan(0, 1),
            })
            .expect("scoped courier should register");
        registry
            .grant_context(SCOPED_COURIER, BLOCK_CONTEXT, ContextProjectionKind::Alias)
            .expect("scoped courier should receive one projected block context");

        let primary = registry
            .courier(PRIMARY_COURIER)
            .expect("primary courier should exist");
        assert_eq!(
            primary.courier_support().visibility,
            CourierVisibility::Full
        );
        assert_eq!(
            primary.courier_support().claim_awareness,
            ClaimAwareness::Black
        );
        assert_eq!(
            primary.courier_support().claim_context,
            Some(ClaimContextId::new(0xAAA0))
        );
        assert_eq!(primary.visible_context_count(), 2);

        let scoped = registry
            .courier(SCOPED_COURIER)
            .expect("scoped courier should exist");
        assert_eq!(
            scoped.courier_support().visibility,
            CourierVisibility::Scoped
        );
        assert_eq!(
            scoped.courier_support().claim_awareness,
            ClaimAwareness::Blind
        );
        assert_eq!(scoped.visible_context_count(), 1);
        assert!(!scoped.can_observe_context(FIBER_CONTEXT));
        assert!(scoped.can_observe_context(BLOCK_CONTEXT));

        let visible: [Option<&str>; 2] = {
            let mut names = [None; 2];
            for (index, context) in scoped.visible_contexts().enumerate() {
                names[index] = Some(context.record().descriptor.name);
                assert_eq!(
                    context.context_support().projection,
                    ContextProjectionKind::Alias
                );
            }
            names
        };
        assert_eq!(visible, [Some("nvme0n1p1"), None]);
    }

    #[test]
    fn duplicate_courier_ids_are_rejected() {
        let mut registry: DomainRegistry<'_, 4, 4, 4> = DomainRegistry::new(DomainDescriptor {
            id: DOMAIN_ID,
            name: "pvas",
            kind: DomainKind::NativeSubstrate,
            caps: DomainCaps::COURIER_REGISTRY | DomainCaps::COURIER_VISIBILITY,
        });
        registry
            .register_courier(CourierDescriptor {
                id: PRIMARY_COURIER,
                name: "primary",
                caps: CourierCaps::ENUMERATE_VISIBLE_CONTEXTS,
                visibility: CourierVisibility::Full,
                claim_awareness: ClaimAwareness::Blind,
                claim_context: None,
                plan: demo_plan(1, 2),
            })
            .expect("primary courier should register");

        let result = registry.register_courier(CourierDescriptor {
            id: PRIMARY_COURIER,
            name: "duplicate",
            caps: CourierCaps::ENUMERATE_VISIBLE_CONTEXTS,
            visibility: CourierVisibility::Scoped,
            claim_awareness: ClaimAwareness::Black,
            claim_context: Some(ClaimContextId::new(1)),
            plan: demo_plan(0, 1),
        });
        assert!(matches!(
            result,
            Err(error) if error.kind() == DomainErrorKind::StateConflict
        ));
    }

    #[test]
    fn child_couriers_and_fibers_are_visible_through_courier_handles() {
        let mut registry: DomainRegistry<'_, 4, 4, 4, 2, 4> =
            DomainRegistry::new(DomainDescriptor {
                id: DOMAIN_ID,
                name: "pvas",
                kind: DomainKind::NativeSubstrate,
                caps: DomainCaps::COURIER_REGISTRY | DomainCaps::COURIER_VISIBILITY,
            });
        let root_seal = LocalAdmissionSeal::new(
            ImageSealId::new(1),
            ClaimsDigest::zero(),
            ClaimsDigest::zero(),
            ClaimsDigest::zero(),
            47,
        );
        registry
            .register_courier(CourierDescriptor {
                id: PRIMARY_COURIER,
                name: "root",
                caps: CourierCaps::ENUMERATE_VISIBLE_CONTEXTS | CourierCaps::SPAWN_SUB_FIBERS,
                visibility: CourierVisibility::Full,
                claim_awareness: ClaimAwareness::Black,
                claim_context: Some(ClaimContextId::new(0xAAA0)),
                plan: demo_plan(2, 4),
            })
            .expect("root courier should register");
        registry
            .register_child_courier(
                PRIMARY_COURIER,
                CourierDescriptor {
                    id: SCOPED_COURIER,
                    name: "httpd",
                    caps: CourierCaps::ENUMERATE_VISIBLE_CONTEXTS | CourierCaps::SPAWN_SUB_FIBERS,
                    visibility: CourierVisibility::Scoped,
                    claim_awareness: ClaimAwareness::Black,
                    claim_context: Some(ClaimContextId::new(0xBBB0)),
                    plan: demo_plan(0, 2),
                },
                PrincipalId::parse("httpd#01@web[cache.pvas-local]:443").unwrap(),
                root_seal,
                47,
                10,
                FiberId::new(9),
            )
            .expect("child courier should register");
        registry
            .register_fiber(
                SCOPED_COURIER,
                ManagedFiberSnapshot {
                    id: FiberId::new(9),
                    state: crate::fiber::FiberState::Created,
                    started: false,
                    claim_awareness: ClaimAwareness::Black,
                    claim_context: Some(ClaimContextId::new(0xBBB0)),
                },
                1,
                true,
                Some(FiberMetadataAttachment::new(11)),
                10,
            )
            .expect("root fiber should register under child courier");

        let parent = registry.courier(PRIMARY_COURIER).unwrap();
        assert_eq!(parent.plan(), demo_plan(2, 4));
        assert_eq!(parent.child_courier_count(), 1);
        let child = parent.child_couriers().next().unwrap();
        assert_eq!(child.child, SCOPED_COURIER);
        assert_eq!(child.root_fiber, FiberId::new(9));

        let launched = registry.courier(SCOPED_COURIER).unwrap();
        assert_eq!(launched.parent_courier(), Some(PRIMARY_COURIER));
        assert_eq!(launched.fiber_count(), 1);
        let root = launched.fiber(FiberId::new(9)).unwrap();
        assert!(root.is_root);
        assert_eq!(
            root.metadata_attachment,
            Some(FiberMetadataAttachment::new(11))
        );
    }

    #[test]
    fn child_progress_updates_parent_and_child_launch_state() {
        let mut registry: DomainRegistry<'_, 4, 4, 4, 2, 2> =
            DomainRegistry::new(DomainDescriptor {
                id: DOMAIN_ID,
                name: "pvas",
                kind: DomainKind::NativeSubstrate,
                caps: DomainCaps::COURIER_REGISTRY | DomainCaps::COURIER_VISIBILITY,
            });
        let seal = LocalAdmissionSeal::new(
            ImageSealId::new(2),
            ClaimsDigest::zero(),
            ClaimsDigest::zero(),
            ClaimsDigest::zero(),
            51,
        );
        registry
            .register_courier(CourierDescriptor {
                id: PRIMARY_COURIER,
                name: "root",
                caps: CourierCaps::ENUMERATE_VISIBLE_CONTEXTS,
                visibility: CourierVisibility::Full,
                claim_awareness: ClaimAwareness::Black,
                claim_context: Some(ClaimContextId::new(1)),
                plan: demo_plan(1, 1),
            })
            .unwrap();
        registry
            .register_child_courier(
                PRIMARY_COURIER,
                CourierDescriptor {
                    id: SCOPED_COURIER,
                    name: "child",
                    caps: CourierCaps::ENUMERATE_VISIBLE_CONTEXTS,
                    visibility: CourierVisibility::Scoped,
                    claim_awareness: ClaimAwareness::Blind,
                    claim_context: None,
                    plan: demo_plan(0, 1),
                },
                PrincipalId::parse("child@svc[kernel-local]").unwrap(),
                seal,
                51,
                20,
                FiberId::new(1),
            )
            .unwrap();
        registry
            .mark_child_stale(PRIMARY_COURIER, SCOPED_COURIER)
            .unwrap();
        assert_eq!(
            registry
                .courier(SCOPED_COURIER)
                .unwrap()
                .launch_record()
                .unwrap()
                .responsiveness,
            CourierResponsiveness::Stale
        );
        registry
            .record_child_progress(PRIMARY_COURIER, SCOPED_COURIER, 44)
            .unwrap();
        let child = registry.courier(SCOPED_COURIER).unwrap();
        assert_eq!(child.launch_record().unwrap().last_progress_tick, 44);
        assert_eq!(
            child.launch_record().unwrap().responsiveness,
            CourierResponsiveness::Responsive
        );
        let parent = registry.courier(PRIMARY_COURIER).unwrap();
        assert_eq!(
            parent.child_couriers().next().unwrap().last_progress_tick,
            44
        );
    }

    #[test]
    fn courier_owned_metadata_updates_drive_authoritative_progress() {
        let mut registry: DomainRegistry<'_, 4, 4, 4, 2, 2, 8> =
            DomainRegistry::new(DomainDescriptor {
                id: DOMAIN_ID,
                name: "pvas",
                kind: DomainKind::NativeSubstrate,
                caps: DomainCaps::COURIER_REGISTRY | DomainCaps::COURIER_VISIBILITY,
            });
        let seal = LocalAdmissionSeal::new(
            ImageSealId::new(3),
            ClaimsDigest::zero(),
            ClaimsDigest::zero(),
            ClaimsDigest::zero(),
            61,
        );
        registry
            .register_courier(CourierDescriptor {
                id: PRIMARY_COURIER,
                name: "root",
                caps: CourierCaps::ENUMERATE_VISIBLE_CONTEXTS,
                visibility: CourierVisibility::Full,
                claim_awareness: ClaimAwareness::Black,
                claim_context: Some(ClaimContextId::new(1)),
                plan: CourierPlan::new(1, 2)
                    .with_app_metadata_capacity(8)
                    .with_obligation_capacity(1),
            })
            .unwrap();
        registry
            .register_child_courier(
                PRIMARY_COURIER,
                CourierDescriptor {
                    id: SCOPED_COURIER,
                    name: "worker",
                    caps: CourierCaps::ENUMERATE_VISIBLE_CONTEXTS,
                    visibility: CourierVisibility::Scoped,
                    claim_awareness: ClaimAwareness::Blind,
                    claim_context: None,
                    plan: CourierPlan::new(0, 2)
                        .with_app_metadata_capacity(8)
                        .with_obligation_capacity(1),
                },
                PrincipalId::parse("worker@svc[kernel-local]").unwrap(),
                seal,
                61,
                20,
                FiberId::new(9),
            )
            .unwrap();
        registry
            .register_fiber(
                SCOPED_COURIER,
                ManagedFiberSnapshot {
                    id: FiberId::new(9),
                    state: crate::fiber::FiberState::Created,
                    started: false,
                    claim_awareness: ClaimAwareness::Blind,
                    claim_context: None,
                },
                1,
                true,
                None,
                20,
            )
            .unwrap();
        registry
            .mark_child_stale(PRIMARY_COURIER, SCOPED_COURIER)
            .unwrap();

        registry
            .upsert_courier_metadata(SCOPED_COURIER, "title", "worker", 55)
            .unwrap();
        registry
            .upsert_fiber_metadata(SCOPED_COURIER, FiberId::new(9), "phase", "boot", 56)
            .unwrap();
        registry
            .upsert_child_courier_metadata(PRIMARY_COURIER, SCOPED_COURIER, "status", "warm", 57)
            .unwrap();
        registry
            .upsert_async_metadata(SCOPED_COURIER, "executor", "ready", 58)
            .unwrap();

        let child = registry.courier(SCOPED_COURIER).unwrap();
        assert_eq!(child.launch_record().unwrap().last_progress_tick, 58);
        assert_eq!(
            child.launch_record().unwrap().responsiveness,
            CourierResponsiveness::Responsive
        );
        assert_eq!(
            child
                .courier_metadata_entry("title")
                .expect("courier metadata should exist")
                .value,
            "worker"
        );
        assert_eq!(
            child
                .fiber_metadata_entry(FiberId::new(9), "phase")
                .expect("fiber metadata should exist")
                .value,
            "boot"
        );
        let parent = registry.courier(PRIMARY_COURIER).unwrap();
        assert_eq!(
            parent.child_couriers().next().unwrap().last_progress_tick,
            58
        );
        assert_eq!(
            parent
                .child_courier_metadata_entry(SCOPED_COURIER, "status")
                .expect("parent-owned child metadata should exist")
                .value,
            "warm"
        );
        assert_eq!(
            child
                .async_metadata_entry("executor")
                .expect("async metadata should exist")
                .value,
            "ready"
        );
    }

    #[test]
    fn courier_obligations_drive_child_responsiveness() {
        let mut registry: DomainRegistry<'_, 4, 4, 4, 2, 2, 8> =
            DomainRegistry::new(DomainDescriptor {
                id: DOMAIN_ID,
                name: "pvas",
                kind: DomainKind::NativeSubstrate,
                caps: DomainCaps::COURIER_REGISTRY | DomainCaps::COURIER_VISIBILITY,
            });
        let seal = LocalAdmissionSeal::new(
            ImageSealId::new(4),
            ClaimsDigest::zero(),
            ClaimsDigest::zero(),
            ClaimsDigest::zero(),
            67,
        );
        registry
            .register_courier(CourierDescriptor {
                id: PRIMARY_COURIER,
                name: "root",
                caps: CourierCaps::ENUMERATE_VISIBLE_CONTEXTS,
                visibility: CourierVisibility::Full,
                claim_awareness: ClaimAwareness::Black,
                claim_context: Some(ClaimContextId::new(1)),
                plan: CourierPlan::new(1, 2)
                    .with_app_metadata_capacity(4)
                    .with_obligation_capacity(4),
            })
            .unwrap();
        registry
            .register_child_courier(
                PRIMARY_COURIER,
                CourierDescriptor {
                    id: SCOPED_COURIER,
                    name: "text-editor",
                    caps: CourierCaps::ENUMERATE_VISIBLE_CONTEXTS,
                    visibility: CourierVisibility::Scoped,
                    claim_awareness: ClaimAwareness::Blind,
                    claim_context: None,
                    plan: CourierPlan::new(0, 1)
                        .with_app_metadata_capacity(4)
                        .with_obligation_capacity(4),
                },
                PrincipalId::parse("editor@user[pvas-local]").unwrap(),
                seal,
                67,
                10,
                FiberId::new(1),
            )
            .unwrap();
        let obligation = registry
            .register_input_obligation(
                SCOPED_COURIER,
                CourierMetadataSubject::Courier,
                "hw.keyboard@kernel-local[pvas.me]",
                5,
                10,
                10,
            )
            .unwrap();
        assert_eq!(
            registry
                .evaluate_courier_responsiveness(SCOPED_COURIER, 15)
                .unwrap(),
            CourierResponsiveness::Stale
        );
        assert_eq!(
            registry
                .courier(SCOPED_COURIER)
                .unwrap()
                .launch_record()
                .unwrap()
                .responsiveness,
            CourierResponsiveness::Stale
        );
        assert_eq!(
            registry
                .courier(PRIMARY_COURIER)
                .unwrap()
                .child_couriers()
                .next()
                .unwrap()
                .responsiveness,
            CourierResponsiveness::Stale
        );
        registry
            .record_obligation_progress(SCOPED_COURIER, obligation, 16)
            .unwrap();
        assert_eq!(
            registry
                .courier(SCOPED_COURIER)
                .unwrap()
                .launch_record()
                .unwrap()
                .responsiveness,
            CourierResponsiveness::Responsive
        );
        assert_eq!(
            registry
                .evaluate_courier_responsiveness(SCOPED_COURIER, 26)
                .unwrap(),
            CourierResponsiveness::NonResponsive
        );
        let child = registry.courier(SCOPED_COURIER).unwrap();
        assert_eq!(child.obligation_count(), 1);
        let obligation = child.obligation(obligation).unwrap();
        assert_eq!(
            obligation.kind,
            crate::courier::CourierObligationKind::Input
        );
        assert_eq!(
            obligation.binding,
            CourierObligationBinding::Input("hw.keyboard@kernel-local[pvas.me]")
        );
        assert_eq!(obligation.label, "hw.keyboard@kernel-local[pvas.me]");
    }

    #[test]
    fn courier_plan_bounds_child_and_fiber_registration() {
        let mut registry: DomainRegistry<'_, 4, 4, 4, 1, 1> =
            DomainRegistry::new(DomainDescriptor {
                id: DOMAIN_ID,
                name: "pvas",
                kind: DomainKind::NativeSubstrate,
                caps: DomainCaps::COURIER_REGISTRY | DomainCaps::COURIER_VISIBILITY,
            });
        let seal = LocalAdmissionSeal::new(
            ImageSealId::new(3),
            ClaimsDigest::zero(),
            ClaimsDigest::zero(),
            ClaimsDigest::zero(),
            52,
        );
        registry
            .register_courier(CourierDescriptor {
                id: PRIMARY_COURIER,
                name: "root",
                caps: CourierCaps::ENUMERATE_VISIBLE_CONTEXTS,
                visibility: CourierVisibility::Full,
                claim_awareness: ClaimAwareness::Blind,
                claim_context: None,
                plan: demo_plan(1, 1),
            })
            .unwrap();
        registry
            .register_child_courier(
                PRIMARY_COURIER,
                CourierDescriptor {
                    id: SCOPED_COURIER,
                    name: "first",
                    caps: CourierCaps::ENUMERATE_VISIBLE_CONTEXTS,
                    visibility: CourierVisibility::Scoped,
                    claim_awareness: ClaimAwareness::Blind,
                    claim_context: None,
                    plan: demo_plan(0, 1),
                },
                PrincipalId::parse("first@svc[kernel-local]").unwrap(),
                seal,
                52,
                1,
                FiberId::new(1),
            )
            .unwrap();
        let second_child = registry.register_child_courier(
            PRIMARY_COURIER,
            CourierDescriptor {
                id: CourierId::new(3),
                name: "second",
                caps: CourierCaps::ENUMERATE_VISIBLE_CONTEXTS,
                visibility: CourierVisibility::Scoped,
                claim_awareness: ClaimAwareness::Blind,
                claim_context: None,
                plan: demo_plan(0, 1),
            },
            PrincipalId::parse("second@svc[kernel-local]").unwrap(),
            seal,
            52,
            2,
            FiberId::new(2),
        );
        assert!(matches!(
            second_child,
            Err(error) if error.kind() == DomainErrorKind::ResourceExhausted
        ));

        registry
            .register_fiber(
                SCOPED_COURIER,
                ManagedFiberSnapshot {
                    id: FiberId::new(1),
                    state: crate::fiber::FiberState::Created,
                    started: false,
                    claim_awareness: ClaimAwareness::Blind,
                    claim_context: None,
                },
                1,
                true,
                None,
                1,
            )
            .unwrap();
        let second_fiber = registry.register_fiber(
            SCOPED_COURIER,
            ManagedFiberSnapshot {
                id: FiberId::new(2),
                state: crate::fiber::FiberState::Created,
                started: false,
                claim_awareness: ClaimAwareness::Blind,
                claim_context: None,
            },
            1,
            false,
            None,
            2,
        );
        assert!(matches!(
            second_fiber,
            Err(error) if error.kind() == DomainErrorKind::ResourceExhausted
        ));
    }
}
