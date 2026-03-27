//! Cortex-M interrupt-vector ownership backend.

#[cfg(all(
    feature = "cortex-m-vector-secure-world",
    feature = "cortex-m-vector-nonsecure-world"
))]
compile_error!(
    "cortex-m-vector-secure-world and cortex-m-vector-nonsecure-world are mutually exclusive"
);

#[cfg(all(target_arch = "arm", target_os = "none"))]
use core::arch::{asm, global_asm};
use core::cell::UnsafeCell;
use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicUsize, Ordering, compiler_fence};

use crate::contract::hal::vector::{
    IrqSlot,
    SlotState,
    SystemException,
    VectorBase,
    VectorCaps,
    VectorDispatchCookie,
    VectorDispatchLane,
    VectorError,
    VectorInlineEligibility,
    VectorInlineHandler,
    VectorInlineReservedStack,
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
use crate::contract::runtime::thread::ThreadCoreId;
use crate::pal::soc::cortex_m::hal::selected_soc_inline_current_exception_stack_allows;
use crate::pal::soc::cortex_m::hal::selected_soc_irq_implemented_priority_bits;

const SYSTEM_VECTOR_ENTRY_COUNT: usize = 16;
const MAX_IRQ_SLOTS: usize = 64;
const MAX_VECTOR_ENTRIES: usize = SYSTEM_VECTOR_ENTRY_COUNT + MAX_IRQ_SLOTS;
const VECTOR_PENDING_WORDS: usize = MAX_IRQ_SLOTS.div_ceil(32);
const MAX_VECTOR_CORES: usize = 2;
const VECTOR_SCOPE_COUNT: usize = 1 + MAX_VECTOR_CORES;
const SHARED_SCOPE_INDEX: usize = 0;
const CORE_SCOPE_BASE: usize = 1;
const CORTEX_M_INLINE_STACK_ALIGNMENT_BYTES: usize = 8;

const CORTEX_M_SCB_VTOR: *mut usize = 0xE000_ED08 as *mut usize;
const CORTEX_M_SCB_ICSR: *mut u32 = 0xE000_ED04 as *mut u32;
const CORTEX_M_SCB_SHPR: *mut u8 = 0xE000_ED18 as *mut u8;
const CORTEX_M_NVIC_ISPR: *const u32 = 0xE000_E200 as *const u32;
const CORTEX_M_ICSR_PENDSVSET: u32 = 1_u32 << 28;
const CORTEX_M_PENDSV_INDEX: usize = 14;

#[cfg(all(target_arch = "arm", target_os = "none"))]
global_asm!(include_str!("cortex_m_vector_reserved_stack.S"));

const OWNERSHIP_UNOWNED: u8 = 0;
const OWNERSHIP_ADOPTED: u8 = 1;

const TOPOLOGY_SHARED: u8 = 0;
const TOPOLOGY_PER_CORE: u8 = 1;

#[repr(C, align(512))]
struct CortexMOwnedVectorTable([usize; MAX_VECTOR_ENTRIES]);

#[repr(transparent)]
struct ScopeStorageCell<T>(UnsafeCell<T>);

unsafe impl<T> Sync for ScopeStorageCell<T> {}

impl<T> ScopeStorageCell<T> {
    const fn new(value: T) -> Self {
        Self(UnsafeCell::new(value))
    }

    fn get(&self) -> *mut T {
        self.0.get()
    }
}

#[derive(Debug, Clone, Copy)]
struct SlotMeta {
    state: SlotState,
    inline: Option<VectorInlineHandler>,
    inline_stack: VectorInlineStackPolicy,
    inline_eligibility: Option<VectorInlineEligibility>,
}

impl SlotMeta {
    const FOREIGN: Self = Self {
        state: SlotState::Foreign,
        inline: None,
        inline_stack: VectorInlineStackPolicy::CurrentExceptionStack,
        inline_eligibility: None,
    };

    const UNBOUND: Self = Self {
        state: SlotState::Unbound,
        inline: None,
        inline_stack: VectorInlineStackPolicy::CurrentExceptionStack,
        inline_eligibility: None,
    };
}

const fn active_vector_domain() -> VectorSecurityDomain {
    #[cfg(feature = "cortex-m-vector-secure-world")]
    {
        return VectorSecurityDomain::Secure;
    }

    #[cfg(feature = "cortex-m-vector-nonsecure-world")]
    {
        return VectorSecurityDomain::NonSecure;
    }

    #[cfg(not(any(
        feature = "cortex-m-vector-secure-world",
        feature = "cortex-m-vector-nonsecure-world"
    )))]
    {
        VectorSecurityDomain::Unified
    }
}

const fn topology_from_raw(raw: u8) -> VectorTableTopology {
    match raw {
        TOPOLOGY_PER_CORE => VectorTableTopology::PerCoreTables,
        _ => VectorTableTopology::SharedTable,
    }
}

const fn topology_to_raw(topology: VectorTableTopology) -> u8 {
    match topology {
        VectorTableTopology::SharedTable => TOPOLOGY_SHARED,
        VectorTableTopology::PerCoreTables => TOPOLOGY_PER_CORE,
    }
}

const fn mode_from(topology: VectorTableTopology, ownership: u8) -> VectorTableMode {
    VectorTableMode {
        ownership: match ownership {
            OWNERSHIP_ADOPTED => VectorOwnershipKind::AdoptedOwned,
            _ => VectorOwnershipKind::Unowned,
        },
        topology,
        domain: active_vector_domain(),
    }
}

static VECTOR_BUILDER_ACTIVE: [AtomicBool; VECTOR_SCOPE_COUNT] =
    [const { AtomicBool::new(false) }; VECTOR_SCOPE_COUNT];
static ACTIVE_TOPOLOGY: AtomicU8 = AtomicU8::new(TOPOLOGY_SHARED);
static SCOPE_OWNERSHIP: [AtomicU8; VECTOR_SCOPE_COUNT] =
    [const { AtomicU8::new(OWNERSHIP_UNOWNED) }; VECTOR_SCOPE_COUNT];
static SCOPE_SEALED: [AtomicBool; VECTOR_SCOPE_COUNT] =
    [const { AtomicBool::new(false) }; VECTOR_SCOPE_COUNT];
static ORIGINAL_VTOR: [AtomicUsize; VECTOR_SCOPE_COUNT] =
    [const { AtomicUsize::new(0) }; VECTOR_SCOPE_COUNT];
static PRIMARY_PENDING: [[AtomicU32; VECTOR_PENDING_WORDS]; VECTOR_SCOPE_COUNT] =
    [const { [const { AtomicU32::new(0) }; VECTOR_PENDING_WORDS] }; VECTOR_SCOPE_COUNT];
static SECONDARY_PENDING: [[AtomicU32; VECTOR_PENDING_WORDS]; VECTOR_SCOPE_COUNT] =
    [const { [const { AtomicU32::new(0) }; VECTOR_PENDING_WORDS] }; VECTOR_SCOPE_COUNT];

static OWNED_VECTOR_TABLES: [ScopeStorageCell<CortexMOwnedVectorTable>; VECTOR_SCOPE_COUNT] =
    [const { ScopeStorageCell::new(CortexMOwnedVectorTable([0; MAX_VECTOR_ENTRIES])) };
        VECTOR_SCOPE_COUNT];
static ORIGINAL_VECTOR_TABLES: [ScopeStorageCell<CortexMOwnedVectorTable>; VECTOR_SCOPE_COUNT] =
    [const { ScopeStorageCell::new(CortexMOwnedVectorTable([0; MAX_VECTOR_ENTRIES])) };
        VECTOR_SCOPE_COUNT];
