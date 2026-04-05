//! Native `aarch64` macOS context-switch backend.
//!
//! This backend saves the AAPCS64 callee-saved integer register set, preserved SIMD lanes,
//! and floating-point control state directly in an ISA-specific record.

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

global_asm!(include_str!("aarch64.S"));

const STACK_ALIGNMENT: usize = 16;
const RED_ZONE_BYTES: usize = 0;
const STRUCTURAL_STACK_OVERHEAD_BYTES: usize = 352;

const AARCH64_CONTEXT_SUPPORT: ContextSupport = ContextSupport {
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

/// Native `aarch64` macOS context provider.
#[derive(Debug, Clone, Copy, Default)]
pub struct MacOsContext;

#[repr(C)]
#[derive(Debug, Default)]
struct Aarch64Registers {
    sp: usize,
    x19: usize,
    x20: usize,
    x21: usize,
    x22: usize,
    x23: usize,
    x24: usize,
    x25: usize,
    x26: usize,
    x27: usize,
    x28: usize,
    x29: usize,
    x30: usize,
    fpcr: u64,
    fpsr: u64,
    d8: u64,
    d9: u64,
    d10: u64,
    d11: u64,
    d12: u64,
    d13: u64,
    d14: u64,
    d15: u64,
}

/// Saved `aarch64` execution context.
#[derive(Debug, Default)]
pub struct MacOsSavedContext {
    registers: Aarch64Registers,
    ready: bool,
}

unsafe extern "C" {
    fn fusion_macos_aarch64_context_swap(from: *mut Aarch64Registers, to: *const Aarch64Registers);
}

impl MacOsContext {
    /// Creates a new `aarch64` macOS context provider.
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
            registers: Aarch64Registers {
                sp: 0,
                x19: 0,
                x20: 0,
                x21: 0,
                x22: 0,
                x23: 0,
                x24: 0,
                x25: 0,
                x26: 0,
                x27: 0,
                x28: 0,
                x29: 0,
                x30: 0,
                fpcr: 0,
                fpsr: 0,
                d8: 0,
                d9: 0,
                d10: 0,
                d11: 0,
                d12: 0,
                d13: 0,
                d14: 0,
                d15: 0,
            },
            ready: false,
        }
    }
}

impl ContextBaseContract for MacOsContext {
    type Context = MacOsSavedContext;

    fn support(&self) -> ContextSupport {
        AARCH64_CONTEXT_SUPPORT
    }
}

// SAFETY: this backend saves and restores the reported aarch64 context record directly and the
// saved state is carrier-agnostic process memory, so suspended contexts may resume on a
// different carrier while still sharing that carrier thread's TLS domain.
unsafe impl ContextSwitch for MacOsContext {
    unsafe fn make(
        &self,
        stack: ContextStackLayout,
        entry: RawContextEntry,
        arg: *mut (),
    ) -> Result<Self::Context, ContextError> {
        let bootstrap_sp = validate_stack_layout(stack)?;
        let mut saved = MacOsSavedContext::empty();

        saved.registers.sp = bootstrap_sp;
        saved.registers.x19 = entry as usize;
        saved.registers.x20 = arg as usize;
        saved.registers.x30 = fusion_macos_aarch64_context_start as *const () as usize;
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
            fusion_macos_aarch64_context_swap(&raw mut from.registers, &raw const to.registers);
        }
        Ok(())
    }
}

#[unsafe(no_mangle)]
extern "C" fn fusion_macos_aarch64_context_bootstrap(entry_bits: usize, arg_bits: usize) -> ! {
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
    if stack.len.get() < STACK_ALIGNMENT {
        return Err(ContextError::invalid());
    }

    Ok(top)
}

unsafe extern "C" {
    fn fusion_macos_aarch64_context_start() -> !;
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
    AARCH64_CONTEXT_SUPPORT
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
    fn macos_aarch64_context_support_reports_native_make_and_swap() {
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
    fn macos_aarch64_context_make_rejects_invalid_stack_layout() {
        let context = system_context();

        let mut misaligned = self::std::vec![0_u8; 1025].into_boxed_slice();
        let bad_base = unsafe { misaligned.as_mut_ptr().add(1) };
        let misaligned_layout = ContextStackLayout {
            // SAFETY: pointer originates from an owned boxed allocation and is non-null.
            base: unsafe { NonNull::new_unchecked(bad_base) },
            len: NonZeroUsize::new(1024).expect("non-zero length"),
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

        let mut small = self::std::vec![0_u8; 8].into_boxed_slice();
        let small_layout = ContextStackLayout {
            // SAFETY: pointer originates from an owned boxed allocation and is non-null.
            base: unsafe { NonNull::new_unchecked(small.as_mut_ptr()) },
            len: NonZeroUsize::new(8).expect("non-zero length"),
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
