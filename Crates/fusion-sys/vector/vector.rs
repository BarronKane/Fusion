//! fusion-sys-level vector-ownership wrappers built on top of fusion-pal-truthful backends.

use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use fusion_pal::pal::thread::ThreadCoreId;
pub use fusion_pal::sys::vector::{
    IrqSlot,
    SlotState,
    SystemException,
    VectorBase,
    VectorCaps,
    VectorDispatchCookie,
    VectorDispatchLane,
    VectorError,
    VectorErrorKind,
    VectorImplementationKind,
    VectorInlineEligibility,
    VectorInlineHandler,
    VectorInlineStackPolicy,
    VectorOwnershipControl,
    VectorOwnershipKind,
    VectorPriority,
    VectorSealedQuery,
    VectorSecurityDomain,
    VectorSlotBinding,
    VectorSlotTarget,
    VectorSupport,
    VectorSystemBinding,
    VectorTableBuilderControl,
    VectorTableMode,
    VectorTableTopology,
};
use fusion_pal::sys::vector::{
    PlatformSealedVectorTable,
    PlatformVector,
    PlatformVectorBuilder,
    bind_reserved_pendsv_dispatch as platform_bind_reserved_pendsv_dispatch,
    system_vector as pal_system_vector,
    take_pending_active_scope as platform_take_pending_active_scope,
};

const VECTOR_DEFERRED_REGISTRY_CAPACITY: usize = 128;

/// Deferred callback registered through the fusion-sys vector bridge.
pub type VectorDeferredCallback = fn(VectorDispatchCookie);

static NEXT_VECTOR_COOKIE: AtomicU32 = AtomicU32::new(1);
static VECTOR_CALLBACK_REGISTRY: [AtomicUsize; VECTOR_DEFERRED_REGISTRY_CAPACITY] =
    [const { AtomicUsize::new(0) }; VECTOR_DEFERRED_REGISTRY_CAPACITY];

/// fusion-sys vector provider wrapper around the selected fusion-pal backend.
#[derive(Debug, Clone, Copy)]
pub struct VectorSystem {
    inner: PlatformVector,
}

/// Mutable fusion-sys vector-table builder wrapper.
#[derive(Debug)]
pub struct VectorTableBuilder {
    inner: PlatformVectorBuilder,
}

/// Immutable fusion-sys sealed vector-table wrapper.
#[derive(Debug, Clone, Copy)]
pub struct SealedVectorTable {
    inner: PlatformSealedVectorTable,
}

impl VectorSystem {
    /// Creates a wrapper for the selected platform vector provider.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            inner: pal_system_vector(),
        }
    }

    /// Reports the truthful vector-ownership surface for the selected backend.
    #[must_use]
    pub fn support(&self) -> VectorSupport {
        VectorBase::support(&self.inner)
    }

    /// Returns the current vector-table mode known to the selected backend.
    #[must_use]
    pub fn table_mode(&self) -> VectorTableMode {
        VectorBase::table_mode(&self.inner)
    }

    /// Adopts the current vector table into owned RAM in one explicit mode.
    ///
    /// # Errors
    ///
    /// Returns any honest backend ownership or relocation failure.
    pub fn adopt_and_clone_owned(
        &self,
        mode: VectorTableMode,
    ) -> Result<VectorTableBuilder, VectorError> {
        let builder = VectorOwnershipControl::adopt_and_clone(&self.inner, mode)?;
        Ok(VectorTableBuilder { inner: builder })
    }

    /// Adopts the current vector table into owned RAM in shared-table unified mode.
    ///
    /// # Errors
    ///
    /// Returns any honest backend ownership or relocation failure.
    pub fn adopt_and_clone_shared_owned(&self) -> Result<VectorTableBuilder, VectorError> {
        self.adopt_and_clone_owned(VectorTableMode {
            ownership: VectorOwnershipKind::AdoptedOwned,
            topology: VectorTableTopology::SharedTable,
            domain: self.table_mode().domain,
        })
    }

    /// Adopts the current vector table into owned RAM in one per-core owned mode for the current
    /// active security domain.
    ///
    /// # Errors
    ///
    /// Returns any honest backend ownership or relocation failure.
    pub fn adopt_and_clone_per_core_owned(&self) -> Result<VectorTableBuilder, VectorError> {
        self.adopt_and_clone_owned(VectorTableMode {
            ownership: VectorOwnershipKind::AdoptedOwned,
            topology: VectorTableTopology::PerCoreTables,
            domain: self.table_mode().domain,
        })
    }
}