static SLOT_META: [ScopeStorageCell<[SlotMeta; MAX_IRQ_SLOTS]>; VECTOR_SCOPE_COUNT] =
    [const { ScopeStorageCell::new([SlotMeta::FOREIGN; MAX_IRQ_SLOTS]) }; VECTOR_SCOPE_COUNT];
static SYSTEM_EXCEPTION_BOUND: [ScopeStorageCell<[bool; SYSTEM_VECTOR_ENTRY_COUNT]>;
    VECTOR_SCOPE_COUNT] =
    [const { ScopeStorageCell::new([false; SYSTEM_VECTOR_ENTRY_COUNT]) }; VECTOR_SCOPE_COUNT];

/// Cortex-M vector provider type.
#[derive(Debug, Clone, Copy, Default)]
pub struct CortexMVector;

/// Mutable builder for one owned Cortex-M vector table.
#[derive(Debug)]
pub struct CortexMVectorBuilder {
    mode: VectorTableMode,
    slot_count: u16,
    scope_index: usize,
    core: Option<ThreadCoreId>,
    armed: bool,
}

/// Immutable sealed Cortex-M vector-table handle.
#[derive(Debug, Clone, Copy)]
pub struct CortexMSealedVectorTable {
    mode: VectorTableMode,
    slot_count: u16,
    scope_index: usize,
    core: Option<ThreadCoreId>,
}

/// Selected Cortex-M vector provider type.
pub type PlatformVector = CortexMVector;
/// Selected Cortex-M vector builder type.
pub type PlatformVectorBuilder = CortexMVectorBuilder;
/// Selected Cortex-M sealed vector-table type.
pub type PlatformSealedVectorTable = CortexMSealedVectorTable;

/// Returns the selected Cortex-M vector provider.
#[must_use]
pub const fn system_vector() -> PlatformVector {
    PlatformVector::new()
}

impl CortexMVector {
    /// Creates a new Cortex-M vector provider handle.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl VectorBase for CortexMVector {
    fn support(&self) -> VectorSupport {
        let slot_count =
            u16::try_from(crate::pal::soc::cortex_m::hal::soc::board::irqs().len()).unwrap_or(0);
        if slot_count == 0 || usize::from(slot_count) > MAX_IRQ_SLOTS {
            return VectorSupport::unsupported();
        }

        let mut caps = VectorCaps::ADOPT_AND_CLONE
            | VectorCaps::PRIORITY_CONTROL
            | VectorCaps::PENDING_CONTROL
            | VectorCaps::SEAL
            | VectorCaps::INLINE_DISPATCH
            | VectorCaps::DEFERRED_PRIMARY
            | VectorCaps::DEFERRED_SECONDARY;

        if per_core_tables_supported() {
            caps |= VectorCaps::PER_CORE_TABLES;
        }

        match active_vector_domain() {
            VectorSecurityDomain::Secure => caps |= VectorCaps::SECURE_WORLD,
            VectorSecurityDomain::NonSecure => caps |= VectorCaps::NON_SECURE_WORLD,
            VectorSecurityDomain::Unified => {}
        }

        VectorSupport {
            caps,
            implementation: crate::contract::caps::ImplementationKind::Native,
            slot_count,
            implemented_priority_bits: selected_soc_irq_implemented_priority_bits(),
        }
    }

    fn table_mode(&self) -> VectorTableMode {
        let topology = active_topology();
        let scope_index = active_scope_index_for_topology(topology).unwrap_or(SHARED_SCOPE_INDEX);
        mode_from(
            topology,
            SCOPE_OWNERSHIP[scope_index].load(Ordering::Acquire),
        )
    }

    fn slot_count(&self) -> u16 {
        self.support().slot_count
    }

    fn slot_state(&self, slot: IrqSlot) -> Result<SlotState, VectorError> {
        let topology = active_topology();
        let scope_index =
            active_scope_index_for_topology(topology).ok_or_else(VectorError::core_mismatch)?;
        slot_state_for(scope_index, slot, self.slot_count())
    }

    fn core_affinity(&self, slot: IrqSlot) -> Result<Option<ThreadCoreId>, VectorError> {
        validate_slot(slot, self.slot_count())?;
        match active_topology() {
            VectorTableTopology::SharedTable => Ok(None),
            VectorTableTopology::PerCoreTables => Ok(Some(current_core_id()?)),
        }
    }
}

impl VectorOwnershipControl for CortexMVector {
    type Builder = CortexMVectorBuilder;

    fn adopt_and_clone(&self, mode: VectorTableMode) -> Result<Self::Builder, VectorError> {
        let support = self.support();
        if !support.caps.contains(VectorCaps::ADOPT_AND_CLONE) {
            return Err(VectorError::unsupported());
        }
        if mode.ownership != VectorOwnershipKind::AdoptedOwned {
            return Err(VectorError::unsupported());
        }
        if mode.domain != active_vector_domain() {
            return Err(VectorError::world_mismatch());
        }
        if mode.topology == VectorTableTopology::PerCoreTables
            && !support.caps.contains(VectorCaps::PER_CORE_TABLES)
        {
            return Err(VectorError::unsupported());
        }

        let scope = match mode.topology {
            VectorTableTopology::SharedTable => (SHARED_SCOPE_INDEX, None),
            VectorTableTopology::PerCoreTables => {
                let core = match current_core_id() {
                    Ok(core) => core,
                    Err(error) => {
                        return Err(error);
                    }
                };
                let scope_index = match scope_index_for_core(core) {
                    Ok(scope_index) => scope_index,
                    Err(error) => {
                        return Err(error);
                    }
                };
                (scope_index, Some(core))
            }
        };

        if VECTOR_BUILDER_ACTIVE[scope.0]
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(VectorError::state_conflict());
        }

        if let Err(error) = validate_topology_request(mode.topology) {
            VECTOR_BUILDER_ACTIVE[scope.0].store(false, Ordering::Release);
            return Err(error);
        }
        if SCOPE_SEALED[scope.0].load(Ordering::Acquire)
            || SCOPE_OWNERSHIP[scope.0].load(Ordering::Acquire) == OWNERSHIP_ADOPTED
        {
            VECTOR_BUILDER_ACTIVE[scope.0].store(false, Ordering::Release);
            return Err(VectorError::state_conflict());
        }

        let slot_count = support.slot_count;
        if let Err(error) = adopt_scope(scope.0, slot_count) {
            VECTOR_BUILDER_ACTIVE[scope.0].store(false, Ordering::Release);
            return Err(error);
        }

        ACTIVE_TOPOLOGY.store(topology_to_raw(mode.topology), Ordering::Release);
        SCOPE_OWNERSHIP[scope.0].store(OWNERSHIP_ADOPTED, Ordering::Release);

        Ok(CortexMVectorBuilder {
            mode,
            slot_count,
            scope_index: scope.0,
            core: scope.1,
            armed: true,
        })
    }

    fn overlay_builder(&self, _mode: VectorTableMode) -> Result<Self::Builder, VectorError> {
        Err(VectorError::unsupported())
    }
}

impl VectorTableBuilderControl for CortexMVectorBuilder {
    type Sealed = CortexMSealedVectorTable;

    fn support(&self) -> VectorSupport {
        CortexMVector::new().support()
    }

    fn mode(&self) -> VectorTableMode {
        self.mode
    }

