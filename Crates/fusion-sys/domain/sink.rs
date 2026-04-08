use super::*;

pub(super) fn runtime_sink_vtable<
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
        runtime_ledger: runtime_sink_runtime_ledger::<
            MAX_COURIERS,
            MAX_CONTEXTS,
            MAX_VISIBLE,
            MAX_CHILDREN,
            MAX_FIBERS,
            MAX_METADATA,
        >,
        fiber_record: runtime_sink_fiber_record::<
            MAX_COURIERS,
            MAX_CONTEXTS,
            MAX_VISIBLE,
            MAX_CHILDREN,
            MAX_FIBERS,
            MAX_METADATA,
        >,
        evaluate_responsiveness: runtime_sink_evaluate_responsiveness::<
            MAX_COURIERS,
            MAX_CONTEXTS,
            MAX_VISIBLE,
            MAX_CHILDREN,
            MAX_FIBERS,
            MAX_METADATA,
        >,
        upsert_metadata: runtime_sink_upsert_metadata::<
            MAX_COURIERS,
            MAX_CONTEXTS,
            MAX_VISIBLE,
            MAX_CHILDREN,
            MAX_FIBERS,
            MAX_METADATA,
        >,
        remove_metadata: runtime_sink_remove_metadata::<
            MAX_COURIERS,
            MAX_CONTEXTS,
            MAX_VISIBLE,
            MAX_CHILDREN,
            MAX_FIBERS,
            MAX_METADATA,
        >,
        register_obligation: runtime_sink_register_obligation::<
            MAX_COURIERS,
            MAX_CONTEXTS,
            MAX_VISIBLE,
            MAX_CHILDREN,
            MAX_FIBERS,
            MAX_METADATA,
        >,
        record_obligation_progress: runtime_sink_record_obligation_progress::<
            MAX_COURIERS,
            MAX_CONTEXTS,
            MAX_VISIBLE,
            MAX_CHILDREN,
            MAX_FIBERS,
            MAX_METADATA,
        >,
        remove_obligation: runtime_sink_remove_obligation::<
            MAX_COURIERS,
            MAX_CONTEXTS,
            MAX_VISIBLE,
            MAX_CHILDREN,
            MAX_FIBERS,
            MAX_METADATA,
        >,
    }
}

pub(super) fn launch_control_vtable<
    'a,
    const MAX_COURIERS: usize,
    const MAX_CONTEXTS: usize,
    const MAX_VISIBLE: usize,
    const MAX_CHILDREN: usize,
    const MAX_FIBERS: usize,
    const MAX_METADATA: usize,
>() -> CourierLaunchControlVTable<'a> {
    CourierLaunchControlVTable {
        register_child_courier: launch_control_register_child_courier::<
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

unsafe fn launch_control_register_child_courier<
    'a,
    const MAX_COURIERS: usize,
    const MAX_CONTEXTS: usize,
    const MAX_VISIBLE: usize,
    const MAX_CHILDREN: usize,
    const MAX_FIBERS: usize,
    const MAX_METADATA: usize,
>(
    context: *mut (),
    request: CourierChildLaunchRequest<'a>,
    launched_at_tick: u64,
    root_fiber: FiberId,
) -> Result<(), CourierLaunchControlError> {
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
        .register_child_courier(
            request.parent,
            CourierDescriptor {
                id: request.descriptor.id,
                name: request.descriptor.name,
                scope_role: request.descriptor.scope_role,
                caps: request.descriptor.caps,
                visibility: request.descriptor.visibility,
                claim_awareness: request.descriptor.claim_awareness,
                claim_context: request.descriptor.claim_context,
                plan: request.descriptor.plan,
            },
            request.principal,
            request.image_seal,
            request.launch_epoch,
            launched_at_tick,
            root_fiber,
        )
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

unsafe fn runtime_sink_runtime_ledger<
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
) -> Result<CourierRuntimeLedger, CourierRuntimeSinkError> {
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
    registry.runtime_ledger(courier).map_err(Into::into)
}

unsafe fn runtime_sink_fiber_record<
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
) -> Result<Option<CourierFiberRecord>, CourierRuntimeSinkError> {
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
    registry.fiber_record(courier, fiber).map_err(Into::into)
}

unsafe fn runtime_sink_evaluate_responsiveness<
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
    tick: u64,
) -> Result<CourierResponsiveness, CourierRuntimeSinkError> {
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
        .courier_responsiveness(courier, tick)
        .map_err(Into::into)
}

unsafe fn runtime_sink_upsert_metadata<
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
    subject: CourierMetadataSubject,
    key: &'static str,
    value: &'static str,
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
    match subject {
        CourierMetadataSubject::Courier => {
            registry.upsert_courier_metadata(courier, key, value, tick)
        }
        CourierMetadataSubject::ChildCourier(child) => {
            registry.upsert_child_courier_metadata(courier, child, key, value, tick)
        }
        CourierMetadataSubject::Fiber(fiber) => {
            registry.upsert_fiber_metadata(courier, fiber, key, value, tick)
        }
        CourierMetadataSubject::Context(runtime_context) => {
            registry.upsert_context_metadata(runtime_context, key, value, tick)
        }
        CourierMetadataSubject::AsyncLane => {
            registry.upsert_async_metadata(courier, key, value, tick)
        }
    }
    .map_err(Into::into)
}

unsafe fn runtime_sink_remove_metadata<
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
    subject: CourierMetadataSubject,
    key: &str,
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
        .remove_metadata(courier, subject, key)
        .map_err(Into::into)
}

unsafe fn runtime_sink_register_obligation<
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
    spec: CourierObligationSpec<'static>,
    tick: u64,
) -> Result<CourierObligationId, CourierRuntimeSinkError> {
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
        .register_obligation_spec(courier, spec, tick)
        .map_err(Into::into)
}

unsafe fn runtime_sink_record_obligation_progress<
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
    obligation: CourierObligationId,
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
        .record_obligation_progress(courier, obligation, tick)
        .map_err(Into::into)
}

unsafe fn runtime_sink_remove_obligation<
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
    obligation: CourierObligationId,
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
        .remove_obligation(courier, obligation)
        .map_err(Into::into)
}