impl Default for VectorSystem {
    fn default() -> Self {
        Self::new()
    }
}

impl VectorTableBuilder {
    /// Reports the truthful vector support captured by this builder.
    #[must_use]
    pub fn support(&self) -> VectorSupport {
        VectorTableBuilderControl::support(&self.inner)
    }

    /// Returns the active mode of this builder.
    #[must_use]
    pub fn mode(&self) -> VectorTableMode {
        VectorTableBuilderControl::mode(&self.inner)
    }

    /// Binds one slot for ISR-inline execution.
    ///
    /// # Errors
    ///
    /// Returns any honest backend slot-binding failure.
    pub fn bind_inline(
        &mut self,
        slot: IrqSlot,
        priority: Option<VectorPriority>,
        handler: VectorInlineHandler,
    ) -> Result<(), VectorError> {
        self.bind_inline_with_options(
            slot,
            None,
            priority,
            handler,
            VectorInlineStackPolicy::CurrentExceptionStack,
        )
    }

    /// Binds one slot for ISR-inline execution on one explicit core with one explicit stack
    /// policy.
    ///
    /// # Errors
    ///
    /// Returns any honest backend slot-binding failure.
    pub fn bind_inline_with_options(
        &mut self,
        slot: IrqSlot,
        core: Option<ThreadCoreId>,
        priority: Option<VectorPriority>,
        handler: VectorInlineHandler,
        stack: VectorInlineStackPolicy,
    ) -> Result<(), VectorError> {
        self.bind_inline_with_eligibility(slot, core, priority, handler, stack, None)
    }

    /// Binds one slot for ISR-inline execution on one explicit core with one explicit stack
    /// policy and one optional eligibility/fallback contract.
    ///
    /// # Errors
    ///
    /// Returns any honest backend slot-binding failure.
    pub fn bind_inline_with_eligibility(
        &mut self,
        slot: IrqSlot,
        core: Option<ThreadCoreId>,
        priority: Option<VectorPriority>,
        handler: VectorInlineHandler,
        stack: VectorInlineStackPolicy,
        eligibility: Option<VectorInlineEligibility>,
    ) -> Result<(), VectorError> {
        VectorTableBuilderControl::bind(
            &mut self.inner,
            VectorSlotBinding {
                slot,
                core,
                priority,
                target: VectorSlotTarget::Inline {
                    handler,
                    stack,
                    eligibility,
                },
            },
        )
    }

    /// Registers one deferred callback and returns the opaque cookie bound to it.
    ///
    /// # Errors
    ///
    /// Returns an error when the bridge registry is exhausted.
    pub fn register_deferred_callback(
        &mut self,
        callback: VectorDeferredCallback,
    ) -> Result<VectorDispatchCookie, VectorError> {
        let cookie = NEXT_VECTOR_COOKIE.fetch_add(1, Ordering::AcqRel);
        let cookie = VectorDispatchCookie(cookie);
        register_deferred_callback_cookie(cookie, callback)?;
        Ok(cookie)
    }

    /// Registers one deferred callback at one explicit cookie value.
    ///
    /// This is the static-contract path used by generated red inline-fallback bindings: the
    /// analyzer emits a fixed cookie, and the runtime must be able to bind the matching callback
    /// without hoping the monotonic allocator lands on the same number by coincidence.
    ///
    /// # Errors
    ///
    /// Returns an error when the supplied cookie is invalid, exhausted, or already registered.
    pub fn register_deferred_callback_with_cookie(
        &mut self,
        cookie: VectorDispatchCookie,
        callback: VectorDeferredCallback,
    ) -> Result<(), VectorError> {
        register_deferred_callback_cookie(cookie, callback)
    }

    /// Reports whether one deferred callback cookie is currently registered.
    ///
    /// # Errors
    ///
    /// Returns an error when the supplied cookie does not fit the bridge registry.
    pub fn deferred_callback_registered(
        &self,
        cookie: VectorDispatchCookie,
    ) -> Result<bool, VectorError> {
        deferred_callback_registered(cookie)
    }

    /// Unregisters one previously registered deferred callback cookie from the bridge registry.
    ///
    /// # Errors
    ///
    /// Returns an error when the cookie is invalid or currently unbound.
    pub fn unregister_deferred_callback(
        &mut self,
        cookie: VectorDispatchCookie,
    ) -> Result<(), VectorError> {
        unregister_deferred_callback_cookie(cookie)
    }