    fn bind(&mut self, binding: VectorSlotBinding) -> Result<(), VectorError> {
        validate_slot(binding.slot, self.slot_count)?;
        validate_slot_domain(binding.slot, self.mode.domain)?;
        let slot_index = usize::from(binding.slot.0);
        let core = resolve_binding_core(binding.core, self.mode.topology, self.core)?;

        validate_inline_stack_policy(self.scope_index, binding.target)?;

        if !matches!(
            read_slot_meta(self.scope_index, slot_index).state,
            SlotState::Foreign | SlotState::Unbound
        ) {
            return Err(VectorError::already_bound());
        }

        if let Some(priority) = binding.priority {
            crate::pal::soc::cortex_m::hal::soc::board::irq_set_priority(
                binding.slot.0,
                priority.0,
            )
            .map_err(map_hardware_error)?;
        }

        match binding.target {
            VectorSlotTarget::Inline {
                handler,
                stack,
                eligibility,
            } => {
                if eligibility.is_some_and(|eligibility| {
                    eligibility.fallback_lane == VectorDispatchLane::Inline
                }) {
                    return Err(VectorError::invalid());
                }
                let mut meta = read_slot_meta(self.scope_index, slot_index);
                meta.inline = Some(handler);
                meta.inline_stack = stack;
                meta.inline_eligibility = eligibility;
                meta.state = SlotState::Inline {
                    priority: binding.priority,
                    core,
                    stack: stack.kind(),
                };
                write_slot_meta(self.scope_index, slot_index, meta);
            }
            VectorSlotTarget::Deferred { lane, cookie } => {
                if lane == VectorDispatchLane::Inline {
                    return Err(VectorError::invalid());
                }
                let mut meta = read_slot_meta(self.scope_index, slot_index);
                meta.inline = None;
                meta.inline_stack = VectorInlineStackPolicy::CurrentExceptionStack;
                meta.inline_eligibility = None;
                meta.state = SlotState::Deferred {
                    lane,
                    cookie,
                    priority: binding.priority,
                    core,
                };
                write_slot_meta(self.scope_index, slot_index, meta);
            }
        }

        write_slot_vector_entry(
            self.scope_index,
            binding.slot,
            irq_trampoline_for_slot(slot_index),
        );
        Ok(())
    }

    fn bind_system(&mut self, binding: VectorSystemBinding) -> Result<(), VectorError> {
        if binding.exception == SystemException::PendSv {
            return Err(VectorError::reserved());
        }
        if binding.exception == SystemException::SecureFault
            && self.mode.domain != VectorSecurityDomain::Secure
        {
            return Err(VectorError::world_mismatch());
        }

        let index = system_exception_index(binding.exception).ok_or_else(VectorError::reserved)?;
        if read_system_exception_bound(self.scope_index, index) {
            return Err(VectorError::already_bound());
        }

        if let Some(priority) = binding.priority {
            set_system_exception_priority(binding.exception, priority)?;
        }

        write_owned_vector_entry(self.scope_index, index, binding.handler as usize);
        write_system_exception_bound(self.scope_index, index, true);
        Ok(())
    }

    fn unbind(&mut self, slot: IrqSlot) -> Result<(), VectorError> {
        validate_slot(slot, self.slot_count)?;
        validate_slot_domain(slot, self.mode.domain)?;
        let slot_index = usize::from(slot.0);
        if matches!(
            read_slot_meta(self.scope_index, slot_index).state,
            SlotState::Foreign | SlotState::Unbound
        ) {
            return Err(VectorError::not_bound());
        }

        write_slot_meta(self.scope_index, slot_index, SlotMeta::UNBOUND);
        let entry_index = SYSTEM_VECTOR_ENTRY_COUNT + slot_index;
        write_owned_vector_entry(
            self.scope_index,
            entry_index,
            read_original_vector_entry(self.scope_index, entry_index),
        );
        clear_slot_pending(self.scope_index, slot_index);
        Ok(())
    }

    fn seal(mut self) -> Result<Self::Sealed, VectorError> {
        VECTOR_BUILDER_ACTIVE[self.scope_index].store(false, Ordering::Release);
        SCOPE_SEALED[self.scope_index].store(true, Ordering::Release);
        let sealed = CortexMSealedVectorTable {
            mode: self.mode,
            slot_count: self.slot_count,
            scope_index: self.scope_index,
            core: self.core,
        };
        self.armed = false;
        Ok(sealed)
    }
}

impl Drop for CortexMVectorBuilder {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        rollback_scope(self.scope_index, self.slot_count);
        VECTOR_BUILDER_ACTIVE[self.scope_index].store(false, Ordering::Release);
    }
}

impl VectorSealedQuery for CortexMSealedVectorTable {
    fn slot_state(&self, slot: IrqSlot) -> Result<SlotState, VectorError> {
        slot_state_for(self.scope_index, slot, self.slot_count)
    }

    fn slot_count(&self) -> u16 {
        self.slot_count
    }

    fn mode(&self) -> VectorTableMode {
        self.mode
    }

    fn core_affinity(&self, slot: IrqSlot) -> Result<Option<ThreadCoreId>, VectorError> {
        validate_slot(slot, self.slot_count)?;
        Ok(self.core)
    }

    fn is_pending(&self, slot: IrqSlot) -> Result<bool, VectorError> {
        validate_slot(slot, self.slot_count)?;
        if !slot_matches_domain(slot, self.mode.domain)? {
            return Ok(false);
        }

        let slot_index = usize::from(slot.0);
        match read_slot_meta(self.scope_index, slot_index).state {
            SlotState::Deferred { lane, .. } => Ok(match lane {
                VectorDispatchLane::DeferredPrimary => {
                    pending_word(&PRIMARY_PENDING[self.scope_index], slot_index)
                }
                VectorDispatchLane::DeferredSecondary => {
                    pending_word(&SECONDARY_PENDING[self.scope_index], slot_index)
                }
                VectorDispatchLane::Inline => false,
            }),
            SlotState::Inline { .. } => {
                let deferred = match read_slot_meta(self.scope_index, slot_index).inline_eligibility
                {
                    Some(eligibility) => match eligibility.fallback_lane {
                        VectorDispatchLane::DeferredPrimary => {
                            pending_word(&PRIMARY_PENDING[self.scope_index], slot_index)
                        }
                        VectorDispatchLane::DeferredSecondary => {
                            pending_word(&SECONDARY_PENDING[self.scope_index], slot_index)
                        }
                        VectorDispatchLane::Inline => false,
                    },
                    None => false,
                };
                Ok(irq_pending(slot.0) || deferred)
            }
            SlotState::Foreign | SlotState::Reserved | SlotState::Unbound => Ok(false),
        }
    }

    fn take_pending(
        &self,
        lane: VectorDispatchLane,
        output: &mut [VectorDispatchCookie],
    ) -> Result<usize, VectorError> {
        match lane {
            VectorDispatchLane::DeferredPrimary => take_pending_cookies(
                self.scope_index,
                VectorDispatchLane::DeferredPrimary,
                &PRIMARY_PENDING[self.scope_index],
                self.slot_count,
                output,
            ),
            VectorDispatchLane::DeferredSecondary => take_pending_cookies(
                self.scope_index,
                VectorDispatchLane::DeferredSecondary,
                &SECONDARY_PENDING[self.scope_index],
                self.slot_count,
                output,
            ),
            VectorDispatchLane::Inline => Err(VectorError::invalid()),
        }
    }
}

/// Binds the reserved PendSV deferred-dispatch handler into one owned Cortex-M vector table.
///
/// # Errors
///
/// Returns any honest ownership, state, or priority-programming failure.
pub fn bind_reserved_pendsv_dispatch(
    builder: &mut PlatformVectorBuilder,
    priority: Option<VectorPriority>,
    handler: VectorInlineHandler,
) -> Result<(), VectorError> {
    let index =
        system_exception_index(SystemException::PendSv).ok_or_else(VectorError::reserved)?;
    if let Some(priority) = priority {
        set_system_exception_priority(SystemException::PendSv, priority)?;
    }

    let installed = read_owned_vector_entry(builder.scope_index, index);
    if read_system_exception_bound(builder.scope_index, index) {
        if installed == handler as usize {
            return Ok(());
        }
        return Err(VectorError::state_conflict());
    }

    write_owned_vector_entry(builder.scope_index, index, handler as usize);
    write_system_exception_bound(builder.scope_index, index, true);
    Ok(())
}

