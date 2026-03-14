//! Linux fusion-pal user-space context backend.
//!
//! Linux does not provide a modern first-class fiber API, so this backend currently uses the
//! classic `ucontext` family as an emulated context-switching surface on supported 64-bit
//! targets. That is not glamorous, but it is honest, no-alloc, and good enough to stop the
//! rest of the stack from standing on a blank file with ambitions.

#[cfg(any(target_arch = "aarch64", target_arch = "x86_64"))]
mod supported {
    use core::mem::{self, MaybeUninit};

    use rustix::thread as rustix_thread;

    use crate::pal::context::{
        ContextAuthoritySet, ContextBase, ContextCaps, ContextError, ContextGuarantee,
        ContextImplementationKind, ContextMigrationSupport, ContextStackDirection,
        ContextStackLayout, ContextSupport, ContextSwitch, ContextTlsIsolation, RawContextEntry,
    };

    const STACK_ALIGNMENT: usize = 16;

    #[cfg(target_arch = "x86_64")]
    const RED_ZONE_BYTES: usize = 128;
    #[cfg(target_arch = "aarch64")]
    const RED_ZONE_BYTES: usize = 0;

    const LINUX_CONTEXT_SUPPORT: ContextSupport = ContextSupport {
        caps: ContextCaps::MAKE
            .union(ContextCaps::SWAP)
            .union(ContextCaps::STACK_DIRECTION)
            .union(ContextCaps::TLS_ISOLATION)
            .union(ContextCaps::CROSS_CARRIER_RESUME)
            .union(ContextCaps::SIGNAL_MASK_PRESERVED)
            .union(ContextCaps::GUARD_REQUIRED),
        guarantee: ContextGuarantee::Enforced,
        min_stack_alignment: STACK_ALIGNMENT,
        red_zone_bytes: RED_ZONE_BYTES,
        stack_direction: ContextStackDirection::Down,
        guard_required: false,
        tls_isolation: ContextTlsIsolation::SharedCarrierThread,
        signal_mask_preserved: true,
        unwind_across_boundary: false,
        migration: ContextMigrationSupport::SameCarrierOnly,
        authorities: ContextAuthoritySet::OPERATING_SYSTEM.union(ContextAuthoritySet::ISA),
        implementation: ContextImplementationKind::Emulated,
    };

    /// Linux `ucontext` provider handle.
    #[derive(Debug, Clone, Copy, Default)]
    pub struct LinuxContext;

    /// Linux saved-context record backed by `ucontext_t`.
    pub struct LinuxSavedContext {
        raw: MaybeUninit<libc::ucontext_t>,
        ready: bool,
        owner_tid: libc::pid_t,
    }

    impl LinuxContext {
        /// Creates a new Linux context provider.
        #[must_use]
        pub const fn new() -> Self {
            Self
        }
    }

    impl LinuxSavedContext {
        /// Returns an empty capture slot ready to receive a saved context on first swap.
        #[must_use]
        pub const fn empty() -> Self {
            Self {
                raw: MaybeUninit::uninit(),
                ready: false,
                owner_tid: 0,
            }
        }

        const fn raw_ptr(&self) -> *const libc::ucontext_t {
            self.raw.as_ptr()
        }

        const fn raw_mut_ptr(&mut self) -> *mut libc::ucontext_t {
            self.raw.as_mut_ptr()
        }
    }

    impl Default for LinuxSavedContext {
        fn default() -> Self {
            Self::empty()
        }
    }

