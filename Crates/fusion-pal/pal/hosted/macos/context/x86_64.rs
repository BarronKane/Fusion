//! Native `x86_64` macOS context-switch backend.
//!
//! This backend saves the SysV callee-saved register set plus floating-point control words
//! in an ISA-specific record and resumes contexts with a small assembly shim.

use core::arch::{asm, global_asm};
use core::mem;

use crate::contract::pal::runtime::context::{
    ContextAuthoritySet,
    ContextBase,
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

global_asm!(include_str!("x86_64.S"));

const STACK_ALIGNMENT: usize = 16;
const RED_ZONE_BYTES: usize = 128;
const STRUCTURAL_STACK_OVERHEAD_BYTES: usize = 352;

const X86_64_CONTEXT_SUPPORT: ContextSupport = ContextSupport {
    caps: ContextCaps::MAKE
        .union(ContextCaps::SWAP)
        .union(ContextCaps::STACK_DIRECTION)
        .union(ContextCaps::TLS_ISOLATION)
        .union(ContextCaps::CROSS_CARRIER_RESUME)
        .union(ContextCaps::SIGNAL_MASK_PRESERVED)
        .union(ContextCaps::GUARD_REQUIRED),
    guarantee: ContextGuarantee::Enforced,
    structural_stack_overhead_bytes: STRUCTURAL_STACK_OVERHEAD_BYTES,
    min_stack_alignment: STACK_ALIGNMENT,
    red_zone_bytes: RED_ZONE_BYTES,
    stack_direction: ContextStackDirection::Down,
    guard_required: false,
    tls_isolation: ContextTlsIsolation::SharedCarrierThread,
    signal_mask_preserved: false,
    unwind_across_boundary: false,
    migration: ContextMigrationSupport::CrossCarrier,
    authorities: ContextAuthoritySet::ISA.union(ContextAuthoritySet::OPERATING_SYSTEM),
    implementation: ContextImplementationKind::Native,
};

/// Native `x86_64` macOS context provider.
#[derive(Debug, Clone, Copy, Default)]
pub struct MacOsContext;

#[repr(C)]
#[derive(Debug, Default)]
struct X86_64Registers {
    rsp: usize,
    rbx: usize,
    rbp: usize,
    r12: usize,
    r13: usize,
    r14: usize,
    r15: usize,
    mxcsr: u32,
    x87_cw: u16,
    _padding: u16,
}

/// Saved `x86_64` execution context.
#[derive(Debug, Default)]
pub struct MacOsSavedContext {
    registers: X86_64Registers,
    ready: bool,
}

unsafe extern "C" {
    fn fusion_macos_x86_64_context_swap(from: *mut X86_64Registers, to: *const X86_64Registers);
}

impl MacOsContext {
    /// Creates a new `x86_64` macOS context provider.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl MacOsSavedContext {
    /// Returns an empty capture slot ready to receive a saved context on first swap.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            registers: X86_64Registers {
                rsp: 0,
                rbx: 0,
                rbp: 0,
                r12: 0,
                r13: 0,
                r14: 0,
                r15: 0,
                mxcsr: 0,
                x87_cw: 0,
                _padding: 0,
            },
            ready: false,
        }
    }
}

impl ContextBase for MacOsContext {
    type Context = MacOsSavedContext;

    fn support(&self) -> ContextSupport {
        X86_64_CONTEXT_SUPPORT
    }
}

// SAFETY: this backend saves and restores the reported x86_64 context record directly and the
// saved state is carrier-agnostic process memory, so suspended contexts may resume on a
// different carrier while still sharing that carrier thread's TLS domain.
unsafe impl ContextSwitch for MacOsContext {
    unsafe fn make(
        &self,
        stack: ContextStackLayout,
        entry: RawContextEntry,
        arg: *mut (),
    ) -> Result<Self::Context, ContextError> {
        let bootstrap_rsp = validate_stack_layout(stack)?;
        let mut saved = MacOsSavedContext::empty();
        let (mxcsr, x87_cw) = capture_control_state();

        saved.registers.rsp = bootstrap_rsp;
        saved.registers.r12 = entry as usize;
        saved.registers.r13 = arg as usize;
        saved.registers.mxcsr = mxcsr;
        saved.registers.x87_cw = x87_cw;
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
            fusion_macos_x86_64_context_swap(&raw mut from.registers, &raw const to.registers);
        }
        Ok(())
    }
}

#[unsafe(no_mangle)]
extern "C" fn fusion_macos_x86_64_context_bootstrap(entry_bits: usize, arg_bits: usize) -> ! {
    let entry = unsafe { mem::transmute::<usize, RawContextEntry>(entry_bits) };
    let arg = arg_bits as *mut ();
    unsafe { entry(arg) }
}

