//! Linux hosted-fiber helper surface.
//!
//! This module owns the Linux-specific seams required by higher hosted fiber runtimes:
//! SIGSEGV handler installation for elastic stack promotion, alternate signal stacks for
//! carrier threads, and nonblocking wake pipes suitable for readiness pollers.

use core::cell::UnsafeCell;
use core::hint::spin_loop;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicUsize, Ordering};

use libc::{self, c_void};

use super::mem::system_mem;
use crate::pal::mem::{
    Backing,
    CachePolicy,
    MapFlags,
    MapRequest,
    MemBase,
    MemError,
    MemErrorKind,
    MemMap,
    Protect,
    Region,
    RegionAttrs,
};
use crate::sys::fiber_common::{
    FiberHostError,
    FiberHostSupport,
    PlatformElasticFaultHandler,
    PlatformWakeToken,
};

/// Linux hosted-fiber helper provider.
#[derive(Debug, Clone, Copy, Default)]
pub struct LinuxFiberHost;

/// Target-selected hosted-fiber helper provider alias for Linux builds.
pub type PlatformFiberHost = LinuxFiberHost;

#[derive(Debug)]
struct ElasticSignalState {
    previous: libc::sigaction,
}

static ELASTIC_SIGNAL_STATE: AtomicUsize = AtomicUsize::new(0);
static ELASTIC_SIGNAL_HANDLER: AtomicUsize = AtomicUsize::new(0);
const ELASTIC_SIGNAL_UNINITIALIZED: usize = 0;
const ELASTIC_SIGNAL_INSTALLING: usize = 1;
const ELASTIC_SIGNAL_READY: usize = 2;

struct ElasticSignalStateStorage {
    state: UnsafeCell<MaybeUninit<ElasticSignalState>>,
}

unsafe impl Sync for ElasticSignalStateStorage {}

static ELASTIC_SIGNAL_STATE_STORAGE: ElasticSignalStateStorage = ElasticSignalStateStorage {
    state: UnsafeCell::new(MaybeUninit::uninit()),
};

/// Opaque alternate-signal-stack guard for one carrier thread.
#[derive(Debug)]
pub struct PlatformFiberSignalStack {
    region: Region,
}

/// Opaque Linux wake signal backed by a nonblocking pipe.
#[derive(Debug)]
pub struct PlatformFiberWakeSignal {
    source_handle: usize,
    reader_fd: i32,
    writer_fd: i32,
}

/// Returns the process-wide Linux hosted-fiber helper provider handle.
#[must_use]
pub const fn system_fiber_host() -> PlatformFiberHost {
    PlatformFiberHost::new()
}

#[allow(clippy::trivially_copy_pass_by_ref, clippy::unused_self)]
impl LinuxFiberHost {
    /// Creates a new Linux hosted-fiber helper provider.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Returns the truthful Linux hosted-fiber helper support surface.
    #[must_use]
    pub const fn support(&self) -> FiberHostSupport {
        FiberHostSupport {
            elastic_stack_faults: true,
            signal_stack: true,
            wake_signal: true,
        }
    }

