fn current_green_runtime_sink() -> Option<CourierRuntimeSink> {
    let context = current_green_context()?;
    // SAFETY: the current green context only points at a live owning pool while the task is
    // actively running on that pool.
    unsafe { (*context.inner).runtime_sink }
}

fn current_runtime_tick() -> u64 {
    current_monotonic_nanos().unwrap_or(0)
}

/// Returns the owning courier identity for the current running fiber/task when available.
///
/// This prefers the live green-task context and falls back to the lower `fusion-sys` managed-fiber
/// slot when the caller is running on one raw managed fiber outside the green scheduler.
///
/// # Errors
///
/// Returns an error when no current fiber/courier identity is available honestly.
#[allow(dead_code)]
pub fn current_courier_id() -> Result<CourierId, FiberError> {
    if let Some(context) = current_green_context()
        && let Some(courier_id) = context.courier_id
    {
        return Ok(courier_id);
    }
    system_current_courier_id()
}

/// Returns the courier-owned runtime ledger for the current running fiber/task when available.
///
/// # Errors
///
/// Returns an error when the current runtime cannot honestly resolve one courier-owned ledger.
pub fn current_courier_runtime_ledger() -> Result<CourierRuntimeLedger, FiberError> {
    let courier_id = current_courier_id()?;
    let runtime_sink = current_green_runtime_sink().ok_or_else(FiberError::unsupported)?;
    runtime_sink
        .runtime_ledger(courier_id)
        .map_err(fiber_error_from_runtime_sink)
}

/// Returns the courier-owned fiber ledger record for the current running fiber/task.
///
/// # Errors
///
/// Returns an error when the current runtime cannot honestly resolve one courier-owned fiber
/// record.
pub fn current_fiber_record() -> Result<CourierFiberRecord, FiberError> {
    let courier_id = current_courier_id()?;
    let fiber_id = current_fiber_id()?;
    let runtime_sink = current_green_runtime_sink().ok_or_else(FiberError::unsupported)?;
    runtime_sink
        .fiber_record(courier_id, fiber_id)
        .map_err(fiber_error_from_runtime_sink)?
        .ok_or_else(FiberError::state_conflict)
}

/// Returns the courier-owned responsiveness classification for the current running fiber/task.
///
/// # Errors
///
/// Returns an error when the current runtime cannot honestly resolve one courier-owned
/// responsiveness state.
pub fn current_courier_responsiveness() -> Result<CourierResponsiveness, FiberError> {
    let courier_id = current_courier_id()?;
    let runtime_sink = current_green_runtime_sink().ok_or_else(FiberError::unsupported)?;
    runtime_sink
        .evaluate_responsiveness(courier_id, current_runtime_tick())
        .map_err(fiber_error_from_runtime_sink)
}

fn current_runtime_metadata_subjects()
-> Result<(CourierRuntimeSink, CourierId, FiberId, ContextId, u64), FiberError> {
    let runtime_sink = current_green_runtime_sink().ok_or_else(FiberError::unsupported)?;
    Ok((
        runtime_sink,
        current_courier_id()?,
        current_fiber_id()?,
        current_context_id()?,
        current_runtime_tick(),
    ))
}

/// Updates one courier-owned metadata entry for the current courier.
///
/// This is the runtime-facing `/proc` lane: the courier owns the truth in memory, and running
/// fibers may submit app-defined metadata into that store without inventing mandatory channels.
///
/// # Errors
///
/// Returns an error when the current runtime cannot honestly resolve or update the owning courier.
pub fn update_current_courier_metadata(
    key: &'static str,
    value: &'static str,
) -> Result<(), FiberError> {
    let (runtime_sink, courier_id, _, _, tick) = current_runtime_metadata_subjects()?;
    runtime_sink
        .upsert_metadata(
            courier_id,
            CourierMetadataSubject::Courier,
            key,
            value,
            tick,
        )
        .map_err(fiber_error_from_runtime_sink)
}