/// Takes pending deferred-dispatch cookies from the active owned Cortex-M vector scope.
///
/// # Errors
///
/// Returns any honest scope-resolution or pending-extraction failure.
pub fn take_pending_active_scope(
    lane: VectorDispatchLane,
    output: &mut [VectorDispatchCookie],
) -> Result<usize, VectorError> {
    let topology = active_topology();
    let scope_index =
        active_scope_index_for_topology(topology).ok_or_else(VectorError::core_mismatch)?;
    if !scope_owned(scope_index) {
        return Err(VectorError::state_conflict());
    }
    let slot_count = u16::try_from(crate::pal::soc::cortex_m::hal::soc::board::irqs().len())
        .map_err(|_| VectorError::invalid())?;
    match lane {
        VectorDispatchLane::DeferredPrimary => take_pending_cookies(
            scope_index,
            VectorDispatchLane::DeferredPrimary,
            &PRIMARY_PENDING[scope_index],
            slot_count,
            output,
        ),
        VectorDispatchLane::DeferredSecondary => take_pending_cookies(
            scope_index,
            VectorDispatchLane::DeferredSecondary,
            &SECONDARY_PENDING[scope_index],
            slot_count,
            output,
        ),
        VectorDispatchLane::Inline => Err(VectorError::invalid()),
    }
}

const fn map_hardware_error(error: crate::contract::hal::HardwareError) -> VectorError {
    match error.kind() {
        crate::contract::hal::HardwareErrorKind::Unsupported => VectorError::unsupported(),
        crate::contract::hal::HardwareErrorKind::Invalid => VectorError::invalid(),
        crate::contract::hal::HardwareErrorKind::ResourceExhausted => {
            VectorError::resource_exhausted()
        }
        crate::contract::hal::HardwareErrorKind::StateConflict => VectorError::state_conflict(),
        crate::contract::hal::HardwareErrorKind::Busy => VectorError::state_conflict(),
        crate::contract::hal::HardwareErrorKind::Platform(code) => VectorError::platform(code),
    }
}

fn per_core_tables_supported() -> bool {
    crate::pal::soc::cortex_m::hal::soc::board::selected_soc()
        .topology_summary
        .and_then(|summary| summary.core_count)
        .is_some_and(|count| count > 1)
}

fn active_topology() -> VectorTableTopology {
    topology_from_raw(ACTIVE_TOPOLOGY.load(Ordering::Acquire))
}

fn scope_owned(scope_index: usize) -> bool {
    SCOPE_OWNERSHIP[scope_index].load(Ordering::Acquire) == OWNERSHIP_ADOPTED
}

fn validate_topology_request(topology: VectorTableTopology) -> Result<(), VectorError> {
    if topology == VectorTableTopology::PerCoreTables && !per_core_tables_supported() {
        return Err(VectorError::unsupported());
    }

    let mut any_scope_owned = false;
    for ownership in &SCOPE_OWNERSHIP {
        if ownership.load(Ordering::Acquire) == OWNERSHIP_ADOPTED {
            any_scope_owned = true;
            break;
        }
    }
    if !any_scope_owned {
        for sealed in &SCOPE_SEALED {
            if sealed.load(Ordering::Acquire) {
                any_scope_owned = true;
                break;
            }
        }
    }
    if !any_scope_owned {
        return Ok(());
    }

    if active_topology() == topology {
        Ok(())
    } else {
        Err(VectorError::state_conflict())
    }
}

fn current_core_id() -> Result<ThreadCoreId, VectorError> {
    let observation = crate::pal::soc::cortex_m::hal::soc::board::current_execution_location()
        .map_err(|_| VectorError::core_mismatch())?;
    observation
        .location
        .core
        .ok_or_else(VectorError::core_mismatch)
}

fn scope_index_for_core(core: ThreadCoreId) -> Result<usize, VectorError> {
    let core_index = usize::try_from(core.0).map_err(|_| VectorError::core_mismatch())?;
    if core_index >= MAX_VECTOR_CORES {
        return Err(VectorError::core_mismatch());
    }
    Ok(CORE_SCOPE_BASE + core_index)
}

fn active_scope_index_for_topology(topology: VectorTableTopology) -> Option<usize> {
    match topology {
        VectorTableTopology::SharedTable => Some(SHARED_SCOPE_INDEX),
        VectorTableTopology::PerCoreTables => current_core_id()
            .ok()
            .and_then(|core| scope_index_for_core(core).ok()),
    }
}

fn read_slot_meta(scope_index: usize, slot_index: usize) -> SlotMeta {
    unsafe { (*SLOT_META[scope_index].get())[slot_index] }
}

fn write_slot_meta(scope_index: usize, slot_index: usize, value: SlotMeta) {
    unsafe {
        (*SLOT_META[scope_index].get())[slot_index] = value;
    }
}

fn read_system_exception_bound(scope_index: usize, index: usize) -> bool {
    unsafe { (*SYSTEM_EXCEPTION_BOUND[scope_index].get())[index] }
}

fn write_system_exception_bound(scope_index: usize, index: usize, value: bool) {
    unsafe {
        (*SYSTEM_EXCEPTION_BOUND[scope_index].get())[index] = value;
    }
}

fn read_owned_vector_entry(scope_index: usize, entry_index: usize) -> usize {
    unsafe { (*OWNED_VECTOR_TABLES[scope_index].get()).0[entry_index] }
}

fn write_owned_vector_entry(scope_index: usize, entry_index: usize, value: usize) {
    unsafe {
        (*OWNED_VECTOR_TABLES[scope_index].get()).0[entry_index] = value;
    }
}

fn write_original_vector_entry(scope_index: usize, entry_index: usize, value: usize) {
    unsafe {
        (*ORIGINAL_VECTOR_TABLES[scope_index].get()).0[entry_index] = value;
    }
}

fn read_original_vector_entry(scope_index: usize, entry_index: usize) -> usize {
    unsafe { (*ORIGINAL_VECTOR_TABLES[scope_index].get()).0[entry_index] }
}

fn adopt_scope(scope_index: usize, slot_count: u16) -> Result<(), VectorError> {
    let _guard = CortexMInterruptMaskGuard::enter();
    let entry_count = SYSTEM_VECTOR_ENTRY_COUNT + usize::from(slot_count);
    // SAFETY: VTOR is the architected vector-table base register for the active execution domain.
    let current_vtor = unsafe { ptr::read_volatile(CORTEX_M_SCB_VTOR) };
    ORIGINAL_VTOR[scope_index].store(current_vtor, Ordering::Release);
    let current_table = current_vtor as *const usize;
    for index in 0..entry_count {
        // SAFETY: the active vector table is one contiguous array of word-sized entries.
        let entry = unsafe { ptr::read_volatile(current_table.add(index)) };
        write_owned_vector_entry(scope_index, index, entry);
        write_original_vector_entry(scope_index, index, entry);
    }
    for slot in 0..usize::from(slot_count) {
        write_slot_meta(scope_index, slot, SlotMeta::FOREIGN);
    }
    for index in 0..SYSTEM_VECTOR_ENTRY_COUNT {
        write_system_exception_bound(scope_index, index, false);
    }
    clear_pending_words(scope_index);
    // SAFETY: the owned table is one statically allocated RAM image sized for this backend.
    unsafe {
        ptr::write_volatile(
            CORTEX_M_SCB_VTOR,
            core::ptr::addr_of!((*OWNED_VECTOR_TABLES[scope_index].get()).0) as usize,
        )
    };
    cortex_m_data_sync_barrier();
    cortex_m_instruction_sync_barrier();
    Ok(())
}

