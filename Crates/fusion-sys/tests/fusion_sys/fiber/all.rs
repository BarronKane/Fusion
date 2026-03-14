extern crate std;

use core::mem;
use core::num::NonZeroUsize;
use core::ptr::NonNull;
use core::sync::atomic::{AtomicUsize, Ordering};

use fusion_sys::fiber::{
    ContextCaps, ContextErrorKind, ContextStackLayout, ContextSwitch, FiberSystem,
    PlatformSavedContext, system_context,
};
use std::vec;

#[repr(C)]
struct YieldState {
    caller: *mut PlatformSavedContext,
    callee: *mut PlatformSavedContext,
    progress: *const AtomicUsize,
}

unsafe fn yield_once(context: *mut ()) -> ! {
    let yield_state = unsafe { &mut *context.cast::<YieldState>() };
    let progress = unsafe { &*yield_state.progress };
    progress.store(1, Ordering::Release);

    let context = system_context();
    // SAFETY: the test sets both context pointers to valid saved-context slots before first
    // resume and only resumes on the same carrier thread.
    unsafe {
        context
            .swap(&mut *yield_state.callee, &*yield_state.caller)
            .expect("fiber context should yield back to caller");
    }

    loop {
        core::hint::spin_loop();
    }
}

#[test]
fn fiber_support_surface_is_exposed() {
    let support = FiberSystem::new().support();

    if support.context.caps.contains(ContextCaps::MAKE) {
        assert!(support.context.caps.contains(ContextCaps::SWAP));
    } else {
        assert_eq!(
            support.context.implementation,
            fusion_sys::fiber::ContextImplementationKind::Unsupported
        );
    }
}

#[test]
fn raw_context_make_and_swap_follow_backend_truth() {
    let context = system_context();
    let support = FiberSystem::new().support();

    let mut stack_words = vec![0_u128; 4096].into_boxed_slice();
    let stack_layout = ContextStackLayout {
        // SAFETY: the stack buffer is a live local allocation for the duration of the test.
        base: unsafe { NonNull::new_unchecked(stack_words.as_mut_ptr().cast::<u8>()) },
        len: NonZeroUsize::new(stack_words.len() * mem::size_of::<u128>())
            .expect("stack length should be non-zero"),
    };

    let progress = AtomicUsize::new(0);
    let mut resume_slot = PlatformSavedContext::default();
    let mut yield_state = YieldState {
        caller: core::ptr::null_mut(),
        callee: core::ptr::null_mut(),
        progress: &raw const progress,
    };
    let state_ptr = &raw mut yield_state;

    // SAFETY: the context entry and stack layout remain valid for the duration of the test.
    let make_result = unsafe {
        context.make(
            stack_layout,
            yield_once,
            state_ptr.cast::<YieldState>().cast(),
        )
    };

    if !support.context.caps.contains(ContextCaps::MAKE) {
        assert_eq!(
            make_result
                .expect_err("unsupported backend should reject raw context creation")
                .kind(),
            ContextErrorKind::Unsupported
        );
        return;
    }

    let mut fiber_context = make_result.expect("supported backend should create raw context");
    unsafe {
        (*state_ptr).caller = &raw mut resume_slot;
        (*state_ptr).callee = &raw mut fiber_context;
    }

    // SAFETY: both contexts are valid and the backend reports same-carrier migration only.
    unsafe {
        context
            .swap(&mut resume_slot, &fiber_context)
            .expect("raw context should switch to callee");
    }
    assert_eq!(progress.load(Ordering::Acquire), 1);
}