/// Updates one courier-owned metadata entry for the current fiber.
///
/// # Errors
///
/// Returns an error when the current runtime cannot honestly resolve or update the current fiber.
pub fn update_current_fiber_metadata(
    key: &'static str,
    value: &'static str,
) -> Result<(), FiberError> {
    let (runtime_sink, courier_id, fiber_id, _, tick) = current_runtime_metadata_subjects()?;
    runtime_sink
        .upsert_metadata(
            courier_id,
            CourierMetadataSubject::Fiber(fiber_id),
            key,
            value,
            tick,
        )
        .map_err(fiber_error_from_runtime_sink)
}

/// Updates one courier-owned metadata entry for the current context.
///
/// # Errors
///
/// Returns an error when the current runtime cannot honestly resolve or update the current
/// context.
pub fn update_current_context_metadata(
    key: &'static str,
    value: &'static str,
) -> Result<(), FiberError> {
    let (runtime_sink, courier_id, _, context_id, tick) = current_runtime_metadata_subjects()?;
    runtime_sink
        .upsert_metadata(
            courier_id,
            CourierMetadataSubject::Context(context_id),
            key,
            value,
            tick,
        )
        .map_err(fiber_error_from_runtime_sink)
}

/// Registers one courier-level externally visible obligation under the current running fiber.
///
/// # Errors
///
/// Returns an error when the current runtime cannot honestly resolve or register the obligation.
pub fn register_current_courier_obligation(
    spec: CourierObligationSpec<'static>,
) -> Result<CourierObligationId, FiberError> {
    let (runtime_sink, courier_id, _, _, tick) = current_runtime_metadata_subjects()?;
    runtime_sink
        .register_obligation(courier_id, spec, tick)
        .map_err(fiber_error_from_runtime_sink)
}

/// Records progress on one previously registered courier obligation.
///
/// # Errors
///
/// Returns an error when the current runtime cannot honestly resolve or update the obligation.
pub fn record_current_courier_obligation_progress(
    obligation: CourierObligationId,
) -> Result<(), FiberError> {
    let (runtime_sink, courier_id, _, _, tick) = current_runtime_metadata_subjects()?;
    runtime_sink
        .record_obligation_progress(courier_id, obligation, tick)
        .map_err(fiber_error_from_runtime_sink)
}

/// Removes one previously registered courier obligation from the current running fiber's courier.
///
/// # Errors
///
/// Returns an error when the current runtime cannot honestly resolve or remove the obligation.
pub fn remove_current_courier_obligation(
    obligation: CourierObligationId,
) -> Result<(), FiberError> {
    let (runtime_sink, courier_id, _, _, _) = current_runtime_metadata_subjects()?;
    runtime_sink
        .remove_obligation(courier_id, obligation)
        .map_err(fiber_error_from_runtime_sink)
}

/// Returns the owning context identity for the current running fiber/task when available.
///
/// This prefers the live green-task context and falls back to the lower `fusion-sys` managed-fiber
/// slot when the caller is running on one raw managed fiber outside the green scheduler.
///
/// # Errors
///
/// Returns an error when no current fiber/context identity is available honestly.
#[allow(dead_code)]
pub fn current_context_id() -> Result<ContextId, FiberError> {
    if let Some(context) = current_green_context()
        && let Some(context_id) = context.context_id
    {
        return Ok(context_id);
    }
    system_current_context_id()
}

/// Returns the stable fiber identifier for the current running task when available.
///
/// This prefers the live green-task context and falls back to the lower `fusion-sys` managed-fiber
/// slot when the caller is running on one raw managed fiber outside the green scheduler.
///
/// # Errors
///
/// Returns an error when no current fiber identity is available honestly.
pub fn current_fiber_id() -> Result<FiberId, FiberError> {
    if let Some(context) = current_green_context() {
        return Ok(context.fiber_id);
    }
    fusion_sys::fiber::current_fiber_id()
}
