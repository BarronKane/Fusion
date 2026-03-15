//! Linux `ucontext` fallback backend.
//!
//! This remains as an emulated path on Linux architectures that do not yet have a native
//! assembly backend in-tree.

use core::mem::{self, MaybeUninit};

use rustix::thread as rustix_thread;

use crate::pal::context::{
    ContextAuthoritySet, ContextBase, ContextCaps, ContextError, ContextGuarantee,
    ContextImplementationKind, ContextMigrationSupport, ContextStackDirection, ContextStackLayout,
    ContextSupport, ContextSwitch, ContextTlsIsolation, RawContextEntry,
};

const STACK_ALIGNMENT: usize = 16;
const RED_ZONE_BYTES: usize = 0;

const UCONTEXT_SUPPORT: ContextSupport = ContextSupport {
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
        UCONTEXT_SUPPORT
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