    impl core::fmt::Debug for LinuxSavedContext {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            f.debug_struct("LinuxSavedContext")
                .field("ready", &self.ready)
                .field("owner_tid", &self.owner_tid)
                .finish_non_exhaustive()
        }
    }

    impl ContextBase for LinuxContext {
        type Context = LinuxSavedContext;

        fn support(&self) -> ContextSupport {
            LINUX_CONTEXT_SUPPORT
        }
    }

    // SAFETY: this backend delegates creation and swapping to the libc `ucontext` surface and
    // enforces the reported same-carrier migration constraint before resuming saved contexts.
    unsafe impl ContextSwitch for LinuxContext {
        unsafe fn make(
            &self,
            stack: ContextStackLayout,
            entry: RawContextEntry,
            arg: *mut (),
        ) -> Result<Self::Context, ContextError> {
            validate_stack_layout(stack)?;

            let mut saved = LinuxSavedContext::empty();
            let rc = unsafe { libc::getcontext(saved.raw_mut_ptr()) };
            if rc != 0 {
                return Err(map_errno(last_errno()));
            }

            let raw = unsafe { &mut *saved.raw_mut_ptr() };
            raw.uc_link = core::ptr::null_mut();
            raw.uc_stack.ss_sp = stack.base.as_ptr().cast();
            raw.uc_stack.ss_size = stack.len.get();
            raw.uc_stack.ss_flags = 0;

            let trampoline = unsafe {
                mem::transmute::<extern "C" fn(usize, usize) -> !, extern "C" fn()>(
                    context_entry_trampoline,
                )
            };
            let entry_bits = entry as usize;
            let arg_bits = arg as usize;

            unsafe {
                libc::makecontext(saved.raw_mut_ptr(), trampoline, 2, entry_bits, arg_bits);
            }

            saved.ready = true;
            saved.owner_tid = current_tid();
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

            let current_tid = current_tid();
            if to.owner_tid != 0 && to.owner_tid != current_tid {
                return Err(ContextError::state_conflict());
            }

            from.ready = true;
            from.owner_tid = current_tid;
            let rc = unsafe { libc::swapcontext(from.raw_mut_ptr(), to.raw_ptr()) };
            if rc != 0 {
                from.ready = false;
                from.owner_tid = 0;
                return Err(map_errno(last_errno()));
            }
            Ok(())
        }
    }

    extern "C" fn context_entry_trampoline(entry_bits: usize, arg_bits: usize) -> ! {
        let entry = unsafe { mem::transmute::<usize, RawContextEntry>(entry_bits) };
        let arg = arg_bits as *mut ();
        unsafe { entry(arg) }
    }

    fn validate_stack_layout(stack: ContextStackLayout) -> Result<(), ContextError> {
        let top = stack
            .base
            .addr()
            .get()
            .checked_add(stack.len.get())
            .ok_or_else(ContextError::invalid)?;

        if top % STACK_ALIGNMENT != 0 {
            return Err(ContextError::invalid());
        }
        if stack.len.get() < STACK_ALIGNMENT + RED_ZONE_BYTES {
            return Err(ContextError::invalid());
        }
        Ok(())
    }

    fn current_tid() -> libc::pid_t {
        rustix_thread::gettid().as_raw_pid()
    }

    const fn map_errno(errno: libc::c_int) -> ContextError {
        match errno {
            libc::EINVAL | libc::EFAULT => ContextError::invalid(),
            libc::EBUSY => ContextError::busy(),
            libc::EPERM => ContextError::permission_denied(),
            libc::ENOMEM => ContextError::resource_exhausted(),
            _ => ContextError::platform(errno),
        }
    }

    fn last_errno() -> libc::c_int {
        unsafe { *libc::__errno_location() }
    }

    /// Selected Linux context provider type.
    pub type PlatformContext = LinuxContext;
    /// Selected Linux saved-context type.
    pub type PlatformSavedContext = LinuxSavedContext;

    /// Returns the selected Linux context provider.
    #[must_use]
    pub const fn system_context() -> PlatformContext {
        PlatformContext::new()
    }

    #[cfg(test)]
    mod tests {
        use core::num::NonZeroUsize;
        use core::ptr::NonNull;
        use core::sync::atomic::{AtomicUsize, Ordering};

        use super::*;

        extern crate std;
        use self::std::vec;

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
            // SAFETY: the test sets both pointers to valid same-thread context slots before the
            // first resume and only swaps back to the originating carrier thread.
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
        fn linux_context_support_reports_emulated_make_and_swap() {
            let support = system_context().support();

            assert_eq!(support.implementation, ContextImplementationKind::Emulated);
            assert_eq!(support.guarantee, ContextGuarantee::Enforced);
            assert!(support.caps.contains(ContextCaps::MAKE));
            assert!(support.caps.contains(ContextCaps::SWAP));
            assert_eq!(support.stack_direction, ContextStackDirection::Down);
            assert_eq!(
                support.tls_isolation,
                ContextTlsIsolation::SharedCarrierThread
            );
            assert_eq!(support.migration, ContextMigrationSupport::SameCarrierOnly);
            assert!(support.signal_mask_preserved);
            assert!(!support.unwind_across_boundary);
        }

        #[test]
        fn linux_context_make_and_swap_can_yield_back_to_caller() {
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
                // SAFETY: the stack buffer is live for the duration of the test.
                base: unsafe { NonNull::new_unchecked(stack_words.as_mut_ptr().cast::<u8>()) },
                len: NonZeroUsize::new(stack_words.len() * mem::size_of::<u128>())
                    .expect("stack length should be non-zero"),
            };

            // SAFETY: the stack layout, entry, and argument pointer remain valid for the test.
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
            }

            // SAFETY: both saved-context slots are valid and remain on the same carrier thread.
            unsafe {
                context
                    .swap(&mut resume_slot, &fiber_context)
                    .expect("swap to callee should succeed");
            }

            assert_eq!(progress.load(Ordering::Acquire), 1);
        }
    }
}

#[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
mod supported {
    use crate::pal::context::{UnsupportedContext, UnsupportedSavedContext};

    /// Selected Linux context provider type.
    pub type PlatformContext = UnsupportedContext;
    /// Selected Linux saved-context type.
    pub type PlatformSavedContext = UnsupportedSavedContext;

    /// Returns the selected Linux context provider.
    #[must_use]
    pub const fn system_context() -> PlatformContext {
        PlatformContext::new()
    }
}

pub use supported::{PlatformContext, PlatformSavedContext, system_context};