    /// Installs the Linux SIGSEGV handler used for elastic stack promotion.
    ///
    /// # Errors
    ///
    /// Returns an honest state or platform error when Linux refuses handler installation.
    pub fn ensure_elastic_fault_handler(
        &self,
        handler: PlatformElasticFaultHandler,
    ) -> Result<(), FiberHostError> {
        let installed = elastic_stack_sigsegv_handler as *const () as usize;
        let mut current = unsafe { core::mem::zeroed::<libc::sigaction>() };
        unsafe {
            if libc::sigaction(libc::SIGSEGV, core::ptr::null(), &raw mut current) != 0 {
                return Err(FiberHostError::state_conflict());
            }
        }

        if current.sa_sigaction == installed
            && ELASTIC_SIGNAL_STATE.load(Ordering::Acquire) == ELASTIC_SIGNAL_READY
        {
            ELASTIC_SIGNAL_HANDLER.store(handler as usize, Ordering::Release);
            return Ok(());
        }

        loop {
            match ELASTIC_SIGNAL_STATE.compare_exchange(
                ELASTIC_SIGNAL_UNINITIALIZED,
                ELASTIC_SIGNAL_INSTALLING,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => break,
                Err(ELASTIC_SIGNAL_READY) => {
                    let mut live = unsafe { core::mem::zeroed::<libc::sigaction>() };
                    unsafe {
                        if libc::sigaction(libc::SIGSEGV, core::ptr::null(), &raw mut live) != 0 {
                            return Err(FiberHostError::state_conflict());
                        }
                    }
                    if live.sa_sigaction != installed {
                        return Err(FiberHostError::state_conflict());
                    }
                    ELASTIC_SIGNAL_HANDLER.store(handler as usize, Ordering::Release);
                    return Ok(());
                }
                Err(ELASTIC_SIGNAL_INSTALLING) => spin_loop(),
                Err(_) => return Err(FiberHostError::state_conflict()),
            }
        }

        let mut action = unsafe { core::mem::zeroed::<libc::sigaction>() };
        let mut previous = unsafe { core::mem::zeroed::<libc::sigaction>() };
        action.sa_flags = libc::SA_SIGINFO | libc::SA_ONSTACK;
        action.sa_sigaction = installed;
        unsafe {
            libc::sigemptyset(&raw mut action.sa_mask);
            if libc::sigaction(libc::SIGSEGV, &raw const action, &raw mut previous) != 0 {
                ELASTIC_SIGNAL_STATE.store(ELASTIC_SIGNAL_UNINITIALIZED, Ordering::Release);
                return Err(FiberHostError::state_conflict());
            }
        }

        unsafe {
            write_elastic_signal_state(previous);
        }
        ELASTIC_SIGNAL_HANDLER.store(handler as usize, Ordering::Release);
        ELASTIC_SIGNAL_STATE.store(ELASTIC_SIGNAL_READY, Ordering::Release);
        Ok(())
    }

    /// Promotes one detector page to read/write access.
    ///
    /// # Errors
    ///
    /// Returns an honest platform error when Linux rejects the protection change.
    pub fn promote_elastic_page(&self, base: usize, len: usize) -> Result<(), FiberHostError> {
        let rc = unsafe {
            libc::mprotect(
                base as *mut libc::c_void,
                len,
                libc::PROT_READ | libc::PROT_WRITE,
            )
        };
        if rc == 0 {
            Ok(())
        } else {
            Err(map_errno(last_errno()))
        }
    }

    /// Installs one alternate signal stack for the current carrier thread.
    ///
    /// # Errors
    ///
    /// Returns an honest memory-mapping or `sigaltstack` failure.
    pub fn install_signal_stack(&self) -> Result<PlatformFiberSignalStack, FiberHostError> {
        let memory = system_mem();
        let page = memory.page_info().alloc_granule.get();
        let size = libc::SIGSTKSZ.max(page.saturating_mul(16)).max(64 * 1024);
        let region = unsafe {
            memory.map(&MapRequest {
                len: size,
                align: page,
                protect: Protect::READ | Protect::WRITE,
                flags: MapFlags::PRIVATE,
                attrs: RegionAttrs::VIRTUAL_ONLY,
                cache: CachePolicy::Default,
                placement: crate::pal::mem::Placement::Anywhere,
                backing: Backing::Anonymous,
            })
        }
        .map_err(map_mem_error)?;

        let stack = libc::stack_t {
            ss_sp: region.base.as_ptr().cast::<c_void>(),
            ss_flags: 0,
            ss_size: region.len,
        };
        let rc = unsafe { libc::sigaltstack(&raw const stack, core::ptr::null_mut()) };
        if rc != 0 {
            let _ = unsafe { memory.unmap(region) };
            return Err(FiberHostError::state_conflict());
        }

        Ok(PlatformFiberSignalStack { region })
    }