fn validate_stack_layout(stack: ContextStackLayout) -> Result<usize, ContextError> {
    let top = stack
        .base
        .addr()
        .get()
        .checked_add(stack.len.get())
        .ok_or_else(ContextError::invalid)?;

    if !top.is_multiple_of(STACK_ALIGNMENT) {
        return Err(ContextError::invalid());
    }
    if stack.len.get() < STACK_ALIGNMENT + RED_ZONE_BYTES + mem::size_of::<usize>() * 2 {
        return Err(ContextError::invalid());
    }

    let bootstrap_rsp = top
        .checked_sub(mem::size_of::<usize>() * 2)
        .ok_or_else(ContextError::invalid)?;
    let bootstrap_slot = bootstrap_rsp as *mut usize;
    unsafe {
        bootstrap_slot.write(fusion_macos_x86_64_context_start as *const () as usize);
        bootstrap_slot.add(1).write(0);
    }

    Ok(bootstrap_rsp)
}

fn capture_control_state() -> (u32, u16) {
    let mut mxcsr = 0_u32;
    let mut x87_cw = 0_u16;

    unsafe {
        asm!(
            "stmxcsr [{}]",
            in(reg) &raw mut mxcsr,
            options(nostack, preserves_flags),
        );
        asm!(
            "fnstcw [{}]",
            in(reg) &raw mut x87_cw,
            options(nostack, preserves_flags),
        );
    }

    (mxcsr, x87_cw)
}

unsafe extern "C" {
    fn fusion_macos_x86_64_context_start() -> !;
}

/// Selected macOS context provider type.
pub type PlatformContext = MacOsContext;
/// Selected macOS saved-context type.
pub type PlatformSavedContext = MacOsSavedContext;

/// Returns the selected macOS context provider.
#[must_use]
pub const fn system_context() -> PlatformContext {
    PlatformContext::new()
}

/// Returns the selected macOS context support truth.
#[must_use]
pub const fn system_context_support() -> ContextSupport {
    X86_64_CONTEXT_SUPPORT
}

#[cfg(all(test, feature = "std", not(target_os = "none")))]
mod tests {
    extern crate std;

    use core::num::NonZeroUsize;
    use core::ptr::NonNull;

    use super::*;

    unsafe fn never_entry(_arg: *mut ()) -> ! {
        loop {
            core::hint::spin_loop();
        }
    }

    #[test]
    fn macos_x86_64_context_support_reports_native_make_and_swap() {
        let support = system_context().support();

        assert_eq!(support.implementation, ContextImplementationKind::Native);
        assert_eq!(support.guarantee, ContextGuarantee::Enforced);
        assert!(support.caps.contains(ContextCaps::MAKE));
        assert!(support.caps.contains(ContextCaps::SWAP));
        assert_eq!(
            support.structural_stack_overhead_bytes,
            STRUCTURAL_STACK_OVERHEAD_BYTES
        );
        assert_eq!(support.min_stack_alignment, STACK_ALIGNMENT);
        assert_eq!(support.red_zone_bytes, RED_ZONE_BYTES);
        assert_eq!(support.stack_direction, ContextStackDirection::Down);
    }

    #[test]
    fn macos_x86_64_context_make_rejects_invalid_stack_layout() {
        let context = system_context();

        let mut misaligned = self::std::vec![0_u8; 8193].into_boxed_slice();
        let bad_base = unsafe { misaligned.as_mut_ptr().add(1) };
        let misaligned_layout = ContextStackLayout {
            // SAFETY: pointer originates from an owned boxed allocation and is non-null.
            base: unsafe { NonNull::new_unchecked(bad_base) },
            len: NonZeroUsize::new(8192).expect("non-zero length"),
        };
        let misaligned_error = unsafe {
            context
                .make(misaligned_layout, never_entry, core::ptr::null_mut())
                .expect_err("misaligned stack top should be rejected")
        };
        assert_eq!(
            misaligned_error.kind(),
            crate::contract::pal::runtime::context::ContextErrorKind::Invalid
        );

        let mut small = self::std::vec![0_u8; 128].into_boxed_slice();
        let small_layout = ContextStackLayout {
            // SAFETY: pointer originates from an owned boxed allocation and is non-null.
            base: unsafe { NonNull::new_unchecked(small.as_mut_ptr()) },
            len: NonZeroUsize::new(128).expect("non-zero length"),
        };
        let small_error = unsafe {
            context
                .make(small_layout, never_entry, core::ptr::null_mut())
                .expect_err("undersized stack should be rejected")
        };
        assert_eq!(
            small_error.kind(),
            crate::contract::pal::runtime::context::ContextErrorKind::Invalid
        );
    }
}
