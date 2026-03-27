//! Shared interrupt-vector ownership identifiers and descriptor vocabulary.

use core::num::NonZeroUsize;
use core::ptr::NonNull;

use crate::contract::runtime::thread::ThreadCoreId;

/// One peripheral IRQ slot index, zero-based from the first external interrupt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IrqSlot(pub u16);

/// One opaque deferred-dispatch cookie surfaced by the PAL vector layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VectorDispatchCookie(pub u32);

/// One raw hardware interrupt-priority byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VectorPriority(pub u8);

/// One low-level dispatch lane surfaced by the PAL vector layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VectorDispatchLane {
    /// ISR-inline execution.
    Inline,
    /// Deferred dispatch lane A.
    DeferredPrimary,
    /// Deferred dispatch lane B.
    DeferredSecondary,
}

/// Ownership strength for the active vector-table mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VectorOwnershipKind {
    /// Fusion does not own the current table.
    Unowned,
    /// Fusion is cooperating with an external table owner.
    OverlayCompatible,
    /// Fusion adopted and now owns the current hardware table.
    AdoptedOwned,
}

/// Physical topology of the active vector-table arrangement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VectorTableTopology {
    /// One shared table for the active execution domain.
    SharedTable,
    /// One independent table per core.
    PerCoreTables,
}

/// Protection or security domain of the active vector table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VectorSecurityDomain {
    /// One unified table with no exposed secure/non-secure split.
    Unified,
    /// One secure-world table.
    Secure,
    /// One non-secure table.
    NonSecure,
}

/// Complete mode of the active vector-table arrangement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VectorTableMode {
    /// Current ownership model.
    pub ownership: VectorOwnershipKind,
    /// Current topology.
    pub topology: VectorTableTopology,
    /// Current protection domain.
    pub domain: VectorSecurityDomain,
}

impl VectorTableMode {
    /// Returns one unowned shared-table mode.
    #[must_use]
    pub const fn unowned_shared() -> Self {
        Self {
            ownership: VectorOwnershipKind::Unowned,
            topology: VectorTableTopology::SharedTable,
            domain: VectorSecurityDomain::Unified,
        }
    }
}

/// System exceptions modeled separately from peripheral IRQ slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SystemException {
    Nmi,
    HardFault,
    MemManage,
    BusFault,
    UsageFault,
    SVCall,
    PendSv,
    SysTick,
    SecureFault,
}

/// Function pointer executed inline in one vector slot or system exception.
pub type VectorInlineHandler = unsafe extern "C" fn();
/// Opaque inline-eligibility predicate evaluated by the PAL before running one inline slot.
pub type VectorInlineEligibilityFn = unsafe extern "C" fn(*const ()) -> bool;

/// Public classification of how one inline handler uses stack storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VectorInlineStackKind {
    /// Run entirely on the current exception stack.
    CurrentExceptionStack,
    /// Switch to one dedicated reserved stack before entering the inline handler body.
    DedicatedReserved,
}

/// One concrete reserved stack window for an inline vector handler.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VectorInlineReservedStack {
    /// Lowest byte of the reserved stack window.
    pub base: NonNull<u8>,
    /// Size of the reserved window in bytes.
    pub size_bytes: NonZeroUsize,
}

impl VectorInlineReservedStack {
    /// Returns the exclusive top-of-stack address for this reserved window when it fits in one
    /// machine address.
    #[must_use]
    pub fn checked_top(self) -> Option<usize> {
        (self.base.as_ptr() as usize).checked_add(self.size_bytes.get())
    }

    /// Returns the exclusive top-of-stack address for this reserved window.
    ///
    /// # Panics
    ///
    /// Panics if the reserved stack window would overflow the active machine address width.
    #[must_use]
    pub fn top(self) -> usize {
        self.checked_top()
            .expect("reserved inline stack top should fit in the active address width")
    }
}

/// Requested stack policy for one inline vector binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VectorInlineStackPolicy {
    /// Use the current exception stack only.
    CurrentExceptionStack,
    /// Switch to one dedicated reserved stack before entering the handler body.
    DedicatedReserved(VectorInlineReservedStack),
}

impl VectorInlineStackPolicy {
    /// Returns the public stack-usage kind for this policy.
    #[must_use]
    pub const fn kind(self) -> VectorInlineStackKind {
        match self {
            Self::CurrentExceptionStack => VectorInlineStackKind::CurrentExceptionStack,
            Self::DedicatedReserved(_) => VectorInlineStackKind::DedicatedReserved,
        }
    }
}

/// Opaque eligibility contract for one owned inline vector slot.
#[derive(Debug, Clone, Copy)]
pub struct VectorInlineEligibility {
    /// Opaque runtime context passed back to the predicate.
    pub context: *const (),
    /// Predicate deciding whether inline execution is currently allowed.
    pub allow_now: VectorInlineEligibilityFn,
    /// Minimum current-exception stack headroom required before inline execution may run on the
    /// live exception stack. `0` means “no additional current-stack requirement”.
    pub required_current_exception_stack_bytes: usize,
    /// Deferred lane used when inline execution is currently incompatible.
    pub fallback_lane: VectorDispatchLane,
    /// Opaque deferred cookie surfaced when inline execution falls back.
    pub fallback_cookie: VectorDispatchCookie,
}

/// One concrete target bound to one vector slot.
#[derive(Debug, Clone, Copy)]
pub enum VectorSlotTarget {
    /// One ISR-inline handler.
    Inline {
        handler: VectorInlineHandler,
        stack: VectorInlineStackPolicy,
        eligibility: Option<VectorInlineEligibility>,
    },
    /// One deferred-dispatch cookie routed to one deferred lane.
    Deferred {
        lane: VectorDispatchLane,
        cookie: VectorDispatchCookie,
    },
}

/// One mutable slot-binding request.
#[derive(Debug, Clone, Copy)]
pub struct VectorSlotBinding {
    /// Slot being configured.
    pub slot: IrqSlot,
    /// Optional owning core in per-core modes.
    pub core: Option<ThreadCoreId>,
    /// Raw hardware interrupt priority to apply, when one is provided.
    pub priority: Option<VectorPriority>,
    /// Bound dispatch target.
    pub target: VectorSlotTarget,
}

/// One system-exception binding request.
#[derive(Debug, Clone, Copy)]
pub struct VectorSystemBinding {
    /// System exception being configured.
    pub exception: SystemException,
    /// Raw hardware/system priority to apply, when one is provided.
    pub priority: Option<VectorPriority>,
    /// Inline handler entered through the bound exception.
    pub handler: VectorInlineHandler,
}

/// Visible state of one peripheral IRQ slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SlotState {
    /// Slot is not bound by Fusion.
    Unbound,
    /// Slot is bound inline.
    Inline {
        /// Raw hardware interrupt priority currently applied, when visible.
        priority: Option<VectorPriority>,
        /// Optional owning core in per-core modes.
        core: Option<ThreadCoreId>,
        /// Public stack shape of the inline binding.
        stack: VectorInlineStackKind,
    },
    /// Slot is bound for deferred dispatch.
    Deferred {
        /// Deferred lane carrying the dispatch cookie.
        lane: VectorDispatchLane,
        /// Opaque deferred-dispatch cookie surfaced when the slot fires.
        cookie: VectorDispatchCookie,
        /// Raw hardware interrupt priority currently applied, when visible.
        priority: Option<VectorPriority>,
        /// Optional owning core in per-core modes.
        core: Option<ThreadCoreId>,
    },
    /// Slot is bound by a foreign owner.
    Foreign,
    /// Slot is reserved for platform use.
    Reserved,
}