    /// Binds one slot to one deferred dispatch lane and opaque cookie.
    ///
    /// # Errors
    ///
    /// Returns any honest backend slot-binding failure.
    pub fn bind_deferred(
        &mut self,
        slot: IrqSlot,
        lane: VectorDispatchLane,
        priority: Option<VectorPriority>,
        cookie: VectorDispatchCookie,
    ) -> Result<(), VectorError> {
        self.bind_deferred_with_core(slot, None, lane, priority, cookie)
    }

    /// Binds one slot to one deferred dispatch lane and opaque cookie on one explicit core.
    ///
    /// # Errors
    ///
    /// Returns any honest backend slot-binding failure.
    pub fn bind_deferred_with_core(
        &mut self,
        slot: IrqSlot,
        core: Option<ThreadCoreId>,
        lane: VectorDispatchLane,
        priority: Option<VectorPriority>,
        cookie: VectorDispatchCookie,
    ) -> Result<(), VectorError> {
        VectorTableBuilderControl::bind(
            &mut self.inner,
            VectorSlotBinding {
                slot,
                core,
                priority,
                target: VectorSlotTarget::Deferred { lane, cookie },
            },
        )
    }

    /// Unbinds one peripheral IRQ slot from the owned vector table.
    ///
    /// # Errors
    ///
    /// Returns any honest backend slot-unbind failure.
    pub fn unbind(&mut self, slot: IrqSlot) -> Result<(), VectorError> {
        VectorTableBuilderControl::unbind(&mut self.inner, slot)
    }

    /// Binds one system exception inline.
    ///
    /// # Errors
    ///
    /// Returns any honest backend exception-binding failure.
    pub fn bind_system(
        &mut self,
        exception: SystemException,
        priority: Option<VectorPriority>,
        handler: VectorInlineHandler,
    ) -> Result<(), VectorError> {
        VectorTableBuilderControl::bind_system(
            &mut self.inner,
            VectorSystemBinding {
                exception,
                priority,
                handler,
            },
        )
    }

    /// Binds the reserved `PendSV` deferred-dispatch handler for one owned Cortex-M vector table.
    ///
    /// This is the honest deferred-dispatch path: deferred IRQ trampolines may pend `PendSV` only
    /// after this reserved handler is installed.
    ///
    /// # Errors
    ///
    /// Returns any honest backend ownership, reservation, or priority-programming failure.
    pub fn bind_reserved_pendsv_dispatch(
        &mut self,
        priority: Option<VectorPriority>,
    ) -> Result<(), VectorError> {
        platform_bind_reserved_pendsv_dispatch(
            &mut self.inner,
            priority,
            reserved_pendsv_dispatch_handler,
        )
    }

    /// Seals this builder and returns one immutable runtime handle.
    ///
    /// # Errors
    ///
    /// Returns any honest backend seal-time failure.
    pub fn seal(self) -> Result<SealedVectorTable, VectorError> {
        let inner = VectorTableBuilderControl::seal(self.inner)?;
        Ok(SealedVectorTable { inner })
    }
}

impl SealedVectorTable {
    /// Returns the active mode of this sealed table.
    #[must_use]
    pub fn mode(&self) -> VectorTableMode {
        VectorSealedQuery::mode(&self.inner)
    }

    /// Returns the number of peripheral IRQ slots in this sealed table.
    #[must_use]
    pub fn slot_count(&self) -> u16 {
        VectorSealedQuery::slot_count(&self.inner)
    }

    /// Returns the visible state of one bound slot.
    ///
    /// # Errors
    ///
    /// Returns any honest backend observation failure.
    pub fn slot_state(&self, slot: IrqSlot) -> Result<SlotState, VectorError> {
        VectorSealedQuery::slot_state(&self.inner, slot)
    }

    /// Dispatches all currently pending callbacks from one deferred lane.
    ///
    /// # Errors
    ///
    /// Returns any honest backend pending-state extraction failure.
    pub fn dispatch_pending(&self, lane: VectorDispatchLane) -> Result<usize, VectorError> {
        let mut cookies = [VectorDispatchCookie(0); VECTOR_DEFERRED_REGISTRY_CAPACITY];
        let count = VectorSealedQuery::take_pending(&self.inner, lane, &mut cookies)?;
        for cookie in cookies.into_iter().take(count) {
            dispatch_cookie(cookie)?;
        }
        Ok(count)
    }

