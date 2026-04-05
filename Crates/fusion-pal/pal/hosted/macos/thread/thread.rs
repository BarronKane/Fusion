//! macOS fusion-pal thread backend.
//!
//! This backend exposes the subset of lifecycle and scheduler-adjacent operations that can be
//! expressed honestly with Darwin `pthread` + POSIX timing primitives.

use core::ffi::c_void;
use core::mem::MaybeUninit;
use core::ptr;
use core::time::Duration;

use libc::{
    self,
    PTHREAD_CREATE_DETACHED,
    PTHREAD_CREATE_JOINABLE,
    pthread_attr_t,
    pthread_t,
};

use crate::contract::pal::runtime::thread::{
    RawThreadEntry,
    ThreadAuthoritySet,
    ThreadBaseContract,
    ThreadConfig,
    ThreadEntryReturn,
    ThreadError,
    ThreadExecutionLocation,
    ThreadGuarantee,
    ThreadId,
    ThreadIdentityStability,
    ThreadJoinPolicy,
    ThreadLifecycle,
    ThreadLifecycleCaps,
    ThreadLifecycleSupport,
    ThreadLocalitySupport,
    ThreadObservation,
    ThreadObservationControlContract,
    ThreadPlacementControlContract,
    ThreadPlacementOutcome,
    ThreadPlacementRequest,
    ThreadPlacementSupport,
    ThreadPriorityRange,
    ThreadRunState,
    ThreadSchedulerCaps,
    ThreadSchedulerClass,
    ThreadSchedulerControlContract,
    ThreadSchedulerModel,
    ThreadSchedulerObservation,
    ThreadSchedulerRequest,
    ThreadSchedulerSupport,
    ThreadStackBacking,
    ThreadStackCaps,
    ThreadStackLocalityPolicy,
    ThreadStackLockPolicy,
    ThreadStackObservation,
    ThreadStackObservationControlContract,
    ThreadStackPrefaultPolicy,
    ThreadStackSupport,
    ThreadStartMode,
    ThreadSupport,
    ThreadSuspendControlContract,
    ThreadTermination,
    ThreadTerminationKind,
};

const MACOS_THREAD_SUPPORT: ThreadSupport = ThreadSupport {
    lifecycle: ThreadLifecycleSupport {
        caps: ThreadLifecycleCaps::SPAWN
            .union(ThreadLifecycleCaps::JOIN)
            .union(ThreadLifecycleCaps::DETACH)
            .union(ThreadLifecycleCaps::CURRENT_THREAD_ID)
            .union(ThreadLifecycleCaps::CURRENT_OBSERVE)
            .union(ThreadLifecycleCaps::EXIT_CODE),
        identity_stability: ThreadIdentityStability::ThreadLifetime,
        authorities: ThreadAuthoritySet::OPERATING_SYSTEM,
        implementation: crate::contract::pal::runtime::thread::ThreadImplementationKind::Native,
    },
    placement: ThreadPlacementSupport::unsupported(),
    scheduler: ThreadSchedulerSupport {
        caps: ThreadSchedulerCaps::YIELD
            .union(ThreadSchedulerCaps::SLEEP_FOR)
            .union(ThreadSchedulerCaps::MONOTONIC_NOW),
        model: ThreadSchedulerModel::Preemptive,
        priority: ThreadGuarantee::Unsupported,
        realtime: ThreadGuarantee::Unsupported,
        deadline: ThreadGuarantee::Unsupported,
        observation: ThreadGuarantee::Unknown,
        default_priority_range: None,
        authorities: ThreadAuthoritySet::OPERATING_SYSTEM,
        implementation: crate::contract::pal::runtime::thread::ThreadImplementationKind::Native,
    },
    stack: ThreadStackSupport {
        caps: ThreadStackCaps::EXPLICIT_SIZE,
        explicit_size: ThreadGuarantee::Enforced,
        caller_provided: ThreadGuarantee::Unsupported,
        prefault: ThreadGuarantee::Unsupported,
        lock: ThreadGuarantee::Unsupported,
        locality: ThreadGuarantee::Unsupported,
        usage_observation: ThreadGuarantee::Unsupported,
        authorities: ThreadAuthoritySet::OPERATING_SYSTEM,
        implementation: crate::contract::pal::runtime::thread::ThreadImplementationKind::Native,
    },
    locality: ThreadLocalitySupport::unsupported(),
};

