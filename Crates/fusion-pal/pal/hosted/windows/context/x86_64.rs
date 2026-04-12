//! Native `x86_64` Windows context-switch backend.
//!
//! This backend saves the Win64 callee-saved register set, the nonvolatile XMM registers, and
//! floating-point control words directly in an ISA-specific record and resumes contexts with a
//! small assembly shim.

use core::arch::{
    asm,
    global_asm,
};
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

global_asm!(include_str!("x86_64.S"));

const STACK_ALIGNMENT: usize = 16;
const RED_ZONE_BYTES: usize = 0;
const BOOTSTRAP_STACK_BYTES: usize = 48;
const STRUCTURAL_STACK_OVERHEAD_BYTES: usize = 288;

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

/// Native `x86_64` Windows context provider.
#[derive(Debug, Clone, Copy, Default)]
pub struct WindowsContext;

#[repr(C, align(16))]
#[derive(Debug, Default)]
struct X86_64Registers {
    rsp: usize,
    rbx: usize,
    rbp: usize,
    rdi: usize,
    rsi: usize,
    r12: usize,
    r13: usize,
    r14: usize,
    r15: usize,
    mxcsr: u32,
    x87_cw: u16,
    _padding: u16,
    xmm6: [u8; 16],
    xmm7: [u8; 16],
    xmm8: [u8; 16],
    xmm9: [u8; 16],
    xmm10: [u8; 16],
    xmm11: [u8; 16],
    xmm12: [u8; 16],
    xmm13: [u8; 16],
    xmm14: [u8; 16],
    xmm15: [u8; 16],
}

/// Saved `x86_64` execution context.
#[derive(Debug, Default)]
pub struct WindowsSavedContext {
    registers: X86_64Registers,
    ready: bool,
}

unsafe extern "C" {
    fn fusion_windows_x86_64_context_swap(from: *mut X86_64Registers, to: *const X86_64Registers);
}

impl WindowsContext {
    /// Creates a new `x86_64` Windows context provider.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl WindowsSavedContext {
    /// Returns an empty capture slot ready to receive a saved context on first swap.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            registers: X86_64Registers {
                rsp: 0,
                rbx: 0,
                rbp: 0,
                rdi: 0,
                rsi: 0,
                r12: 0,
                r13: 0,
                r14: 0,
                r15: 0,
                mxcsr: 0,
                x87_cw: 0,
                _padding: 0,
                xmm6: [0; 16],
                xmm7: [0; 16],
                xmm8: [0; 16],
                xmm9: [0; 16],
                xmm10: [0; 16],
                xmm11: [0; 16],
                xmm12: [0; 16],
                xmm13: [0; 16],
                xmm14: [0; 16],
                xmm15: [0; 16],
            },
            ready: false,
        }
    }
}

impl ContextBaseContract for WindowsContext {
    type Context = WindowsSavedContext;

    fn support(&self) -> ContextSupport {
        X86_64_CONTEXT_SUPPORT
    }
}

// SAFETY: this backend saves and restores the reported Win64 context record directly and the
// saved state is carrier-agnostic process memory, so suspended contexts may resume on another
// carrier while still sharing that carrier thread's TLS domain.
unsafe impl ContextSwitch for WindowsContext {
    unsafe fn make(
        &self,
        stack: ContextStackLayout,
        entry: RawContextEntry,
        arg: *mut (),
    ) -> Result<Self::Context, ContextError> {
        let bootstrap_rsp = validate_stack_layout(stack)?;
        let mut saved = WindowsSavedContext::empty();
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
            fusion_windows_x86_64_context_swap(&raw mut from.registers, &raw const to.registers);
        }
        Ok(())
    }
}