    /// Dispatches all currently pending callbacks from the primary deferred lane.
    ///
    /// # Errors
    ///
    /// Returns any honest backend pending-state extraction failure.
    pub fn dispatch_pending_primary(&self) -> Result<usize, VectorError> {
        self.dispatch_pending(VectorDispatchLane::DeferredPrimary)
    }

    /// Dispatches all currently pending callbacks from the secondary deferred lane.
    ///
    /// # Errors
    ///
    /// Returns any honest backend pending-state extraction failure.
    pub fn dispatch_pending_secondary(&self) -> Result<usize, VectorError> {
        self.dispatch_pending(VectorDispatchLane::DeferredSecondary)
    }
}

/// Dispatches deferred cookies from the active owned vector scope for both deferred lanes.
///
/// This is the PendSV-facing path used on Cortex-M after the reserved deferred-dispatch handler
/// has been installed into the owned vector table.
///
/// # Errors
///
/// Returns any honest backend pending-extraction or callback-dispatch failure.
pub fn dispatch_pending_active_scope() -> Result<usize, VectorError> {
    let mut dispatched = 0;
    dispatched += dispatch_pending_active_scope_lane(VectorDispatchLane::DeferredPrimary)?;
    dispatched += dispatch_pending_active_scope_lane(VectorDispatchLane::DeferredSecondary)?;
    Ok(dispatched)
}

fn dispatch_pending_active_scope_lane(lane: VectorDispatchLane) -> Result<usize, VectorError> {
    let mut cookies = [VectorDispatchCookie(0); VECTOR_DEFERRED_REGISTRY_CAPACITY];
    let count = platform_take_pending_active_scope(lane, &mut cookies)?;
    for cookie in cookies.into_iter().take(count) {
        dispatch_cookie(cookie)?;
    }
    Ok(count)
}

unsafe extern "C" fn reserved_pendsv_dispatch_handler() {
    let _ = dispatch_pending_active_scope();
}

fn dispatch_cookie(cookie: VectorDispatchCookie) -> Result<(), VectorError> {
    let index = vector_cookie_index(cookie)?;
    let callback_ptr = VECTOR_CALLBACK_REGISTRY[index].load(Ordering::Acquire);
    if callback_ptr == 0 {
        return Err(VectorError::not_bound());
    }
    let callback: VectorDeferredCallback = unsafe { core::mem::transmute(callback_ptr) };
    callback(cookie);
    Ok(())
}

fn deferred_callback_registered(cookie: VectorDispatchCookie) -> Result<bool, VectorError> {
    let index = vector_cookie_index(cookie)?;
    Ok(VECTOR_CALLBACK_REGISTRY[index].load(Ordering::Acquire) != 0)
}

fn register_deferred_callback_cookie(
    cookie: VectorDispatchCookie,
    callback: VectorDeferredCallback,
) -> Result<(), VectorError> {
    let index = vector_cookie_index(cookie)?;
    let callback_ptr = callback as usize;
    if VECTOR_CALLBACK_REGISTRY[index]
        .compare_exchange(0, callback_ptr, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err(VectorError::state_conflict());
    }
    reserve_vector_cookie(cookie);
    Ok(())
}

fn unregister_deferred_callback_cookie(cookie: VectorDispatchCookie) -> Result<(), VectorError> {
    let index = vector_cookie_index(cookie)?;
    let previous = VECTOR_CALLBACK_REGISTRY[index].swap(0, Ordering::AcqRel);
    if previous == 0 {
        return Err(VectorError::not_bound());
    }
    Ok(())
}

fn vector_cookie_index(cookie: VectorDispatchCookie) -> Result<usize, VectorError> {
    if cookie.0 == 0 {
        return Err(VectorError::invalid());
    }
    let index = usize::try_from(cookie.0 - 1).map_err(|_| VectorError::invalid())?;
    if index >= VECTOR_DEFERRED_REGISTRY_CAPACITY {
        return Err(VectorError::resource_exhausted());
    }
    Ok(index)
}