fn validate_slot(slot: IrqSlot, slot_count: u16) -> Result<(), VectorError> {
    if slot.0 >= slot_count {
        Err(VectorError::invalid())
    } else {
        Ok(())
    }
}

fn slot_descriptor(
    slot: IrqSlot,
) -> Result<&'static crate::pal::soc::cortex_m::hal::soc::board::CortexMIrqDescriptor, VectorError>
{
    crate::pal::soc::cortex_m::hal::soc::board::irqs()
        .get(usize::from(slot.0))
        .ok_or_else(VectorError::invalid)
}

fn slot_matches_domain(slot: IrqSlot, domain: VectorSecurityDomain) -> Result<bool, VectorError> {
    let descriptor = slot_descriptor(slot)?;
    Ok(match domain {
        VectorSecurityDomain::Unified => true,
        VectorSecurityDomain::Secure => !descriptor.nonsecure,
        VectorSecurityDomain::NonSecure => descriptor.nonsecure,
    })
}

fn validate_slot_domain(slot: IrqSlot, domain: VectorSecurityDomain) -> Result<(), VectorError> {
    if slot_matches_domain(slot, domain)? {
        Ok(())
    } else {
        Err(VectorError::world_mismatch())
    }
}

fn resolve_binding_core(
    requested: Option<ThreadCoreId>,
    topology: VectorTableTopology,
    builder_core: Option<ThreadCoreId>,
) -> Result<Option<ThreadCoreId>, VectorError> {
    match topology {
        VectorTableTopology::SharedTable => {
            if requested.is_some() {
                Err(VectorError::core_mismatch())
            } else {
                Ok(None)
            }
        }
        VectorTableTopology::PerCoreTables => {
            let expected = builder_core.ok_or_else(VectorError::core_mismatch)?;
            match requested {
                Some(core) if core != expected => Err(VectorError::core_mismatch()),
                _ => Ok(Some(expected)),
            }
        }
    }
}

fn slot_state_for(
    scope_index: usize,
    slot: IrqSlot,
    slot_count: u16,
) -> Result<SlotState, VectorError> {
    validate_slot(slot, slot_count)?;
    if SCOPE_OWNERSHIP[scope_index].load(Ordering::Acquire) != OWNERSHIP_ADOPTED {
        return Ok(SlotState::Foreign);
    }
    if !slot_matches_domain(slot, active_vector_domain())? {
        return Ok(SlotState::Foreign);
    }
    Ok(read_slot_meta(scope_index, usize::from(slot.0)).state)
}

fn system_exception_index(exception: SystemException) -> Option<usize> {
    Some(match exception {
        SystemException::Nmi => 2,
        SystemException::HardFault => 3,
        SystemException::MemManage => 4,
        SystemException::BusFault => 5,
        SystemException::UsageFault => 6,
        SystemException::SecureFault => 7,
        SystemException::SVCall => 11,
        SystemException::PendSv => 14,
        SystemException::SysTick => 15,
    })
}

fn set_system_exception_priority(
    exception: SystemException,
    priority: VectorPriority,
) -> Result<(), VectorError> {
    let Some(index) = system_exception_index(exception) else {
        return Err(VectorError::reserved());
    };
    if index < 4 {
        return Err(VectorError::reserved());
    }
    // SAFETY: SHPR bytes are the architected writable priority fields for configurable
    // system exceptions. Writing one byte updates only that exception's priority.
    unsafe { ptr::write_volatile(CORTEX_M_SCB_SHPR.add(index - 4), priority.0) };
    Ok(())
}

fn write_slot_vector_entry(scope_index: usize, slot: IrqSlot, trampoline: VectorInlineHandler) {
    let slot_index = usize::from(slot.0);
    compiler_fence(Ordering::Release);
    cortex_m_data_memory_barrier();
    write_owned_vector_entry(
        scope_index,
        SYSTEM_VECTOR_ENTRY_COUNT + slot_index,
        trampoline as usize,
    );
}

fn clear_pending_words(scope_index: usize) {
    for word in &PRIMARY_PENDING[scope_index] {
        word.store(0, Ordering::Release);
    }
    for word in &SECONDARY_PENDING[scope_index] {
        word.store(0, Ordering::Release);
    }
}

fn clear_slot_pending(scope_index: usize, slot_index: usize) {
    let mask = !(1_u32 << (slot_index % 32));
    PRIMARY_PENDING[scope_index][slot_index / 32].fetch_and(mask, Ordering::AcqRel);
    SECONDARY_PENDING[scope_index][slot_index / 32].fetch_and(mask, Ordering::AcqRel);
}

fn pending_word(words: &[AtomicU32; VECTOR_PENDING_WORDS], slot_index: usize) -> bool {
    (words[slot_index / 32].load(Ordering::Acquire) & (1_u32 << (slot_index % 32))) != 0
}

fn take_pending_cookies(
    scope_index: usize,
    lane: VectorDispatchLane,
    words: &[AtomicU32; VECTOR_PENDING_WORDS],
    slot_count: u16,
    output: &mut [VectorDispatchCookie],
) -> Result<usize, VectorError> {
    let mut written = 0;
    for slot in 0..usize::from(slot_count) {
        if written >= output.len() {
            break;
        }
        let mask = 1_u32 << (slot % 32);
        let word = &words[slot / 32];
        if (word.load(Ordering::Acquire) & mask) == 0 {
            continue;
        }
        let previous = word.fetch_and(!mask, Ordering::AcqRel);
        if (previous & mask) == 0 {
            continue;
        }
        let meta = read_slot_meta(scope_index, slot);
        match meta.state {
            SlotState::Deferred {
                cookie,
                lane: slot_lane,
                ..
            } if slot_lane == lane => {
                output[written] = cookie;
                written += 1;
            }
            SlotState::Inline { .. } => {
                if let Some(eligibility) = meta.inline_eligibility
                    && eligibility.fallback_lane == lane
                {
                    output[written] = eligibility.fallback_cookie;
                    written += 1;
                }
            }
            SlotState::Foreign
            | SlotState::Reserved
            | SlotState::Unbound
            | SlotState::Deferred { .. } => {}
        }
    }
    Ok(written)
}

fn irq_pending(irqn: u16) -> bool {
    let register_index = usize::from(irqn / 32);
    let bit = u32::from(irqn % 32);
    // SAFETY: NVIC ISPR is the architected interrupt-pending register block. Reading one word
    // snapshots the pending state for its 32-line window without mutating Rust-managed memory.
    let pending = unsafe { ptr::read_volatile(CORTEX_M_NVIC_ISPR.add(register_index)) };
    (pending & (1_u32 << bit)) != 0
}

fn dispatch_irq_slot(slot_index: usize) {
    let Some(scope_index) = active_scope_index_for_topology(active_topology()) else {
        return;
    };
    let meta = read_slot_meta(scope_index, slot_index);
    match meta.state {
        SlotState::Inline { .. } => {
            if let Some(handler) = meta.inline {
                let stack = meta.inline_stack;
                if let Some(eligibility) = meta.inline_eligibility {
                    if !inline_stack_allows(
                        stack,
                        eligibility.required_current_exception_stack_bytes,
                    ) || !unsafe { (eligibility.allow_now)(eligibility.context) }
                    {
                        mark_slot_pending(scope_index, slot_index, eligibility.fallback_lane);
                        return;
                    }
                }
                // SAFETY: only one owned-table builder can install inline handlers into this
                // scope's adopted vector table, and the installed slot state stores the matching
                // stack policy for the trampoline to honor.
                unsafe { call_inline_handler(handler, stack) };
            }
        }
        SlotState::Deferred { lane, .. } => {
            mark_slot_pending(scope_index, slot_index, lane);
        }
        SlotState::Foreign | SlotState::Reserved | SlotState::Unbound => {}
    }
}

