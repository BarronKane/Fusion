//! Cortex-M bare-metal context backend.
//!
//! This backend performs direct cooperative context switching on caller-supplied stacks by
//! saving the Cortex-M callee-saved register set. It is intentionally same-carrier only: no
//! scheduler pageantry, just honest stackful switching.

use core::arch::global_asm;
use core::mem;

use crate::contract::pal::runtime::context::{
    ContextAuthoritySet,
    ContextBaseContract,
    ContextCaps,
    ContextError,
    ContextGuarantee,
    ContextImplementationKind,
    ContextMigrationSupport,
    ContextStackDirection,
    ContextStackLayout,
    ContextSupport,
    ContextSwitch,
    ContextTlsIsolation,
    RawContextEntry,
};

#[cfg(target_abi = "eabihf")]
global_asm!(include_str!("cortex_m_hardfloat.S"));
#[cfg(not(target_abi = "eabihf"))]
global_asm!(include_str!("cortex_m_softfloat.S"));

const STACK_ALIGNMENT: usize = 8;
const RED_ZONE_BYTES: usize = 0;

const CORTEX_M_CONTEXT_SUPPORT: ContextSupport = ContextSupport {
    caps: ContextCaps::MAKE
        .union(ContextCaps::SWAP)
        .union(ContextCaps::STACK_DIRECTION)
        .union(ContextCaps::TLS_ISOLATION),
    guarantee: ContextGuarantee::Enforced,
    structural_stack_overhead_bytes: 0,
    min_stack_alignment: STACK_ALIGNMENT,
    red_zone_bytes: RED_ZONE_BYTES,
    stack_direction: ContextStackDirection::Down,
    guard_required: false,
    tls_isolation: ContextTlsIsolation::SharedCarrierThread,
    signal_mask_preserved: false,
    unwind_across_boundary: false,
    migration: ContextMigrationSupport::SameCarrierOnly,
    authorities: ContextAuthoritySet::ISA,
    implementation: ContextImplementationKind::Native,
};

/// Cortex-M context provider type.
#[derive(Debug, Clone, Copy, Default)]
pub struct CortexMContext;

#[repr(C)]
#[derive(Debug, Default, PartialEq, Eq, Hash)]
struct CortexMRegisters {
    sp: usize,
    r4: usize,
    r5: usize,
    r6: usize,
    r7: usize,
    r8: usize,
    r9: usize,
    r10: usize,
    r11: usize,
    lr: usize,
    #[cfg(target_abi = "eabihf")]
    fpscr: u32,
    #[cfg(target_abi = "eabihf")]
    _fp_padding: u32,
    #[cfg(target_abi = "eabihf")]
    d8: u64,
    #[cfg(target_abi = "eabihf")]
    d9: u64,
    #[cfg(target_abi = "eabihf")]
    d10: u64,
    #[cfg(target_abi = "eabihf")]
    d11: u64,
    #[cfg(target_abi = "eabihf")]
    d12: u64,
    #[cfg(target_abi = "eabihf")]
    d13: u64,
    #[cfg(target_abi = "eabihf")]
    d14: u64,
    #[cfg(target_abi = "eabihf")]
    d15: u64,
}

/// Cortex-M saved-context record.
#[derive(Debug, Default, PartialEq, Eq, Hash)]
pub struct CortexMSavedContext {
    registers: CortexMRegisters,
    ready: bool,
}

unsafe extern "C" {
    fn fusion_cortex_m_context_swap(from: *mut CortexMRegisters, to: *const CortexMRegisters);
    fn fusion_cortex_m_context_start() -> !;
}

/// Selected Cortex-M context provider type.
pub type PlatformContext = CortexMContext;
/// Selected Cortex-M saved-context type.
pub type PlatformSavedContext = CortexMSavedContext;

/// Returns the selected Cortex-M context provider.
#[must_use]
pub const fn system_context() -> PlatformContext {
    PlatformContext::new()
}

/// Returns the selected Cortex-M context support truth.
#[must_use]
pub const fn system_context_support() -> ContextSupport {
    CORTEX_M_CONTEXT_SUPPORT
}

impl CortexMContext {
    /// Creates a new Cortex-M context provider handle.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl ContextBaseContract for CortexMContext {
    type Context = CortexMSavedContext;

    fn support(&self) -> ContextSupport {
        CORTEX_M_CONTEXT_SUPPORT
    }
}

#[unsafe(no_mangle)]
extern "C" fn fusion_cortex_m_context_bootstrap(entry_bits: usize, arg_bits: usize) -> ! {
    let entry = unsafe { mem::transmute::<usize, RawContextEntry>(entry_bits) };
    let arg = arg_bits as *mut ();
    unsafe { entry(arg) }
}

// SAFETY: this backend reports a direct same-carrier cooperative context switch and preserves the
// Cortex-M callee-saved register set (plus hard-float preserved state where applicable) in a
// backend-owned record.
unsafe impl ContextSwitch for CortexMContext {
    unsafe fn make(
        &self,
        stack: ContextStackLayout,
        entry: RawContextEntry,
        arg: *mut (),
    ) -> Result<Self::Context, ContextError> {
        let sp = validate_stack_layout(stack)?;
        let mut saved = CortexMSavedContext::default();

        saved.registers.sp = sp;
        saved.registers.r4 = entry as usize;
        saved.registers.r5 = arg as usize;
        saved.registers.lr = fusion_cortex_m_context_start as *const () as usize;
        #[cfg(target_abi = "eabihf")]
        {
            saved.registers.fpscr = capture_fpscr();
        }
        saved.ready = true;

        Ok(saved)
    }

    unsafe fn swap(
        &self,
        from: &mut Self::Context,
        to: &Self::Context,
    ) -> Result<(), ContextError> {
        if !to.ready {
            return Err(ContextError::invalid());
        }

        from.ready = true;
        unsafe {
            fusion_cortex_m_context_swap(&raw mut from.registers, &raw const to.registers);
        }
        Ok(())
    }
}

fn validate_stack_layout(stack: ContextStackLayout) -> Result<usize, ContextError> {
    let top = stack
        .base
        .addr()
        .get()
        .checked_add(stack.len.get())
        .ok_or_else(ContextError::invalid)?;

    if top % STACK_ALIGNMENT != 0 {
        return Err(ContextError::invalid());
    }
    if stack.len.get() < STACK_ALIGNMENT {
        return Err(ContextError::invalid());
    }

    Ok(top)
}

#[cfg(target_abi = "eabihf")]
fn capture_fpscr() -> u32 {
    let fpscr: u32;
    unsafe {
        core::arch::asm!(
            "vmrs {fpscr}, fpscr",
            fpscr = lateout(reg) fpscr,
            options(nostack, preserves_flags),
        );
    }
    fpscr
}