/// macOS thread provider handle.
#[derive(Debug, Clone, Copy, Default)]
pub struct MacOsThread;

/// macOS owned thread handle.
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct MacOsThreadHandle {
    pthread: pthread_t,
    tid: ThreadId,
    joinable: bool,
}

/// Selected thread handle type for macOS builds.
pub type PlatformThreadHandle = MacOsThreadHandle;

/// Target-selected thread provider alias for macOS builds.
pub type PlatformThread = MacOsThread;

/// Returns the process-wide macOS thread provider handle.
#[must_use]
pub const fn system_thread() -> PlatformThread {
    PlatformThread::new()
}

impl MacOsThread {
    /// Creates a new macOS thread provider handle.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl ThreadBaseContract for MacOsThread {
    type Handle = MacOsThreadHandle;

    fn support(&self) -> ThreadSupport {
        MACOS_THREAD_SUPPORT
    }
}

#[repr(C)]
struct ThreadStart {
    entry: RawThreadEntry,
    context: *mut (),
}

extern "C" fn macos_thread_entry(arg: *mut c_void) -> *mut c_void {
    let start = unsafe {
        let ptr = arg.cast::<ThreadStart>();
        let value = ptr.read();
        libc::free(ptr.cast::<c_void>());
        value
    };

    let returned = unsafe { (start.entry)(start.context) };
    returned.code.0 as *mut c_void
}

// SAFETY: this implementation owns thread creation and lifecycle through pthread-backed
// handles and does not expose internal invariants except through the fusion-pal contract.
unsafe impl ThreadLifecycle for MacOsThread {
    unsafe fn spawn(
        &self,
        config: &ThreadConfig<'_>,
        entry: RawThreadEntry,
        context: *mut (),
    ) -> Result<Self::Handle, ThreadError> {
        validate_spawn_config(config)?;

        let mut attr = PthreadAttr::new()?;
        configure_attr(&mut attr, config)?;

        let start_ptr = unsafe { libc::malloc(core::mem::size_of::<ThreadStart>()) };
        if start_ptr.is_null() {
            return Err(ThreadError::resource_exhausted());
        }

        unsafe {
            start_ptr
                .cast::<ThreadStart>()
                .write(ThreadStart { entry, context });
        }

        let mut pthread = MaybeUninit::<pthread_t>::uninit();
        let rc = unsafe {
            libc::pthread_create(
                pthread.as_mut_ptr(),
                attr.as_ptr(),
                macos_thread_entry,
                start_ptr,
            )
        };

        if rc != 0 {
            unsafe { libc::free(start_ptr) };
            return Err(map_pthread_error(rc));
        }

        let pthread = unsafe { pthread.assume_init() };
        let tid = thread_id_for_pthread(pthread)?;

        Ok(MacOsThreadHandle {
            pthread,
            tid,
            joinable: config.join_policy == ThreadJoinPolicy::Joinable,
        })
    }

    fn current_thread_id(&self) -> Result<ThreadId, ThreadError> {
        current_thread_id_macos()
    }

    fn join(&self, handle: Self::Handle) -> Result<ThreadTermination, ThreadError> {
        if !handle.joinable {
            return Err(ThreadError::state_conflict());
        }

        let mut result = ptr::null_mut();
        let rc = unsafe { libc::pthread_join(handle.pthread, &raw mut result) };
        if rc != 0 {
            return Err(map_pthread_error(rc));
        }

        if is_pthread_canceled(result) {
            return Ok(ThreadTermination {
                kind: ThreadTerminationKind::Canceled,
                code: None,
            });
        }

        Ok(ThreadTermination::from_entry_return(ThreadEntryReturn {
            code: crate::contract::pal::runtime::thread::ThreadExitCode(result as usize),
        }))
    }

    fn detach(&self, handle: Self::Handle) -> Result<(), ThreadError> {
        if !handle.joinable {
            return Err(ThreadError::state_conflict());
        }

        let rc = unsafe { libc::pthread_detach(handle.pthread) };
        if rc == 0 {
            Ok(())
        } else {
            Err(map_pthread_error(rc))
        }
    }
}

impl ThreadSuspendControlContract for MacOsThread {
    fn suspend(&self, _handle: &Self::Handle) -> Result<(), ThreadError> {
        Err(ThreadError::unsupported())
    }

