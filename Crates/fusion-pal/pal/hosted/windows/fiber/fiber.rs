//! Windows hosted-fiber helper surface.
//!
//! Windows can honestly surface process-wide elastic stack-fault handling through vectored
//! exception handlers. Alternate signal stacks and readiness-compatible wake sources remain
//! unsupported on this backend.

use core::ffi::c_void;
use core::hint::spin_loop;
use core::sync::atomic::{
    AtomicUsize,
    Ordering,
};

use windows::Win32::Foundation::{
    ERROR_ACCESS_DENIED,
    ERROR_INVALID_PARAMETER,
    ERROR_NOT_ENOUGH_MEMORY,
    ERROR_OUTOFMEMORY,
    GetLastError,
    STATUS_ACCESS_VIOLATION,
    STATUS_GUARD_PAGE_VIOLATION,
    WIN32_ERROR,
};
use windows::Win32::System::Diagnostics::Debug::{
    AddVectoredExceptionHandler,
    EXCEPTION_CONTINUE_EXECUTION,
    EXCEPTION_CONTINUE_SEARCH,
    EXCEPTION_POINTERS,
};
use windows::Win32::System::Memory::{
    PAGE_PROTECTION_FLAGS,
    PAGE_READWRITE,
    VirtualProtect,
};

use crate::contract::pal::runtime::fiber::{
    FiberHostError,
    FiberHostSupport,
    PlatformElasticFaultHandler,
    PlatformWakeToken,
};
pub use crate::contract::pal::runtime::fiber::{
    UnsupportedFiberSignalStack as PlatformFiberSignalStack,
    UnsupportedFiberWakeSignal as PlatformFiberWakeSignal,
};

const ELASTIC_HANDLER_UNINITIALIZED: usize = 0;
const ELASTIC_HANDLER_INSTALLING: usize = 1;
const ELASTIC_HANDLER_READY: usize = 2;

static ELASTIC_HANDLER_STATE: AtomicUsize = AtomicUsize::new(ELASTIC_HANDLER_UNINITIALIZED);
static ELASTIC_HANDLER_CALLBACK: AtomicUsize = AtomicUsize::new(0);
static ELASTIC_HANDLER_HANDLE: AtomicUsize = AtomicUsize::new(0);

/// Windows hosted-fiber helper provider.
#[derive(Debug, Clone, Copy, Default)]
pub struct WindowsFiberHost;

/// Target-selected hosted-fiber helper provider alias for Windows builds.
pub type PlatformFiberHost = WindowsFiberHost;

/// Returns the process-wide Windows hosted-fiber helper provider handle.
#[must_use]
pub const fn system_fiber_host() -> PlatformFiberHost {
    PlatformFiberHost::new()
}

impl WindowsFiberHost {
    /// Creates a new Windows hosted-fiber helper provider.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Returns the truthful Windows hosted-fiber helper support surface.
    #[must_use]
    pub const fn support(&self) -> FiberHostSupport {
        FiberHostSupport {
            elastic_stack_faults: true,
            signal_stack: false,
            wake_signal: false,
        }
    }

    /// Installs the Windows vectored exception handler used for elastic stack promotion.
    ///
    /// # Errors
    ///
    /// Returns an honest state or platform error when Windows refuses handler installation.
    pub fn ensure_elastic_fault_handler(
        &self,
        handler: PlatformElasticFaultHandler,
    ) -> Result<(), FiberHostError> {
        if ELASTIC_HANDLER_STATE.load(Ordering::Acquire) == ELASTIC_HANDLER_READY {
            ELASTIC_HANDLER_CALLBACK.store(handler as usize, Ordering::Release);
            return Ok(());
        }

        loop {
            match ELASTIC_HANDLER_STATE.compare_exchange(
                ELASTIC_HANDLER_UNINITIALIZED,
                ELASTIC_HANDLER_INSTALLING,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => break,
                Err(ELASTIC_HANDLER_READY) => {
                    ELASTIC_HANDLER_CALLBACK.store(handler as usize, Ordering::Release);
                    return Ok(());
                }
                Err(ELASTIC_HANDLER_INSTALLING) => spin_loop(),
                Err(_) => return Err(FiberHostError::state_conflict()),
            }
        }

        let registered = unsafe { AddVectoredExceptionHandler(1, Some(elastic_exception_handler)) };
        if registered.is_null() {
            ELASTIC_HANDLER_STATE.store(ELASTIC_HANDLER_UNINITIALIZED, Ordering::Release);
            return Err(map_win32_error(unsafe { GetLastError() }));
        }

        ELASTIC_HANDLER_HANDLE.store(registered as usize, Ordering::Release);
        ELASTIC_HANDLER_CALLBACK.store(handler as usize, Ordering::Release);
        ELASTIC_HANDLER_STATE.store(ELASTIC_HANDLER_READY, Ordering::Release);
        Ok(())
    }

