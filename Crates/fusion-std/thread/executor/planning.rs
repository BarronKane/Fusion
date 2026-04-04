/// Backing request for one executor-owned logical storage domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ExecutorBackingRequest {
    /// Minimum bytes the backing resource should expose to satisfy the domain honestly from an
    /// arbitrarily aligned base address.
    pub bytes: usize,
    /// Maximum alignment this domain may request from the underlying allocator/pool layer.
    pub align: usize,
}

impl ExecutorBackingRequest {
    fn from_extent_request(
        request: fusion_sys::alloc::MemoryPoolExtentRequest,
    ) -> Result<Self, ExecutorError> {
        Self::from_extent_request_with_layout_policy(
            request,
            AllocatorLayoutPolicy::hosted_vm(
                fusion_pal::sys::mem::system_mem().page_info().alloc_granule,
            ),
        )
    }

    fn from_extent_request_with_layout_policy(
        request: fusion_sys::alloc::MemoryPoolExtentRequest,
        layout_policy: AllocatorLayoutPolicy,
    ) -> Result<Self, ExecutorError> {
        let request = Allocator::<1, 1>::resource_request_for_extent_request_with_layout_policy(
            request,
            layout_policy,
        )
        .map_err(executor_error_from_alloc)?;
        Ok(Self {
            bytes: request.provisioning_len().ok_or_else(executor_overflow)?,
            align: request.align,
        })
    }
}

/// Planning-time layout surface for executor-owned current-thread slabs.
///
/// This exists for the same reason `FiberPlanningSupport` exists: exact build-time slab planning
/// must be able to use target/runtime layout truth without pretending the host binary's `size_of`
/// answers are universal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ExecutorPlanningSupport {
    /// Concrete control-block extent bytes for the executor core lease.
    pub control_bytes: usize,
    /// Concrete control-block extent alignment for the executor core lease.
    pub control_align: usize,
    /// Concrete reactor wait-entry bytes.
    pub reactor_wait_entry_bytes: usize,
    /// Concrete reactor wait-entry alignment.
    pub reactor_wait_entry_align: usize,
    /// Concrete reactor outcome-entry bytes.
    pub reactor_outcome_entry_bytes: usize,
    /// Concrete reactor outcome-entry alignment.
    pub reactor_outcome_entry_align: usize,
    /// Concrete current-queue entry bytes.
    pub reactor_queue_entry_bytes: usize,
    /// Concrete current-queue entry alignment.
    pub reactor_queue_entry_align: usize,
    /// Concrete pending-deregister entry bytes.
    pub reactor_pending_entry_bytes: usize,
    /// Concrete pending-deregister entry alignment.
    pub reactor_pending_entry_align: usize,
    /// Concrete registry free-index entry bytes.
    pub registry_free_entry_bytes: usize,
    /// Concrete registry free-index entry alignment.
    pub registry_free_entry_align: usize,
    /// Concrete async task-slot bytes.
    pub registry_slot_bytes: usize,
    /// Concrete async task-slot alignment.
    pub registry_slot_align: usize,
}

impl ExecutorPlanningSupport {
    /// Returns executor planning support for the currently compiled binary.
    #[must_use]
    pub const fn compiled_binary() -> Self {
        let control = match ControlLease::<ExecutorCore>::extent_request() {
            Ok(request) => request,
            Err(_) => fusion_sys::alloc::MemoryPoolExtentRequest { len: 0, align: 1 },
        };
        Self {
            control_bytes: control.len,
            control_align: control.align,
            reactor_wait_entry_bytes: size_of::<AsyncReactorWaitEntry>(),
            reactor_wait_entry_align: align_of::<AsyncReactorWaitEntry>(),
            reactor_outcome_entry_bytes: size_of::<Option<AsyncWaitOutcome>>(),
            reactor_outcome_entry_align: align_of::<Option<AsyncWaitOutcome>>(),
            reactor_queue_entry_bytes: size_of::<Option<CurrentJob>>(),
            reactor_queue_entry_align: align_of::<Option<CurrentJob>>(),
            #[cfg(feature = "std")]
            reactor_pending_entry_bytes: size_of::<Option<EventKey>>(),
            #[cfg(feature = "std")]
            reactor_pending_entry_align: align_of::<Option<EventKey>>(),
            #[cfg(not(feature = "std"))]
            reactor_pending_entry_bytes: 0,
            #[cfg(not(feature = "std"))]
            reactor_pending_entry_align: 1,
            registry_free_entry_bytes: size_of::<usize>(),
            registry_free_entry_align: align_of::<usize>(),
            registry_slot_bytes: size_of::<AsyncTaskSlot>(),
            registry_slot_align: align_of::<AsyncTaskSlot>(),
        }
    }