    fn resume(&self, _handle: &Self::Handle) -> Result<(), ThreadError> {
        Err(ThreadError::unsupported())
    }
}

impl ThreadSchedulerControlContract for MacOsThread {
    fn priority_range(
        &self,
        _class: ThreadSchedulerClass,
    ) -> Result<Option<ThreadPriorityRange>, ThreadError> {
        Err(ThreadError::unsupported())
    }

    fn set_scheduler(
        &self,
        _handle: &Self::Handle,
        _request: &ThreadSchedulerRequest,
    ) -> Result<ThreadSchedulerObservation, ThreadError> {
        Err(ThreadError::unsupported())
    }

    fn scheduler(&self, _handle: &Self::Handle) -> Result<ThreadSchedulerObservation, ThreadError> {
        Err(ThreadError::unsupported())
    }

    fn yield_now(&self) -> Result<(), ThreadError> {
        let rc = unsafe { libc::sched_yield() };
        if rc == 0 {
            Ok(())
        } else {
            Err(map_errno(last_errno()))
        }
    }

    fn sleep_for(&self, duration: Duration) -> Result<(), ThreadError> {
        let request = duration_to_timespec(duration)?;
        let rc = unsafe { libc::nanosleep(&raw const request, ptr::null_mut()) };
        if rc == 0 {
            Ok(())
        } else {
            Err(map_errno(last_errno()))
        }
    }

    fn monotonic_now(&self) -> Result<Duration, ThreadError> {
        let mut ts = MaybeUninit::<libc::timespec>::uninit();
        let rc = unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, ts.as_mut_ptr()) };
        if rc != 0 {
            return Err(map_errno(last_errno()));
        }

        timespec_to_duration(unsafe { ts.assume_init() })
    }
}

impl ThreadPlacementControlContract for MacOsThread {
    fn set_placement(
        &self,
        _handle: &Self::Handle,
        _request: &ThreadPlacementRequest<'_>,
    ) -> Result<ThreadPlacementOutcome, ThreadError> {
        Err(ThreadError::unsupported())
    }

    fn placement(&self, _handle: &Self::Handle) -> Result<ThreadPlacementOutcome, ThreadError> {
        Err(ThreadError::unsupported())
    }
}

impl ThreadObservationControlContract for MacOsThread {
    fn observe_current(&self) -> Result<ThreadObservation, ThreadError> {
        Ok(ThreadObservation {
            id: current_thread_id_macos()?,
            run_state: ThreadRunState::Running,
            location: ThreadExecutionLocation::unknown(),
            scheduler: ThreadSchedulerObservation::unknown(),
            placement: ThreadPlacementOutcome::unsupported(),
        })
    }

    fn observe(&self, _handle: &Self::Handle) -> Result<ThreadObservation, ThreadError> {
        Err(ThreadError::unsupported())
    }
}

impl ThreadStackObservationControlContract for MacOsThread {
    fn observe_current_stack(&self) -> Result<ThreadStackObservation, ThreadError> {
        Err(ThreadError::unsupported())
    }

    fn observe_stack(&self, _handle: &Self::Handle) -> Result<ThreadStackObservation, ThreadError> {
        Err(ThreadError::unsupported())
    }
}

fn validate_spawn_config(config: &ThreadConfig<'_>) -> Result<(), ThreadError> {
    if config.name.is_some() {
        return Err(ThreadError::unsupported());
    }

    if config.start_mode != ThreadStartMode::Immediate {
        return Err(ThreadError::unsupported());
    }

    if config.placement.has_targets() {
        return Err(ThreadError::placement_denied());
    }

    if config.scheduler.class != ThreadSchedulerClass::Inherit
        || config.scheduler.priority.is_some()
        || config.scheduler.deadline.is_some()
    {
        return Err(ThreadError::scheduler_denied());
    }

    if !matches!(config.stack.backing, ThreadStackBacking::Default) {
        return Err(ThreadError::stack_denied());
    }

    if !matches!(
        config.stack.prefault,
        ThreadStackPrefaultPolicy::Inherit | ThreadStackPrefaultPolicy::Disabled
    ) {
        return Err(ThreadError::stack_denied());
    }

    if !matches!(
        config.stack.lock,
        ThreadStackLockPolicy::Inherit | ThreadStackLockPolicy::Disabled
    ) {
        return Err(ThreadError::stack_denied());
    }

    if !matches!(
        config.stack.locality,
        ThreadStackLocalityPolicy::InheritProcessPolicy
    ) {
        return Err(ThreadError::stack_denied());
    }

    Ok(())
}