fn inline_stack_allows(
    stack: VectorInlineStackPolicy,
    required_current_exception_stack_bytes: usize,
) -> bool {
    match stack {
        VectorInlineStackPolicy::CurrentExceptionStack => {
            if required_current_exception_stack_bytes == 0 {
                true
            } else {
                selected_soc_inline_current_exception_stack_allows(
                    required_current_exception_stack_bytes,
                )
            }
        }
        VectorInlineStackPolicy::DedicatedReserved(_) => true,
    }
}

fn validate_inline_stack_policy(
    scope_index: usize,
    target: VectorSlotTarget,
) -> Result<(), VectorError> {
    let stack = match target {
        VectorSlotTarget::Inline { stack, .. } => stack,
        VectorSlotTarget::Deferred { .. } => return Ok(()),
    };
    match stack {
        VectorInlineStackPolicy::CurrentExceptionStack => Ok(()),
        VectorInlineStackPolicy::DedicatedReserved(reserved) => {
            let Some(top) = reserved.checked_top() else {
                return Err(VectorError::invalid());
            };
            if top % CORTEX_M_INLINE_STACK_ALIGNMENT_BYTES != 0 {
                return Err(VectorError::invalid());
            }
            if reserved_stack_in_use(scope_index, reserved) {
                return Err(VectorError::state_conflict());
            }
            Ok(())
        }
    }
}

fn mark_slot_pending(scope_index: usize, slot_index: usize, lane: VectorDispatchLane) {
    let words = match lane {
        VectorDispatchLane::DeferredPrimary => &PRIMARY_PENDING[scope_index],
        VectorDispatchLane::DeferredSecondary => &SECONDARY_PENDING[scope_index],
        VectorDispatchLane::Inline => return,
    };
    let word = &words[slot_index / 32];
    word.fetch_or(1_u32 << (slot_index % 32), Ordering::AcqRel);
    if read_system_exception_bound(scope_index, CORTEX_M_PENDSV_INDEX) {
        pend_pendsv();
    }
}

fn pend_pendsv() {
    // SAFETY: ICSR is the architected interrupt-control register. Writing PENDSVSET requests one
    // deferred PendSV exception without mutating Rust-owned memory.
    unsafe { ptr::write_volatile(CORTEX_M_SCB_ICSR, CORTEX_M_ICSR_PENDSVSET) };
}

fn rollback_scope(scope_index: usize, slot_count: u16) {
    let _guard = CortexMInterruptMaskGuard::enter();
    let entry_count = SYSTEM_VECTOR_ENTRY_COUNT + usize::from(slot_count);
    let original_vtor = ORIGINAL_VTOR[scope_index].load(Ordering::Acquire);

    for slot in 0..usize::from(slot_count) {
        write_slot_meta(scope_index, slot, SlotMeta::FOREIGN);
    }
    for index in 0..SYSTEM_VECTOR_ENTRY_COUNT {
        write_system_exception_bound(scope_index, index, false);
    }
    for index in 0..entry_count {
        write_owned_vector_entry(
            scope_index,
            index,
            read_original_vector_entry(scope_index, index),
        );
    }
    clear_pending_words(scope_index);
    SCOPE_OWNERSHIP[scope_index].store(OWNERSHIP_UNOWNED, Ordering::Release);
    SCOPE_SEALED[scope_index].store(false, Ordering::Release);

    if original_vtor != 0 {
        // SAFETY: VTOR is the architected vector-table base register for the active execution
        // domain. Restoring the captured pre-adoption value undoes one unsealed adopt-and-clone.
        unsafe { ptr::write_volatile(CORTEX_M_SCB_VTOR, original_vtor) };
        cortex_m_data_sync_barrier();
        cortex_m_instruction_sync_barrier();
    }
    if !any_scope_active() {
        ACTIVE_TOPOLOGY.store(TOPOLOGY_SHARED, Ordering::Release);
    }
}

fn any_scope_active() -> bool {
    for scope_index in 0..VECTOR_SCOPE_COUNT {
        if SCOPE_OWNERSHIP[scope_index].load(Ordering::Acquire) == OWNERSHIP_ADOPTED
            || SCOPE_SEALED[scope_index].load(Ordering::Acquire)
        {
            return true;
        }
    }
    false
}

fn reserved_stack_in_use(_scope_index: usize, reserved: VectorInlineReservedStack) -> bool {
    for candidate_scope in 0..VECTOR_SCOPE_COUNT {
        for slot_index in 0..MAX_IRQ_SLOTS {
            if matches!(
                read_slot_meta(candidate_scope, slot_index).state,
                SlotState::Foreign | SlotState::Unbound
            ) {
                continue;
            }
            let VectorInlineStackPolicy::DedicatedReserved(bound) =
                read_slot_meta(candidate_scope, slot_index).inline_stack
            else {
                continue;
            };
            if reserved_stacks_overlap(bound, reserved) {
                return true;
            }
        }
    }
    false
}

fn reserved_stacks_overlap(
    left: VectorInlineReservedStack,
    right: VectorInlineReservedStack,
) -> bool {
    let left_base = left.base.as_ptr() as usize;
    let right_base = right.base.as_ptr() as usize;
    let Some(left_top) = left.checked_top() else {
        return true;
    };
    let Some(right_top) = right.checked_top() else {
        return true;
    };
    left_base < right_top && right_base < left_top
}

#[cfg(all(target_arch = "arm", target_os = "none"))]
struct CortexMInterruptMaskGuard {
    primask: u32,
}

#[cfg(all(target_arch = "arm", target_os = "none"))]
impl CortexMInterruptMaskGuard {
    #[inline]
    fn enter() -> Self {
        let primask: u32;
        unsafe {
            asm!("mrs {0}, PRIMASK", out(reg) primask, options(nomem, nostack, preserves_flags));
            asm!("cpsid i", options(nomem, nostack, preserves_flags));
        }
        compiler_fence(Ordering::SeqCst);
        Self { primask }
    }
}

#[cfg(all(target_arch = "arm", target_os = "none"))]
impl Drop for CortexMInterruptMaskGuard {
    fn drop(&mut self) {
        compiler_fence(Ordering::SeqCst);
        unsafe {
            asm!(
                "msr PRIMASK, {0}",
                in(reg) self.primask,
                options(nomem, nostack, preserves_flags)
            );
        }
    }
}

#[cfg(not(all(target_arch = "arm", target_os = "none")))]
struct CortexMInterruptMaskGuard;

#[cfg(not(all(target_arch = "arm", target_os = "none")))]
impl CortexMInterruptMaskGuard {
    #[inline]
    const fn enter() -> Self {
        Self
    }
}

#[cfg(all(target_arch = "arm", target_os = "none"))]
#[inline]
fn cortex_m_data_sync_barrier() {
    unsafe { asm!("dsb", options(nomem, nostack, preserves_flags)) };
}

#[cfg(not(all(target_arch = "arm", target_os = "none")))]
#[inline]
const fn cortex_m_data_sync_barrier() {}

#[cfg(all(target_arch = "arm", target_os = "none"))]
#[inline]
fn cortex_m_instruction_sync_barrier() {
    unsafe { asm!("isb", options(nomem, nostack, preserves_flags)) };
}

#[cfg(not(all(target_arch = "arm", target_os = "none")))]
#[inline]
const fn cortex_m_instruction_sync_barrier() {}