    /// Creates one nonblocking wake signal that can be registered with a readiness poller.
    ///
    /// # Errors
    ///
    /// Returns an honest pipe-creation or descriptor-conversion failure.
    pub fn create_wake_signal(&self) -> Result<PlatformFiberWakeSignal, FiberHostError> {
        let mut fds = [0_i32; 2];
        let rc = unsafe { libc::pipe2((&raw mut fds).cast(), libc::O_CLOEXEC | libc::O_NONBLOCK) };
        if rc != 0 {
            return Err(map_errno(last_errno()));
        }
        let source_handle =
            usize::try_from(fds[0]).map_err(|_| FiberHostError::resource_exhausted())?;
        Ok(PlatformFiberWakeSignal {
            source_handle,
            reader_fd: fds[0],
            writer_fd: fds[1],
        })
    }

    /// Signals one wake token from a fault or scheduler path.
    ///
    /// # Errors
    ///
    /// Returns an honest write failure when Linux rejects the wake signal.
    pub fn notify_wake_token(&self, token: PlatformWakeToken) -> Result<(), FiberHostError> {
        if !token.is_valid() {
            return Ok(());
        }
        let fd = i32::try_from(token.into_raw()).map_err(|_| FiberHostError::invalid())?;
        signal_fd(fd)
    }
}

impl PlatformFiberWakeSignal {
    /// Returns the source handle used to register this signal with a readiness poller.
    ///
    /// # Errors
    ///
    /// Never fails on Linux after successful construction.
    pub const fn source_handle(&self) -> Result<usize, FiberHostError> {
        Ok(self.source_handle)
    }

    /// Returns the wake token associated with this signal.
    #[must_use]
    pub fn token(&self) -> PlatformWakeToken {
        u64::try_from(self.writer_fd).map_or_else(
            |_| PlatformWakeToken::invalid(),
            PlatformWakeToken::from_raw,
        )
    }

    /// Signals the wake source.
    ///
    /// # Errors
    ///
    /// Returns an honest write failure when Linux rejects the wake signal.
    pub fn signal(&self) -> Result<(), FiberHostError> {
        signal_fd(self.writer_fd)
    }

    /// Drains the wake source after one readiness notification.
    ///
    /// # Errors
    ///
    /// Returns an honest read failure when Linux rejects the drain.
    pub fn drain(&self) -> Result<(), FiberHostError> {
        let mut buffer = [0_u8; 64];
        loop {
            let read = unsafe {
                libc::read(
                    self.reader_fd,
                    (&raw mut buffer).cast::<c_void>(),
                    buffer.len(),
                )
            };
            if read == 0 {
                return Ok(());
            }
            if read > 0 {
                continue;
            }
            let errno = last_errno();
            if errno == libc::EINTR {
                continue;
            }
            if errno == libc::EAGAIN {
                return Ok(());
            }
            return Err(map_errno(errno));
        }
    }
}

impl Drop for PlatformFiberWakeSignal {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.reader_fd);
            libc::close(self.writer_fd);
        }
    }
}

impl Drop for PlatformFiberSignalStack {
    fn drop(&mut self) {
        let disabled = libc::stack_t {
            ss_sp: core::ptr::null_mut(),
            ss_flags: libc::SS_DISABLE,
            ss_size: 0,
        };
        unsafe {
            libc::sigaltstack(&raw const disabled, core::ptr::null_mut());
            let _ = system_mem().unmap(self.region);
        }
    }
}

unsafe extern "C" fn elastic_stack_sigsegv_handler(
    signal: libc::c_int,
    info: *mut libc::siginfo_t,
    context: *mut libc::c_void,
) {
    let fault_addr = if info.is_null() {
        0
    } else {
        unsafe { (*info).si_addr() as usize }
    };

    if fault_addr != 0 && call_fault_handler(fault_addr) {
        return;
    }

    unsafe { chain_previous_sigsegv(signal, info, context) };
}