struct PthreadAttr {
    inner: pthread_attr_t,
}

impl PthreadAttr {
    fn new() -> Result<Self, ThreadError> {
        let mut inner = MaybeUninit::<pthread_attr_t>::uninit();
        let rc = unsafe { libc::pthread_attr_init(inner.as_mut_ptr()) };
        if rc != 0 {
            return Err(map_pthread_error(rc));
        }

        Ok(Self {
            inner: unsafe { inner.assume_init() },
        })
    }

    fn as_ptr(&self) -> *const pthread_attr_t {
        &self.inner
    }

    fn as_mut_ptr(&mut self) -> *mut pthread_attr_t {
        &mut self.inner
    }
}

impl Drop for PthreadAttr {
    fn drop(&mut self) {
        let _ = unsafe { libc::pthread_attr_destroy(self.as_mut_ptr()) };
    }
}

fn configure_attr(attr: &mut PthreadAttr, config: &ThreadConfig<'_>) -> Result<(), ThreadError> {
    let detach_state = match config.join_policy {
        ThreadJoinPolicy::Joinable => PTHREAD_CREATE_JOINABLE,
        ThreadJoinPolicy::Detached => PTHREAD_CREATE_DETACHED,
    };

    let rc = unsafe { libc::pthread_attr_setdetachstate(attr.as_mut_ptr(), detach_state) };
    if rc != 0 {
        return Err(map_pthread_error(rc));
    }

    if let Some(size) = config.stack.size_bytes {
        let rc = unsafe { libc::pthread_attr_setstacksize(attr.as_mut_ptr(), size.get()) };
        if rc != 0 {
            return Err(ThreadError::stack_denied());
        }
    }

    if config.stack.guard_bytes.is_some() {
        // libc's exported pthread surface for Darwin in this toolchain does not expose a
        // configurable guard-size setter, so reject explicit guard requests.
        return Err(ThreadError::stack_denied());
    }

    Ok(())
}

fn current_thread_id_macos() -> Result<ThreadId, ThreadError> {
    thread_id_for_pthread(0)
}

fn thread_id_for_pthread(thread: pthread_t) -> Result<ThreadId, ThreadError> {
    let mut out = 0_u64;
    let rc = unsafe { libc::pthread_threadid_np(thread, &raw mut out) };
    if rc == 0 {
        Ok(ThreadId(out))
    } else {
        Err(map_pthread_error(rc))
    }
}

fn is_pthread_canceled(value: *mut c_void) -> bool {
    value == libc::PTHREAD_CANCELED
}

const fn map_pthread_error(code: i32) -> ThreadError {
    match code {
        libc::EINVAL => ThreadError::invalid(),
        libc::EAGAIN => ThreadError::resource_exhausted(),
        libc::EPERM | libc::EACCES => ThreadError::permission_denied(),
        libc::EBUSY => ThreadError::busy(),
        libc::ENOTSUP | libc::EOPNOTSUPP => ThreadError::unsupported(),
        libc::ESRCH => ThreadError::state_conflict(),
        _ => ThreadError::platform(code),
    }
}

fn duration_to_timespec(duration: Duration) -> Result<libc::timespec, ThreadError> {
    let seconds = i64::try_from(duration.as_secs()).map_err(|_| ThreadError::invalid())?;
    let nanos = i64::from(duration.subsec_nanos());
    Ok(libc::timespec {
        tv_sec: seconds,
        tv_nsec: nanos,
    })
}

fn timespec_to_duration(ts: libc::timespec) -> Result<Duration, ThreadError> {
    let seconds = u64::try_from(ts.tv_sec).map_err(|_| ThreadError::platform(libc::EINVAL))?;
    let nanos = u32::try_from(ts.tv_nsec).map_err(|_| ThreadError::platform(libc::EINVAL))?;
    if nanos >= 1_000_000_000 {
        return Err(ThreadError::platform(libc::EINVAL));
    }
    Ok(Duration::new(seconds, nanos))
}

fn last_errno() -> i32 {
    unsafe { *libc::__error() }
}

const fn map_errno(errno: i32) -> ThreadError {
    map_pthread_error(errno)
}