    /// Promotes one detector page to read/write access.
    ///
    /// # Errors
    ///
    /// Returns an honest platform error when Windows rejects the protection change.
    pub fn promote_elastic_page(&self, base: usize, len: usize) -> Result<(), FiberHostError> {
        if base == 0 || len == 0 {
            return Err(FiberHostError::invalid());
        }

        let mut old = PAGE_PROTECTION_FLAGS(0);
        unsafe {
            VirtualProtect(base as *const c_void, len, PAGE_READWRITE, &mut old)
                .map_err(|error| map_hresult(error.code().0))?;
        }
        Ok(())
    }

    /// Installs one alternate signal stack for the current carrier thread.
    ///
    /// # Errors
    ///
    /// Always returns `Unsupported` on this backend.
    pub const fn install_signal_stack(&self) -> Result<PlatformFiberSignalStack, FiberHostError> {
        Err(FiberHostError::unsupported())
    }

    /// Creates one wake signal that can be registered with a readiness poller.
    ///
    /// # Errors
    ///
    /// Always returns `Unsupported` on this backend because the Windows hosted event backend is
    /// completion-oriented, not readiness-oriented.
    pub const fn create_wake_signal(&self) -> Result<PlatformFiberWakeSignal, FiberHostError> {
        Err(FiberHostError::unsupported())
    }

    /// Signals one wake token from a fault or scheduler path.
    ///
    /// # Errors
    ///
    /// Returns `Unsupported` when a valid token is supplied on this backend.
    pub const fn notify_wake_token(&self, token: PlatformWakeToken) -> Result<(), FiberHostError> {
        if token.is_valid() {
            Err(FiberHostError::unsupported())
        } else {
            Ok(())
        }
    }
}

unsafe extern "system" fn elastic_exception_handler(exceptioninfo: *mut EXCEPTION_POINTERS) -> i32 {
    if exceptioninfo.is_null() {
        return EXCEPTION_CONTINUE_SEARCH;
    }

    let record = unsafe { (*exceptioninfo).ExceptionRecord };
    if record.is_null() {
        return EXCEPTION_CONTINUE_SEARCH;
    }

    let record = unsafe { &*record };
    if record.ExceptionCode != STATUS_GUARD_PAGE_VIOLATION
        && record.ExceptionCode != STATUS_ACCESS_VIOLATION
    {
        return EXCEPTION_CONTINUE_SEARCH;
    }
    if record.NumberParameters < 2 {
        return EXCEPTION_CONTINUE_SEARCH;
    }

    let callback_bits = ELASTIC_HANDLER_CALLBACK.load(Ordering::Acquire);
    if callback_bits == 0 {
        return EXCEPTION_CONTINUE_SEARCH;
    }

    let callback =
        unsafe { core::mem::transmute::<usize, PlatformElasticFaultHandler>(callback_bits) };
    if callback(record.ExceptionInformation[1]) {
        EXCEPTION_CONTINUE_EXECUTION
    } else {
        EXCEPTION_CONTINUE_SEARCH
    }
}

const fn map_win32_error(error: WIN32_ERROR) -> FiberHostError {
    match error {
        ERROR_INVALID_PARAMETER => FiberHostError::invalid(),
        ERROR_ACCESS_DENIED => FiberHostError::state_conflict(),
        ERROR_NOT_ENOUGH_MEMORY | ERROR_OUTOFMEMORY => FiberHostError::resource_exhausted(),
        _ => FiberHostError::platform(error.0 as i32),
    }
}

const fn map_hresult(code: i32) -> FiberHostError {
    let raw = code as u32;
    let facility = (raw >> 16) & 0x1fff;
    if facility == 7 {
        return map_win32_error(WIN32_ERROR(raw & 0xffff));
    }
    FiberHostError::platform(code)
}

#[cfg(all(test, feature = "std", target_os = "windows"))]
mod tests {
    use super::system_fiber_host;

    #[test]
    fn windows_fiber_support_reports_only_elastic_fault_handling() {
        let support = system_fiber_host().support();

        assert!(support.elastic_stack_faults);
        assert!(!support.signal_stack);
        assert!(!support.wake_signal);
    }
}