    /// Returns the default planning support for the selected build target/runtime lane.
    #[must_use]
    pub const fn selected_target() -> Self {
        Self::compiled_binary()
    }

    const fn reactor_align(self) -> usize {
        let mut align = self.reactor_wait_entry_align;
        if self.reactor_outcome_entry_align > align {
            align = self.reactor_outcome_entry_align;
        }
        if self.reactor_queue_entry_align > align {
            align = self.reactor_queue_entry_align;
        }
        if self.reactor_pending_entry_align > align {
            align = self.reactor_pending_entry_align;
        }
        align
    }

    fn reactor_capacity(self, capacity: usize) -> Result<usize, ExecutorError> {
        if capacity == 0 {
            return Err(executor_invalid());
        }

        let waits_bytes = self
            .reactor_wait_entry_bytes
            .checked_mul(capacity)
            .ok_or_else(executor_overflow)?;
        let outcomes_bytes = self
            .reactor_outcome_entry_bytes
            .checked_mul(capacity)
            .ok_or_else(executor_overflow)?;
        let queue_bytes = self
            .reactor_queue_entry_bytes
            .checked_mul(capacity)
            .ok_or_else(executor_overflow)?;
        let pending_bytes = self
            .reactor_pending_entry_bytes
            .checked_mul(capacity)
            .ok_or_else(executor_overflow)?;
        let padding = self.reactor_align();
        let segments = if self.reactor_pending_entry_bytes == 0 {
            3
        } else {
            4
        };
        waits_bytes
            .checked_add(outcomes_bytes)
            .and_then(|total| total.checked_add(queue_bytes))
            .and_then(|total| total.checked_add(pending_bytes))
            .and_then(|total| total.checked_add(padding.saturating_mul(segments)))
            .ok_or_else(executor_overflow)
    }

    const fn registry_align(self) -> usize {
        if self.registry_free_entry_align > self.registry_slot_align {
            self.registry_free_entry_align
        } else {
            self.registry_slot_align
        }
    }

    fn registry_capacity(self, capacity: usize) -> Result<usize, ExecutorError> {
        if capacity == 0 {
            return Err(executor_invalid());
        }

        let free_bytes = self
            .registry_free_entry_bytes
            .checked_mul(capacity)
            .ok_or_else(executor_overflow)?;
        let slot_bytes = self
            .registry_slot_bytes
            .checked_mul(capacity)
            .ok_or_else(executor_overflow)?;
        let padding = self.registry_align();
        free_bytes
            .checked_add(slot_bytes)
            .and_then(|total| total.checked_add(padding.saturating_mul(2)))
            .ok_or_else(executor_overflow)
    }
}

/// Compile-time/backing-time footprint plan for one current-thread async runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CurrentAsyncRuntimeBackingPlan {
    /// Executor control/state backing.
    pub control: ExecutorBackingRequest,
    /// Reactor bookkeeping backing.
    pub reactor: ExecutorBackingRequest,
    /// Task registry backing.
    pub registry: ExecutorBackingRequest,
    /// Optional exact async spill-domain backing shared across future/result lifecycle envelopes.
    pub spill: ExecutorBackingRequest,
}