#[cfg(all(target_arch = "arm", target_os = "none"))]
#[inline]
fn cortex_m_data_memory_barrier() {
    unsafe { asm!("dmb", options(nomem, nostack, preserves_flags)) };
}

#[cfg(not(all(target_arch = "arm", target_os = "none")))]
#[inline]
const fn cortex_m_data_memory_barrier() {}

unsafe fn call_inline_handler(handler: VectorInlineHandler, stack: VectorInlineStackPolicy) {
    match stack {
        VectorInlineStackPolicy::CurrentExceptionStack => unsafe { handler() },
        VectorInlineStackPolicy::DedicatedReserved(reserved) => unsafe {
            if let Some(stack_top) = reserved.checked_top() {
                call_inline_handler_on_reserved_stack(handler, stack_top);
            } else {
                handler();
            }
        },
    }
}

#[cfg(all(target_arch = "arm", target_os = "none"))]
unsafe extern "C" {
    fn fusion_pal_cortex_m_call_inline_handler_on_reserved_stack(handler: usize, stack_top: usize);
}

#[cfg(all(target_arch = "arm", target_os = "none"))]
unsafe fn call_inline_handler_on_reserved_stack(handler: VectorInlineHandler, stack_top: usize) {
    unsafe {
        fusion_pal_cortex_m_call_inline_handler_on_reserved_stack(handler as usize, stack_top)
    };
}

#[cfg(not(all(target_arch = "arm", target_os = "none")))]
unsafe fn call_inline_handler_on_reserved_stack(handler: VectorInlineHandler, _stack_top: usize) {
    unsafe { handler() };
}

unsafe extern "C" fn irq_trampoline<const SLOT: usize>() {
    dispatch_irq_slot(SLOT);
}

macro_rules! irq_trampoline_array {
    ($($slot:literal),* $(,)?) => {
        [$(irq_trampoline::<$slot>,)*]
    };
}

const IRQ_TRAMPOLINES: [VectorInlineHandler; MAX_IRQ_SLOTS] = irq_trampoline_array!(
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25,
    26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49,
    50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63,
);