fn call_fault_handler(fault_addr: usize) -> bool {
    let handler = ELASTIC_SIGNAL_HANDLER.load(Ordering::Acquire);
    if handler == 0 {
        return false;
    }
    let callback: PlatformElasticFaultHandler = unsafe { core::mem::transmute(handler) };
    callback(fault_addr)
}

unsafe fn chain_previous_sigsegv(
    signal: libc::c_int,
    info: *mut libc::siginfo_t,
    context: *mut libc::c_void,
) -> ! {
    let Some(state) = elastic_signal_state() else {
        unsafe { libc::_exit(128 + signal) };
    };
    let previous = &state.previous;
    let handler = previous.sa_sigaction;
    if handler == libc::SIG_IGN {
        unsafe { libc::_exit(0) };
    }
    if handler == libc::SIG_DFL {
        unsafe {
            libc::signal(signal, libc::SIG_DFL);
            libc::raise(signal);
            libc::_exit(128 + signal);
        }
    }

    if previous.sa_flags & libc::SA_SIGINFO != 0 {
        let action: extern "C" fn(libc::c_int, *mut libc::siginfo_t, *mut libc::c_void) =
            unsafe { core::mem::transmute(handler) };
        action(signal, info, context);
    } else {
        let action: extern "C" fn(libc::c_int) = unsafe { core::mem::transmute(handler) };
        action(signal);
    }
    unsafe { libc::_exit(128 + signal) };
}

fn elastic_signal_state() -> Option<&'static ElasticSignalState> {
    if ELASTIC_SIGNAL_STATE.load(Ordering::Acquire) != ELASTIC_SIGNAL_READY {
        return None;
    }
    Some(unsafe { &*(*ELASTIC_SIGNAL_STATE_STORAGE.state.get()).as_ptr() })
}

unsafe fn write_elastic_signal_state(previous: libc::sigaction) {
    unsafe {
        (*ELASTIC_SIGNAL_STATE_STORAGE.state.get()).write(ElasticSignalState { previous });
    }
}

fn signal_fd(fd: i32) -> Result<(), FiberHostError> {
    let byte = 1_u8;
    loop {
        let written = unsafe {
            libc::write(
                fd,
                (&raw const byte).cast::<c_void>(),
                core::mem::size_of::<u8>(),
            )
        };
        if written == 1 {
            return Ok(());
        }
        let errno = last_errno();
        if errno == libc::EINTR {
            continue;
        }
        if errno == libc::EAGAIN {
            return Ok(());
        }
        return Err(map_errno(errno));
    }
}

const fn map_mem_error(error: MemError) -> FiberHostError {
    match error.kind {
        MemErrorKind::Unsupported => FiberHostError::unsupported(),
        MemErrorKind::InvalidInput
        | MemErrorKind::InvalidAddress
        | MemErrorKind::Misaligned
        | MemErrorKind::OutOfBounds
        | MemErrorKind::PermissionDenied
        | MemErrorKind::Overflow => FiberHostError::invalid(),
        MemErrorKind::OutOfMemory => FiberHostError::resource_exhausted(),
        MemErrorKind::Busy | MemErrorKind::Platform(_) => FiberHostError::state_conflict(),
    }
}

const fn map_errno(errno: i32) -> FiberHostError {
    match errno {
        libc::EINVAL | libc::EBADF => FiberHostError::invalid(),
        libc::ENOMEM | libc::EMFILE | libc::ENFILE => FiberHostError::resource_exhausted(),
        libc::EAGAIN => FiberHostError::state_conflict(),
        _ => FiberHostError::platform(errno),
    }
}

fn last_errno() -> i32 {
    unsafe { *libc::__errno_location() }
}