/// Packed one-slab layout for one current-thread async runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CurrentAsyncRuntimeCombinedBackingPlan {
    /// Total owning slab request for all current-thread async domains.
    pub slab: ExecutorBackingRequest,
    /// Executor control/state range inside the slab.
    pub control: ResourceRange,
    /// Reactor bookkeeping range inside the slab.
    pub reactor: ResourceRange,
    /// Task registry range inside the slab.
    pub registry: ResourceRange,
    /// Optional exact async spill-domain range inside the slab.
    pub spill: Option<ResourceRange>,
}

/// Exact configured memory footprint for one async runtime backing shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AsyncRuntimeMemoryFootprint {
    /// Executor control/state bytes.
    pub control_bytes: usize,
    /// Reactor bookkeeping bytes.
    pub reactor_bytes: usize,
    /// Task registry bytes.
    pub registry_bytes: usize,
    /// Optional exact async spill-domain bytes.
    pub spill_bytes: usize,
    /// Extra packed-slab padding bytes introduced by alignment.
    pub packing_padding_bytes: usize,
}

impl AsyncRuntimeMemoryFootprint {
    /// Returns the logical domain bytes before any owning-slab packing padding.
    #[must_use]
    pub const fn domain_bytes(self) -> usize {
        self.control_bytes + self.reactor_bytes + self.registry_bytes + self.spill_bytes
    }

    /// Returns the total reserved bytes for this async runtime shape.
    #[must_use]
    pub const fn total_bytes(self) -> usize {
        self.domain_bytes() + self.packing_padding_bytes
    }
}

impl CurrentAsyncRuntimeBackingPlan {
    /// Returns the exact configured footprint for this per-domain backing plan.
    #[must_use]
    pub const fn memory_footprint(self) -> AsyncRuntimeMemoryFootprint {
        AsyncRuntimeMemoryFootprint {
            control_bytes: self.control.bytes,
            reactor_bytes: self.reactor.bytes,
            registry_bytes: self.registry.bytes,
            spill_bytes: self.spill.bytes,
            packing_padding_bytes: 0,
        }
    }
}

impl CurrentAsyncRuntimeCombinedBackingPlan {
    /// Returns the exact packed footprint for this one-slab backing plan.
    #[must_use]
    pub const fn memory_footprint(self) -> AsyncRuntimeMemoryFootprint {
        let spill_bytes = match self.spill {
            Some(range) => range.len,
            None => 0,
        };
        let domain_bytes = self.control.len + self.reactor.len + self.registry.len + spill_bytes;
        AsyncRuntimeMemoryFootprint {
            control_bytes: self.control.len,
            reactor_bytes: self.reactor.len,
            registry_bytes: self.registry.len,
            spill_bytes,
            packing_padding_bytes: self.slab.bytes.saturating_sub(domain_bytes),
        }
    }
}

fn executor_align_up_packed(offset: usize, align: usize) -> Result<usize, ExecutorError> {
    if align == 0 || !align.is_power_of_two() {
        return Err(executor_invalid());
    }
    let mask = align - 1;
    offset
        .checked_add(mask)
        .map(|value| value & !mask)
        .ok_or_else(executor_overflow)
}

const fn executor_partition_backing_kind(
    kind: ResourceBackingKind,
) -> Result<ResourceBackingKind, ExecutorError> {
    match kind {
        ResourceBackingKind::Borrowed
        | ResourceBackingKind::StaticRegion
        | ResourceBackingKind::Partition => Ok(ResourceBackingKind::Partition),
        _ => Err(ExecutorError::Unsupported),
    }
}