#[unsafe(no_mangle)]
extern "C" fn fusion_windows_x86_64_context_bootstrap(entry_bits: usize, arg_bits: usize) -> ! {
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
    if stack.len.get() < BOOTSTRAP_STACK_BYTES {
        return Err(ContextError::invalid());
    }

    let bootstrap_rsp = top
        .checked_sub(BOOTSTRAP_STACK_BYTES)
        .ok_or_else(ContextError::invalid)?;
    let bootstrap_slot = bootstrap_rsp as *mut usize;
    unsafe {
        bootstrap_slot.write(fusion_windows_x86_64_context_start as *const () as usize);
        bootstrap_slot.add(1).write(0);
        bootstrap_slot.add(2).write(0);
        bootstrap_slot.add(3).write(0);
        bootstrap_slot.add(4).write(0);
        bootstrap_slot.add(5).write(0);
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
    fn fusion_windows_x86_64_context_start() -> !;
}

/// Selected Windows context provider type.
pub type PlatformContext = WindowsContext;
/// Selected Windows saved-context type.
pub type PlatformSavedContext = WindowsSavedContext;

/// Returns the selected Windows context provider.
#[must_use]
pub const fn system_context() -> PlatformContext {
    PlatformContext::new()
}

/// Returns the selected Windows context support truth.
#[must_use]
pub const fn system_context_support() -> ContextSupport {
    X86_64_CONTEXT_SUPPORT
}

#[cfg(all(test, feature = "std", target_os = "windows"))]
mod tests {
    use core::num::NonZeroUsize;
    use core::ptr::NonNull;
    use core::sync::atomic::{
        AtomicUsize,
        Ordering,
    };

    use super::*;

    extern crate std;
    use self::std::vec;

    #[repr(C)]
    struct YieldState {
        caller: *mut PlatformSavedContext,
        callee: *mut PlatformSavedContext,
        progress: *const AtomicUsize,
    }

    const MAIN_SENTINEL: usize = 0x0154_B777_DEAD_1337;
    const FIBER_SENTINEL: usize = 0xBAAD_E85F_1984_DEAD;

    unsafe fn yield_once(context: *mut ()) -> ! {
        let yield_state = unsafe { &mut *context.cast::<YieldState>() };
        unsafe {
            asm!(
                "mov r12, {sentinel}",
                sentinel = in(reg) FIBER_SENTINEL,
                options(nostack, preserves_flags),
            );
        }
        let progress = unsafe { &*yield_state.progress };
        progress.store(1, Ordering::Release);

        let context = system_context();
        unsafe {
            context
                .swap(&mut *yield_state.callee, &*yield_state.caller)
                .expect("yield back to caller should succeed");
        }

        loop {
            core::hint::spin_loop();
        }
    }

    #[test]
    fn windows_context_support_reports_native_make_and_swap() {
        let support = system_context().support();

        assert_eq!(support.implementation, ContextImplementationKind::Native);
        assert_eq!(support.guarantee, ContextGuarantee::Enforced);
        assert!(support.caps.contains(ContextCaps::MAKE));
        assert!(support.caps.contains(ContextCaps::SWAP));
        assert_eq!(
            support.structural_stack_overhead_bytes,
            STRUCTURAL_STACK_OVERHEAD_BYTES
        );
        assert_eq!(support.red_zone_bytes, RED_ZONE_BYTES);
        assert_eq!(support.stack_direction, ContextStackDirection::Down);
        assert_eq!(
            support.tls_isolation,
            ContextTlsIsolation::SharedCarrierThread
        );
        assert_eq!(support.migration, ContextMigrationSupport::CrossCarrier);
        assert!(!support.signal_mask_preserved);
        assert!(!support.unwind_across_boundary);
    }

    #[test]
    fn windows_context_make_and_swap_can_yield_back_to_caller() {
        let context = system_context();
        let progress = AtomicUsize::new(0);
        let mut resume_slot = PlatformSavedContext::default();
        let mut yield_state = YieldState {
            caller: core::ptr::null_mut(),
            callee: core::ptr::null_mut(),
            progress: &raw const progress,
        };
        let state_ptr = &raw mut yield_state;

        let mut stack_words = vec![0_u128; 4096].into_boxed_slice();
        let stack_layout = ContextStackLayout {
            base: unsafe { NonNull::new_unchecked(stack_words.as_mut_ptr().cast::<u8>()) },
            len: NonZeroUsize::new(stack_words.len() * mem::size_of::<u128>())
                .expect("stack length should be non-zero"),
        };

        let mut fiber_context = unsafe {
            context
                .make(
                    stack_layout,
                    yield_once,
                    state_ptr.cast::<YieldState>().cast(),
                )
                .expect("raw context should be created")
        };

        unsafe {
            (*state_ptr).caller = &raw mut resume_slot;
            (*state_ptr).callee = &raw mut fiber_context;
            asm!(
                "mov r12, {sentinel}",
                sentinel = in(reg) MAIN_SENTINEL,
                options(nostack, preserves_flags),
            );
        }

        unsafe {
            context
                .swap(&mut resume_slot, &fiber_context)
                .expect("swap to callee should succeed");
        }

        let preserved_r12: usize;
        unsafe {
            asm!(
                "mov {preserved}, r12",
                preserved = lateout(reg) preserved_r12,
                options(nostack, preserves_flags),
            );
        }

        assert_eq!(progress.load(Ordering::Acquire), 1);
        assert_eq!(preserved_r12, MAIN_SENTINEL);
    }
}