fn irq_trampoline_for_slot(slot_index: usize) -> VectorInlineHandler {
    IRQ_TRAMPOLINES[slot_index]
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::num::NonZeroUsize;
    use core::ptr;
    use core::ptr::NonNull;
    use core::sync::atomic::AtomicUsize;
    use std::sync::Mutex;

    static VECTOR_TEST_LOCK: Mutex<()> = Mutex::new(());
    static INLINE_HANDLER_HITS: AtomicUsize = AtomicUsize::new(0);
    static ELIGIBILITY_ALLOW: AtomicBool = AtomicBool::new(true);
    static mut ALIGNED_RESERVED_STACK: [u64; 8] = [0; 8];
    static mut MISALIGNED_RESERVED_STACK: [u64; 9] = [0; 9];

    unsafe extern "C" fn test_inline_handler() {
        INLINE_HANDLER_HITS.fetch_add(1, Ordering::AcqRel);
    }

    unsafe extern "C" fn test_inline_eligibility(_context: *const ()) -> bool {
        ELIGIBILITY_ALLOW.load(Ordering::Acquire)
    }

    fn reset_vector_test_state() {
        for active in &VECTOR_BUILDER_ACTIVE {
            active.store(false, Ordering::Release);
        }
        ACTIVE_TOPOLOGY.store(TOPOLOGY_SHARED, Ordering::Release);
        INLINE_HANDLER_HITS.store(0, Ordering::Release);
        ELIGIBILITY_ALLOW.store(true, Ordering::Release);
        for ownership in &SCOPE_OWNERSHIP {
            ownership.store(OWNERSHIP_UNOWNED, Ordering::Release);
        }
        for sealed in &SCOPE_SEALED {
            sealed.store(false, Ordering::Release);
        }
        for scope in 0..VECTOR_SCOPE_COUNT {
            clear_pending_words(scope);
            for slot in 0..MAX_IRQ_SLOTS {
                write_slot_meta(scope, slot, SlotMeta::FOREIGN);
            }
        }
    }

    fn test_builder() -> CortexMVectorBuilder {
        CortexMVectorBuilder {
            mode: VectorTableMode {
                ownership: VectorOwnershipKind::AdoptedOwned,
                topology: VectorTableTopology::SharedTable,
                domain: active_vector_domain(),
            },
            slot_count: u16::try_from(crate::pal::soc::cortex_m::hal::soc::board::irqs().len())
                .expect("test board slot count should fit in u16"),
            scope_index: SHARED_SCOPE_INDEX,
            core: None,
            armed: false,
        }
    }

    #[test]
    fn shared_owned_scope_blocks_per_core_topology_switch() {
        let _guard = VECTOR_TEST_LOCK
            .lock()
            .expect("vector test lock should acquire");
        reset_vector_test_state();

        SCOPE_OWNERSHIP[SHARED_SCOPE_INDEX].store(OWNERSHIP_ADOPTED, Ordering::Release);
        ACTIVE_TOPOLOGY.store(TOPOLOGY_SHARED, Ordering::Release);

        assert!(matches!(
            validate_topology_request(VectorTableTopology::PerCoreTables),
            Err(error) if error.kind() == VectorError::state_conflict().kind()
        ));
    }

    #[test]
    fn pending_cookies_are_isolated_per_scope() {
        let _guard = VECTOR_TEST_LOCK
            .lock()
            .expect("vector test lock should acquire");
        reset_vector_test_state();

        SCOPE_OWNERSHIP[SHARED_SCOPE_INDEX].store(OWNERSHIP_ADOPTED, Ordering::Release);
        SCOPE_OWNERSHIP[CORE_SCOPE_BASE].store(OWNERSHIP_ADOPTED, Ordering::Release);
        let mut shared_meta = read_slot_meta(SHARED_SCOPE_INDEX, 3);
        shared_meta.state = SlotState::Deferred {
            lane: VectorDispatchLane::DeferredPrimary,
            cookie: VectorDispatchCookie(11),
            priority: None,
            core: None,
        };
        write_slot_meta(SHARED_SCOPE_INDEX, 3, shared_meta);
        let mut core_meta = read_slot_meta(CORE_SCOPE_BASE, 3);
        core_meta.state = SlotState::Deferred {
            lane: VectorDispatchLane::DeferredPrimary,
            cookie: VectorDispatchCookie(29),
            priority: None,
            core: Some(ThreadCoreId(0)),
        };
        write_slot_meta(CORE_SCOPE_BASE, 3, core_meta);
        PRIMARY_PENDING[SHARED_SCOPE_INDEX][0].store(1_u32 << 3, Ordering::Release);
        PRIMARY_PENDING[CORE_SCOPE_BASE][0].store(1_u32 << 3, Ordering::Release);

        let mut shared = [VectorDispatchCookie(0); 1];
        let shared_count = take_pending_cookies(
            SHARED_SCOPE_INDEX,
            VectorDispatchLane::DeferredPrimary,
            &PRIMARY_PENDING[SHARED_SCOPE_INDEX],
            8,
            &mut shared,
        )
        .expect("shared scope extraction should succeed");
        assert_eq!(shared_count, 1);
        assert_eq!(shared[0], VectorDispatchCookie(11));
        assert!(pending_word(&PRIMARY_PENDING[CORE_SCOPE_BASE], 3));
    }

    #[test]
    fn active_scope_pending_requires_owned_scope() {
        let _guard = VECTOR_TEST_LOCK
            .lock()
            .expect("vector test lock should acquire");
        reset_vector_test_state();

        let mut output = [VectorDispatchCookie(0); 1];
        assert!(matches!(
            take_pending_active_scope(VectorDispatchLane::DeferredPrimary, &mut output),
            Err(error) if error.kind() == VectorError::state_conflict().kind()
        ));
    }

    #[test]
    fn bind_rejects_misaligned_dedicated_reserved_stack() {
        let _guard = VECTOR_TEST_LOCK
            .lock()
            .expect("vector test lock should acquire");
        reset_vector_test_state();
        SCOPE_OWNERSHIP[SHARED_SCOPE_INDEX].store(OWNERSHIP_ADOPTED, Ordering::Release);

        let mut builder = test_builder();
        let stack = unsafe {
            VectorInlineReservedStack {
                base: NonNull::new_unchecked(
                    core::ptr::addr_of_mut!(MISALIGNED_RESERVED_STACK)
                        .cast::<u8>()
                        .add(1),
                ),
                size_bytes: NonZeroUsize::new(64).expect("test stack size should be non-zero"),
            }
        };

        let result = builder.bind(VectorSlotBinding {
            slot: IrqSlot(5),
            core: None,
            priority: None,
            target: VectorSlotTarget::Inline {
                handler: test_inline_handler,
                stack: VectorInlineStackPolicy::DedicatedReserved(stack),
                eligibility: None,
            },
        });

        assert!(matches!(
            result,
            Err(error) if error.kind() == VectorError::invalid().kind()
        ));
    }

    #[test]
    fn bind_accepts_aligned_dedicated_reserved_stack() {
        let _guard = VECTOR_TEST_LOCK
            .lock()
            .expect("vector test lock should acquire");
        reset_vector_test_state();
        SCOPE_OWNERSHIP[SHARED_SCOPE_INDEX].store(OWNERSHIP_ADOPTED, Ordering::Release);

        let mut builder = test_builder();
        let stack = unsafe {
            VectorInlineReservedStack {
                base: NonNull::new_unchecked(
                    core::ptr::addr_of_mut!(ALIGNED_RESERVED_STACK).cast::<u8>(),
                ),
                size_bytes: NonZeroUsize::new(core::mem::size_of::<[u64; 8]>())
                    .expect("test stack size should be non-zero"),
            }
        };

        builder
            .bind(VectorSlotBinding {
                slot: IrqSlot(5),
                core: None,
                priority: None,
                target: VectorSlotTarget::Inline {
                    handler: test_inline_handler,
                    stack: VectorInlineStackPolicy::DedicatedReserved(stack),
                    eligibility: None,
                },
            })
            .expect("aligned dedicated reserved stack should bind cleanly");

        assert!(matches!(
            read_slot_meta(SHARED_SCOPE_INDEX, 5).state,
            SlotState::Inline {
                stack: VectorInlineStackKind::DedicatedReserved,
                ..
            }
        ));
    }

    #[test]
    fn bind_rejects_duplicate_dedicated_reserved_stack_window() {
        let _guard = VECTOR_TEST_LOCK
            .lock()
            .expect("vector test lock should acquire");
        reset_vector_test_state();
        SCOPE_OWNERSHIP[SHARED_SCOPE_INDEX].store(OWNERSHIP_ADOPTED, Ordering::Release);

        let mut builder = test_builder();
        let stack = unsafe {
            VectorInlineReservedStack {
                base: NonNull::new_unchecked(
                    core::ptr::addr_of_mut!(ALIGNED_RESERVED_STACK).cast::<u8>(),
                ),
                size_bytes: NonZeroUsize::new(core::mem::size_of::<[u64; 8]>())
                    .expect("test stack size should be non-zero"),
            }
        };

        builder
            .bind(VectorSlotBinding {
                slot: IrqSlot(5),
                core: None,
                priority: None,
                target: VectorSlotTarget::Inline {
                    handler: test_inline_handler,
                    stack: VectorInlineStackPolicy::DedicatedReserved(stack),
                    eligibility: None,
                },
            })
            .expect("first dedicated reserved stack bind should succeed");

        let result = builder.bind(VectorSlotBinding {
            slot: IrqSlot(6),
            core: None,
            priority: None,
            target: VectorSlotTarget::Inline {
                handler: test_inline_handler,
                stack: VectorInlineStackPolicy::DedicatedReserved(stack),
                eligibility: None,
            },
        });

        assert!(matches!(
            result,
            Err(error) if error.kind() == VectorError::state_conflict().kind()
        ));
    }

    #[test]
    fn inline_slot_runs_immediately_when_eligibility_allows_it() {
        let _guard = VECTOR_TEST_LOCK
            .lock()
            .expect("vector test lock should acquire");
        reset_vector_test_state();

        SCOPE_OWNERSHIP[SHARED_SCOPE_INDEX].store(OWNERSHIP_ADOPTED, Ordering::Release);
        let mut meta = read_slot_meta(SHARED_SCOPE_INDEX, 5);
        meta.state = SlotState::Inline {
            priority: None,
            core: None,
            stack: VectorInlineStackKind::CurrentExceptionStack,
        };
        meta.inline = Some(test_inline_handler);
        meta.inline_stack = VectorInlineStackPolicy::CurrentExceptionStack;
        meta.inline_eligibility = Some(VectorInlineEligibility {
            context: ptr::null(),
            allow_now: test_inline_eligibility,
            required_current_exception_stack_bytes: 0,
            fallback_lane: VectorDispatchLane::DeferredPrimary,
            fallback_cookie: VectorDispatchCookie(7),
        });
        write_slot_meta(SHARED_SCOPE_INDEX, 5, meta);

        ELIGIBILITY_ALLOW.store(true, Ordering::Release);
        dispatch_irq_slot(5);

        assert_eq!(INLINE_HANDLER_HITS.load(Ordering::Acquire), 1);
        assert!(!pending_word(&PRIMARY_PENDING[SHARED_SCOPE_INDEX], 5));
    }

    #[test]
    fn inline_slot_defers_when_eligibility_blocks_it() {
        let _guard = VECTOR_TEST_LOCK
            .lock()
            .expect("vector test lock should acquire");
        reset_vector_test_state();

        SCOPE_OWNERSHIP[SHARED_SCOPE_INDEX].store(OWNERSHIP_ADOPTED, Ordering::Release);
        let mut meta = read_slot_meta(SHARED_SCOPE_INDEX, 5);
        meta.state = SlotState::Inline {
            priority: None,
            core: None,
            stack: VectorInlineStackKind::CurrentExceptionStack,
        };
        meta.inline = Some(test_inline_handler);
        meta.inline_stack = VectorInlineStackPolicy::CurrentExceptionStack;
        meta.inline_eligibility = Some(VectorInlineEligibility {
            context: ptr::null(),
            allow_now: test_inline_eligibility,
            required_current_exception_stack_bytes: 0,
            fallback_lane: VectorDispatchLane::DeferredPrimary,
            fallback_cookie: VectorDispatchCookie(41),
        });
        write_slot_meta(SHARED_SCOPE_INDEX, 5, meta);

        ELIGIBILITY_ALLOW.store(false, Ordering::Release);
        dispatch_irq_slot(5);

        assert_eq!(INLINE_HANDLER_HITS.load(Ordering::Acquire), 0);
        assert!(pending_word(&PRIMARY_PENDING[SHARED_SCOPE_INDEX], 5));

        let mut output = [VectorDispatchCookie(0); 1];
        let count = take_pending_cookies(
            SHARED_SCOPE_INDEX,
            VectorDispatchLane::DeferredPrimary,
            &PRIMARY_PENDING[SHARED_SCOPE_INDEX],
            8,
            &mut output,
        )
        .expect("deferred fallback extraction should succeed");
        assert_eq!(count, 1);
        assert_eq!(output[0], VectorDispatchCookie(41));
    }
}