fn partition_executor_bound_resource(
    handle: &MemoryResourceHandle,
    range: ResourceRange,
) -> Result<MemoryResourceHandle, ExecutorError> {
    let info = match handle {
        MemoryResourceHandle::Bound(resource) => resource.info(),
        MemoryResourceHandle::Virtual(_) => return Err(ExecutorError::Unsupported),
    };
    let region = handle
        .subview(range)
        .map(|view| unsafe { view.raw_region() })
        .map_err(executor_error_from_resource)?;
    let resource = BoundMemoryResource::new(BoundResourceSpec::new(
        region,
        info.domain,
        executor_partition_backing_kind(info.backing)?,
        info.attrs,
        info.geometry,
        info.layout,
        info.contract,
        info.support,
        handle.state(),
    ))
    .map_err(executor_error_from_resource)?;
    Ok(MemoryResourceHandle::from(resource))
}

const fn executor_resource_base_alignment_from_addr(addr: usize) -> usize {
    if addr == 0 {
        1
    } else {
        1usize << addr.trailing_zeros()
    }
}

fn executor_resource_base_alignment(handle: &MemoryResourceHandle) -> usize {
    executor_resource_base_alignment_from_addr(handle.view().base_addr().get())
}

impl CurrentAsyncRuntimeBackingPlan {
    fn combined_with_base_alignment(
        self,
        base_align: usize,
    ) -> Result<CurrentAsyncRuntimeCombinedBackingPlan, ExecutorError> {
        let mut max_align = self.control.align;
        if self.reactor.align > max_align {
            max_align = self.reactor.align;
        }
        if self.registry.align > max_align {
            max_align = self.registry.align;
        }
        if self.spill.align > max_align {
            max_align = self.spill.align;
        }

        let mut cursor = if base_align >= max_align {
            0
        } else {
            max_align.saturating_sub(1)
        };
        let control_offset = executor_align_up_packed(cursor, self.control.align)?;
        cursor = control_offset
            .checked_add(self.control.bytes)
            .ok_or_else(executor_overflow)?;
        let reactor_offset = executor_align_up_packed(cursor, self.reactor.align)?;
        cursor = reactor_offset
            .checked_add(self.reactor.bytes)
            .ok_or_else(executor_overflow)?;
        let registry_offset = executor_align_up_packed(cursor, self.registry.align)?;
        cursor = registry_offset
            .checked_add(self.registry.bytes)
            .ok_or_else(executor_overflow)?;
        let spill_offset = executor_align_up_packed(cursor, self.spill.align)?;
        let total_bytes = spill_offset
            .checked_add(self.spill.bytes)
            .ok_or_else(executor_overflow)?;

        Ok(CurrentAsyncRuntimeCombinedBackingPlan {
            slab: ExecutorBackingRequest {
                bytes: total_bytes,
                align: max_align,
            },
            control: ResourceRange::new(control_offset, self.control.bytes),
            reactor: ResourceRange::new(reactor_offset, self.reactor.bytes),
            registry: ResourceRange::new(registry_offset, self.registry.bytes),
            spill: Some(ResourceRange::new(spill_offset, self.spill.bytes)),
        })
    }

    fn combined_eager_with_base_alignment(
        self,
        base_align: usize,
    ) -> Result<CurrentAsyncRuntimeCombinedBackingPlan, ExecutorError> {
        self.combined_with_base_alignment(base_align)
    }

    /// Returns the explicit backing plan for one current-thread runtime configuration.
    ///
    /// The optional async spill domain is part of the full plan so hosted or explicitly
    /// provisioned runtimes can reserve exact-envelope backing up front instead of improvising
    /// another acquisition story later.
    pub fn for_config(config: ExecutorConfig) -> Result<Self, ExecutorError> {
        Self::for_config_with_layout_policy(
            config,
            AllocatorLayoutPolicy::hosted_vm(
                fusion_pal::sys::mem::system_mem().page_info().alloc_granule,
            ),
        )
    }

    /// Returns the explicit backing plan for one current-thread runtime configuration under one
    /// explicit allocator layout policy.
    pub fn for_config_with_layout_policy(
        config: ExecutorConfig,
        layout_policy: AllocatorLayoutPolicy,
    ) -> Result<Self, ExecutorError> {
        Self::for_config_with_layout_policy_and_planning_support(
            config,
            layout_policy,
            ExecutorPlanningSupport::selected_target(),
        )
    }