fn reserve_vector_cookie(cookie: VectorDispatchCookie) {
    let required_next = cookie.0.saturating_add(1);
    let mut observed = NEXT_VECTOR_COOKIE.load(Ordering::Acquire);
    while observed < required_next {
        match NEXT_VECTOR_COOKIE.compare_exchange(
            observed,
            required_next,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => break,
            Err(next_observed) => observed = next_observed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::sync::atomic::{AtomicU32, Ordering};

    static TEST_CALLBACK_HITS: AtomicU32 = AtomicU32::new(0);

    fn test_callback(_cookie: VectorDispatchCookie) {
        TEST_CALLBACK_HITS.fetch_add(1, Ordering::AcqRel);
    }

    #[test]
    fn dispatch_cookie_rejects_zero_cookie() {
        assert!(matches!(
            dispatch_cookie(VectorDispatchCookie(0)),
            Err(error) if error.kind() == VectorErrorKind::Invalid
        ));
    }

    #[test]
    fn dispatch_cookie_runs_registered_callback() {
        TEST_CALLBACK_HITS.store(0, Ordering::Release);

        let cookie_raw = NEXT_VECTOR_COOKIE.fetch_add(1, Ordering::AcqRel);
        assert!(cookie_raw > 0);
        let index = usize::try_from(cookie_raw - 1).expect("cookie index should fit in usize");
        assert!(index < VECTOR_DEFERRED_REGISTRY_CAPACITY);

        let slot = &VECTOR_CALLBACK_REGISTRY[index];
        assert!(
            slot.compare_exchange(
                0,
                test_callback as usize,
                Ordering::AcqRel,
                Ordering::Acquire
            )
            .is_ok(),
            "test callback slot should be empty"
        );

        dispatch_cookie(VectorDispatchCookie(cookie_raw)).expect("registered callback should fire");
        assert_eq!(TEST_CALLBACK_HITS.load(Ordering::Acquire), 1);

        slot.store(0, Ordering::Release);
    }

    #[test]
    fn unregister_deferred_callback_clears_registered_cookie() {
        let cookie_raw = NEXT_VECTOR_COOKIE.fetch_add(1, Ordering::AcqRel);
        assert!(cookie_raw > 0);
        let index = usize::try_from(cookie_raw - 1).expect("cookie index should fit in usize");
        assert!(index < VECTOR_DEFERRED_REGISTRY_CAPACITY);

        let slot = &VECTOR_CALLBACK_REGISTRY[index];
        assert!(
            slot.compare_exchange(
                0,
                test_callback as usize,
                Ordering::AcqRel,
                Ordering::Acquire
            )
            .is_ok(),
            "test callback slot should be empty"
        );

        unregister_deferred_callback_cookie(VectorDispatchCookie(cookie_raw))
            .expect("registered callback should unregister");
        assert_eq!(slot.load(Ordering::Acquire), 0);
        assert!(matches!(
            dispatch_cookie(VectorDispatchCookie(cookie_raw)),
            Err(error) if error.kind() == VectorErrorKind::NotBound
        ));
    }

    #[test]
    fn explicit_cookie_registration_dispatches_and_advances_allocator_floor() {
        TEST_CALLBACK_HITS.store(0, Ordering::Release);

        let cookie = VectorDispatchCookie(27);
        register_deferred_callback_cookie(cookie, test_callback)
            .expect("explicit cookie registration should succeed");
        assert!(
            deferred_callback_registered(cookie).expect("cookie should validate"),
            "explicit cookie should report as registered"
        );

        dispatch_cookie(cookie).expect("explicit cookie should dispatch");
        assert_eq!(TEST_CALLBACK_HITS.load(Ordering::Acquire), 1);
        assert!(
            NEXT_VECTOR_COOKIE.load(Ordering::Acquire) >= 28,
            "allocator floor should advance beyond explicit cookie"
        );
        unregister_deferred_callback_cookie(cookie)
            .expect("explicit cookie should unregister cleanly");
    }

    #[cfg(not(target_os = "none"))]
    #[test]
    fn hosted_vector_system_reports_unsupported_truthfully() {
        let system = VectorSystem::new();
        let support = system.support();

        assert_eq!(
            support.implementation,
            VectorImplementationKind::Unsupported
        );
        assert_eq!(support.slot_count, 0);
        assert_eq!(system.table_mode(), VectorTableMode::unowned_shared());
        assert!(matches!(
            system.adopt_and_clone_shared_owned(),
            Err(error) if error.kind() == VectorErrorKind::Unsupported
        ));
        assert!(matches!(
            dispatch_pending_active_scope(),
            Err(error) if error.kind() == VectorErrorKind::Unsupported
        ));
    }
}