    /// Returns the explicit backing plan for one current-thread runtime configuration under one
    /// explicit allocator layout policy and one explicit executor-planning surface.
    pub fn for_config_with_layout_policy_and_planning_support(
        config: ExecutorConfig,
        layout_policy: AllocatorLayoutPolicy,
        planning: ExecutorPlanningSupport,
    ) -> Result<Self, ExecutorError> {
        let sizing = config.sizing;
        let control = apply_executor_sizing_strategy(
            ExecutorBackingRequest::from_extent_request_with_layout_policy(
                fusion_sys::alloc::MemoryPoolExtentRequest {
                    len: planning.control_bytes,
                    align: planning.control_align,
                },
                layout_policy,
            )?,
            sizing,
        )?;
        let reactor = apply_executor_sizing_strategy(
            ExecutorBackingRequest::from_extent_request_with_layout_policy(
                BoundedArena::<fusion_sys::alloc::Mortal>::extent_request_with_layout_policy(
                    executor_reactor_capacity_with_planning_support(config.capacity, planning)?,
                    executor_reactor_align_with_planning_support(planning),
                    layout_policy,
                )
                .map_err(executor_error_from_alloc)?,
                layout_policy,
            )?,
            sizing,
        )?;
        let registry = apply_executor_sizing_strategy(
            ExecutorBackingRequest::from_extent_request_with_layout_policy(
                BoundedArena::<fusion_sys::alloc::Mortal>::extent_request_with_layout_policy(
                    executor_registry_capacity_with_planning_support(config.capacity, planning)?,
                    executor_registry_align_with_planning_support(planning),
                    layout_policy,
                )
                .map_err(executor_error_from_alloc)?,
                layout_policy,
            )?,
            sizing,
        )?;
        let spill = apply_executor_sizing_strategy(
            ExecutorBackingRequest::from_extent_request_with_layout_policy(
                fusion_sys::alloc::MemoryPoolExtentRequest {
                    len: executor_async_spill_capacity_bytes(config.capacity)?,
                    align: default_async_spill_align(),
                },
                layout_policy,
            )?,
            sizing,
        )?;

        Ok(Self {
            control,
            reactor,
            registry,
            spill,
        })
    }

    /// Packs the per-domain requests into one conservative owning-slab layout.
    ///
    /// The total byte count includes worst-case padding for an arbitrarily aligned caller-owned
    /// slab base.
    pub fn combined(self) -> Result<CurrentAsyncRuntimeCombinedBackingPlan, ExecutorError> {
        self.combined_with_base_alignment(1)
    }

    /// Packs the per-domain requests into one owning slab for a caller that can guarantee the
    /// slab base is aligned to at least `base_align`.
    pub fn combined_for_base_alignment(
        self,
        base_align: usize,
    ) -> Result<CurrentAsyncRuntimeCombinedBackingPlan, ExecutorError> {
        self.combined_with_base_alignment(base_align)
    }

    /// Packs only the eagerly required current-thread async domains into one owning slab.
    ///
    /// Exact task backing is now part of the honest runtime floor, so the eager plan includes the
    /// shared async spill domain as well.
    pub fn combined_eager(self) -> Result<CurrentAsyncRuntimeCombinedBackingPlan, ExecutorError> {
        self.combined_eager_with_base_alignment(1)
    }

    /// Packs only the eagerly required domains into one owning slab for a caller that can
    /// guarantee the slab base is aligned to at least `base_align`.
    pub fn combined_eager_for_base_alignment(
        self,
        base_align: usize,
    ) -> Result<CurrentAsyncRuntimeCombinedBackingPlan, ExecutorError> {
        self.combined_eager_with_base_alignment(base_align)
    }
}
