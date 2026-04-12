//! Windows fusion-pal thread backend.
//!
//! This backend exposes the subset of Win32 thread lifecycle and placement behavior it can
//! justify without inventing folklore. Thread creation, naming, current/handle observation,
//! single-group logical CPU affinity, voluntary yield, relative sleep, monotonic time, and
//! base-priority observation are surfaced natively. Scheduler-class control, external
//! suspension, and stack-policy control remain unsupported because the hosted Win32 surface
//! does not let this PAL promise them honestly.

use core::ffi::c_void;
use core::time::Duration;

use std::boxed::Box;
use std::panic::{
    AssertUnwindSafe,
    catch_unwind,
};
use std::sync::{
    Arc,
    Condvar,
    Mutex,
    MutexGuard,
    PoisonError,
};

use windows::core::HSTRING;
use windows::Win32::Foundation::{
    CloseHandle,
    ERROR_ACCESS_DENIED,
    ERROR_BUSY,
    ERROR_INVALID_HANDLE,
    ERROR_INVALID_PARAMETER,
    ERROR_NOT_ENOUGH_MEMORY,
    ERROR_NOT_SUPPORTED,
    ERROR_OUTOFMEMORY,
    ERROR_TIMEOUT,
    GetLastError,
    HANDLE,
    WAIT_FAILED,
    WAIT_OBJECT_0,
    WAIT_TIMEOUT,
    WIN32_ERROR,
};
use windows::Win32::System::Kernel::PROCESSOR_NUMBER;
use windows::Win32::System::SystemInformation::{
    GROUP_AFFINITY,
    GetTickCount64,
};
use windows::Win32::System::Threading::{
    CreateThread,
    GetCurrentProcessorNumberEx,
    GetCurrentThread,
    GetCurrentThreadId,
    GetThreadGroupAffinity,
    GetThreadPriority,
    INFINITE,
    SetThreadDescription,
    SetThreadGroupAffinity,
    Sleep,
    SwitchToThread,
    THREAD_CREATE_RUN_IMMEDIATELY,
    WaitForSingleObject,
};

use crate::contract::pal::runtime::thread::{
    RawThreadEntry,
    ThreadAuthoritySet,
    ThreadBaseContract,
    ThreadConfig,
    ThreadConstraintMode,
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
    ThreadPlacementCaps,
    ThreadPlacementControlContract,
    ThreadPlacementOutcome,
    ThreadPlacementPhase,
    ThreadPlacementRequest,
    ThreadPlacementSupport,
    ThreadPlacementTarget,
    ThreadProcessorGroupId,
    ThreadPriority,
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
    ThreadStackObservation,
    ThreadStackObservationControlContract,
    ThreadStackPrefaultPolicy,
    ThreadStackRequest,
    ThreadStackSupport,
    ThreadStartMode,
    ThreadSupport,
    ThreadSuspendControlContract,
    ThreadTermination,
    ThreadTerminationKind,
};

const THREAD_PRIORITY_ERROR_RETURN: i32 = 0x7fff_ffff;

const WINDOWS_THREAD_SUPPORT: ThreadSupport = ThreadSupport {
    lifecycle: ThreadLifecycleSupport {
        caps: ThreadLifecycleCaps::SPAWN
            .union(ThreadLifecycleCaps::JOIN)
            .union(ThreadLifecycleCaps::DETACH)
            .union(ThreadLifecycleCaps::NAME)
            .union(ThreadLifecycleCaps::CURRENT_THREAD_ID)
            .union(ThreadLifecycleCaps::CURRENT_OBSERVE)
            .union(ThreadLifecycleCaps::HANDLE_OBSERVE)
            .union(ThreadLifecycleCaps::EXIT_CODE),
        identity_stability: ThreadIdentityStability::ThreadLifetime,
        authorities: ThreadAuthoritySet::OPERATING_SYSTEM,
        implementation: crate::contract::pal::runtime::thread::ThreadImplementationKind::Native,
    },
    placement: ThreadPlacementSupport {
        caps: ThreadPlacementCaps::LOGICAL_CPU_AFFINITY
            .union(ThreadPlacementCaps::PRESTART_APPLICATION)
            .union(ThreadPlacementCaps::POSTSTART_APPLICATION)
            .union(ThreadPlacementCaps::CURRENT_CPU_OBSERVE)
            .union(ThreadPlacementCaps::EFFECTIVE_OBSERVE),
        logical_cpu_affinity: ThreadGuarantee::Enforced,
        package_affinity: ThreadGuarantee::Unsupported,
        numa_affinity: ThreadGuarantee::Unsupported,
        core_class_affinity: ThreadGuarantee::Unsupported,
        observation: ThreadGuarantee::Verified,
        authorities: ThreadAuthoritySet::OPERATING_SYSTEM,
        implementation: crate::contract::pal::runtime::thread::ThreadImplementationKind::Native,
    },
    scheduler: ThreadSchedulerSupport {
        caps: ThreadSchedulerCaps::YIELD
            .union(ThreadSchedulerCaps::SLEEP_FOR)
            .union(ThreadSchedulerCaps::MONOTONIC_NOW)
            .union(ThreadSchedulerCaps::QUERY_PRIORITY),
        model: ThreadSchedulerModel::Preemptive,
        priority: ThreadGuarantee::Unsupported,
        realtime: ThreadGuarantee::Unsupported,
        deadline: ThreadGuarantee::Unsupported,
        observation: ThreadGuarantee::Verified,
        default_priority_range: None,
        authorities: ThreadAuthoritySet::OPERATING_SYSTEM,
        implementation: crate::contract::pal::runtime::thread::ThreadImplementationKind::Native,
    },
    stack: ThreadStackSupport::unsupported(),
    locality: ThreadLocalitySupport::unsupported(),
};

/// Windows thread provider handle.
#[derive(Debug, Clone, Copy, Default)]
pub struct WindowsThread;

/// Windows owned thread handle.
#[derive(Debug)]
pub struct WindowsThreadHandle {
    handle: Option<HANDLE>,
    tid: ThreadId,
    joinable: bool,
    termination: Option<Arc<ThreadTerminationCell>>,
}

/// Selected thread handle type for Windows builds.
pub type PlatformThreadHandle = WindowsThreadHandle;

/// Target-selected thread provider alias for Windows builds.
pub type PlatformThread = WindowsThread;

/// Returns the process-wide Windows thread provider handle.
#[must_use]
pub const fn system_thread() -> PlatformThread {
    PlatformThread::new()
}

impl WindowsThread {
    /// Creates a new Windows thread provider handle.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl ThreadBaseContract for WindowsThread {
    type Handle = WindowsThreadHandle;

    fn support(&self) -> ThreadSupport {
        WINDOWS_THREAD_SUPPORT
    }
}

// SAFETY: this implementation owns thread creation and lifecycle through Win32 thread handles
// and only exposes those capabilities through the fusion-pal contract.
unsafe impl ThreadLifecycle for WindowsThread {
    unsafe fn spawn(
        &self,
        config: &ThreadConfig<'_>,
        entry: RawThreadEntry,
        context: *mut (),
    ) -> Result<Self::Handle, ThreadError> {
        validate_spawn_config(config)?;

        let startup = Arc::new(ThreadStartup::new());
        let termination = Arc::new(ThreadTerminationCell::new());
        let start = Box::new(ThreadStart {
            entry,
            context,
            startup: Arc::clone(&startup),
            termination: Arc::clone(&termination),
        });
        let start_ptr = Box::into_raw(start).cast::<c_void>();

        let mut tid = 0_u32;
        let handle = match unsafe {
            CreateThread(
                None,
                0,
                Some(windows_thread_entry),
                Some(start_ptr.cast_const()),
                THREAD_CREATE_RUN_IMMEDIATELY,
                Some(&mut tid),
            )
        } {
            Ok(handle) => handle,
            Err(error) => {
                let _ = unsafe { Box::from_raw(start_ptr.cast::<ThreadStart>()) };
                return Err(map_hresult(error.code().0));
            }
        };

        let configure_result = configure_spawned_thread(handle, config);
        if let Err(error) = configure_result {
            startup.fail(error);
            let _ = wait_for_exit(handle);
            let _ = unsafe { CloseHandle(handle) };
            return Err(error);
        }

        startup.ready();

        if config.join_policy == ThreadJoinPolicy::Detached {
            let _ = unsafe { CloseHandle(handle) };
            return Ok(WindowsThreadHandle {
                handle: None,
                tid: ThreadId(u64::from(tid)),
                joinable: false,
                termination: None,
            });
        }

        Ok(WindowsThreadHandle {
            handle: Some(handle),
            tid: ThreadId(u64::from(tid)),
            joinable: true,
            termination: Some(termination),
        })
    }

    fn current_thread_id(&self) -> Result<ThreadId, ThreadError> {
        Ok(ThreadId(u64::from(unsafe { GetCurrentThreadId() })))
    }

    fn join(&self, mut handle: Self::Handle) -> Result<ThreadTermination, ThreadError> {
        if !handle.joinable {
            return Err(ThreadError::state_conflict());
        }

        let raw = handle.handle.take().ok_or(ThreadError::state_conflict())?;
        wait_for_exit(raw)?;

        let termination = handle.termination.take().map_or(
            ThreadTermination {
                kind: ThreadTerminationKind::Unknown,
                code: None,
            },
            |state| state.take(),
        );

        unsafe { CloseHandle(raw).map_err(|error| map_hresult(error.code().0))? };
        Ok(termination)
    }

    fn detach(&self, mut handle: Self::Handle) -> Result<(), ThreadError> {
        if !handle.joinable {
            return Err(ThreadError::state_conflict());
        }

        let raw = handle.handle.take().ok_or(ThreadError::state_conflict())?;
        handle.termination.take();
        unsafe { CloseHandle(raw).map_err(|error| map_hresult(error.code().0))? };
        Ok(())
    }
}

impl ThreadSuspendControlContract for WindowsThread {
    fn suspend(&self, _handle: &Self::Handle) -> Result<(), ThreadError> {
        Err(ThreadError::unsupported())
    }

    fn resume(&self, _handle: &Self::Handle) -> Result<(), ThreadError> {
        Err(ThreadError::unsupported())
    }
}

impl ThreadSchedulerControlContract for WindowsThread {
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

    fn scheduler(&self, handle: &Self::Handle) -> Result<ThreadSchedulerObservation, ThreadError> {
        scheduler_observation_for_handle(thread_handle(handle)?)
    }

    fn yield_now(&self) -> Result<(), ThreadError> {
        let _ = unsafe { SwitchToThread() };
        Ok(())
    }

    fn sleep_for(&self, duration: Duration) -> Result<(), ThreadError> {
        let mut remaining_ms = duration_to_sleep_ms(duration)?;
        while remaining_ms != 0 {
            let chunk = remaining_ms.min(u128::from(u32::MAX - 1));
            unsafe { Sleep(chunk as u32) };
            remaining_ms -= chunk;
        }
        Ok(())
    }

    fn monotonic_now(&self) -> Result<Duration, ThreadError> {
        Ok(Duration::from_millis(unsafe { GetTickCount64() }))
    }
}

impl ThreadPlacementControlContract for WindowsThread {
    fn set_placement(
        &self,
        handle: &Self::Handle,
        request: &ThreadPlacementRequest<'_>,
    ) -> Result<ThreadPlacementOutcome, ThreadError> {
        apply_affinity_request(thread_handle(handle)?, request, false)
    }

    fn placement(&self, handle: &Self::Handle) -> Result<ThreadPlacementOutcome, ThreadError> {
        placement_outcome_for_handle(thread_handle(handle)?)
    }
}

impl ThreadObservationControlContract for WindowsThread {
    fn observe_current(&self) -> Result<ThreadObservation, ThreadError> {
        Ok(ThreadObservation {
            id: self.current_thread_id()?,
            run_state: ThreadRunState::Running,
            location: current_execution_location(),
            scheduler: scheduler_observation_for_handle(unsafe { GetCurrentThread() })?,
            placement: ThreadPlacementOutcome {
                guarantee: WINDOWS_THREAD_SUPPORT.placement.observation,
                phase: ThreadPlacementPhase::Inherit,
                location: current_execution_location(),
            },
        })
    }

    fn observe(&self, handle: &Self::Handle) -> Result<ThreadObservation, ThreadError> {
        let raw = thread_handle(handle)?;
        let run_state = match unsafe { WaitForSingleObject(raw, 0) } {
            WAIT_OBJECT_0 => ThreadRunState::Exited,
            WAIT_TIMEOUT => ThreadRunState::Unknown,
            WAIT_FAILED => return Err(map_win32_error(unsafe { GetLastError() })),
            other => return Err(ThreadError::platform(other.0 as i32)),
        };

        let placement = placement_outcome_for_handle(raw)?;
        Ok(ThreadObservation {
            id: handle.tid,
            run_state,
            location: placement.location,
            scheduler: scheduler_observation_for_handle(raw)?,
            placement,
        })
    }
}

impl ThreadStackObservationControlContract for WindowsThread {
    fn observe_current_stack(&self) -> Result<ThreadStackObservation, ThreadError> {
        Err(ThreadError::unsupported())
    }

    fn observe_stack(&self, _handle: &Self::Handle) -> Result<ThreadStackObservation, ThreadError> {
        Err(ThreadError::unsupported())
    }
}

impl Drop for WindowsThreadHandle {
    fn drop(&mut self) {
        if let Some(handle) = self.handle {
            let rc = unsafe { CloseHandle(handle) };
            debug_assert!(rc.is_ok());
        }
    }
}

// SAFETY: the owned handle refers to a process-local kernel object and the rest of the state is
// synchronized through `Arc<Mutex<_>>`.
unsafe impl Send for WindowsThreadHandle {}
// SAFETY: shared references do not grant unsynchronized interior access to the underlying state.
unsafe impl Sync for WindowsThreadHandle {}

#[derive(Debug)]
struct ThreadTerminationCell {
    inner: Mutex<Option<ThreadTermination>>,
}

impl ThreadTerminationCell {
    fn new() -> Self {
        Self {
            inner: Mutex::new(None),
        }
    }

    fn store(&self, termination: ThreadTermination) {
        *lock_unpoisoned(&self.inner) = Some(termination);
    }

    fn take(&self) -> ThreadTermination {
        lock_unpoisoned(&self.inner)
            .take()
            .unwrap_or(ThreadTermination {
                kind: ThreadTerminationKind::Unknown,
                code: None,
            })
    }
}

#[derive(Debug)]
struct ThreadStartup {
    state: Mutex<ThreadStartupState>,
    ready: Condvar,
}

impl ThreadStartup {
    fn new() -> Self {
        Self {
            state: Mutex::new(ThreadStartupState::Pending),
            ready: Condvar::new(),
        }
    }

    fn ready(&self) {
        *lock_unpoisoned(&self.state) = ThreadStartupState::Ready;
        self.ready.notify_all();
    }

    fn fail(&self, error: ThreadError) {
        *lock_unpoisoned(&self.state) = ThreadStartupState::Failed(error);
        self.ready.notify_all();
    }

    fn wait(&self) -> Result<(), ThreadError> {
        let mut guard = lock_unpoisoned(&self.state);
        loop {
            match *guard {
                ThreadStartupState::Pending => {
                    guard = wait_unpoisoned(&self.ready, guard);
                }
                ThreadStartupState::Ready => return Ok(()),
                ThreadStartupState::Failed(error) => return Err(error),
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum ThreadStartupState {
    Pending,
    Ready,
    Failed(ThreadError),
}

struct ThreadStart {
    entry: RawThreadEntry,
    context: *mut (),
    startup: Arc<ThreadStartup>,
    termination: Arc<ThreadTerminationCell>,
}

unsafe extern "system" fn windows_thread_entry(arg: *mut c_void) -> u32 {
    let start = unsafe { Box::from_raw(arg.cast::<ThreadStart>()) };
    if let Err(_error) = start.startup.wait() {
        return 0;
    }

    let termination =
        match catch_unwind(AssertUnwindSafe(|| unsafe { (start.entry)(start.context) })) {
            Ok(returned) => ThreadTermination::from_entry_return(returned),
            Err(_) => ThreadTermination {
                kind: ThreadTerminationKind::Aborted,
                code: None,
            },
        };
    start.termination.store(termination);
    encode_exit_code(termination)
}

fn validate_spawn_config(config: &ThreadConfig<'_>) -> Result<(), ThreadError> {
    validate_thread_name(config.name)?;
    validate_stack_request(&config.stack)?;
    validate_placement_request(&config.placement)?;

    if config.scheduler.class != ThreadSchedulerClass::Inherit
        || config.scheduler.priority.is_some()
        || config.scheduler.deadline.is_some()
    {
        return Err(ThreadError::scheduler_denied());
    }

    match config.start_mode {
        ThreadStartMode::Immediate
        | ThreadStartMode::PlacementCommitted
        | ThreadStartMode::PlacementAndStackCommitted => {}
    }

    Ok(())
}

fn validate_thread_name(name: Option<&str>) -> Result<(), ThreadError> {
    if let Some(name) = name
        && name.as_bytes().contains(&0)
    {
        return Err(ThreadError::invalid());
    }
    Ok(())
}

fn validate_stack_request(request: &ThreadStackRequest) -> Result<(), ThreadError> {
    if request.size_bytes.is_some() || request.guard_bytes.is_some() {
        return Err(ThreadError::stack_denied());
    }

    if !matches!(request.backing, ThreadStackBacking::Default) {
        return Err(ThreadError::stack_denied());
    }

    if !matches!(request.prefault, ThreadStackPrefaultPolicy::Inherit) {
        return Err(ThreadError::stack_denied());
    }

    if !matches!(
        request.lock,
        crate::contract::pal::runtime::thread::ThreadStackLockPolicy::Inherit
    ) {
        return Err(ThreadError::stack_denied());
    }

    if !matches!(
        request.locality,
        crate::contract::pal::runtime::thread::ThreadStackLocalityPolicy::InheritProcessPolicy
    ) {
        return Err(ThreadError::stack_denied());
    }

    Ok(())
}

fn validate_placement_request(request: &ThreadPlacementRequest<'_>) -> Result<(), ThreadError> {
    if request.has_non_logical_targets() {
        return Err(ThreadError::placement_denied());
    }

    if matches!(
        request.migration,
        crate::contract::pal::runtime::thread::ThreadMigrationPolicy::Disallow
    ) && request.logical_cpu_count() != 1
    {
        return Err(ThreadError::placement_denied());
    }

    if request.logical_cpu_count() == 0 {
        return Ok(());
    }

    let mut group = None;
    for target in request.targets {
        if let ThreadPlacementTarget::LogicalCpus(cpus) = target {
            for cpu in *cpus {
                if usize::from(cpu.index) >= usize::BITS as usize {
                    return Err(ThreadError::placement_denied());
                }
                if let Some(expected) = group {
                    if expected != cpu.group {
                        return Err(ThreadError::placement_denied());
                    }
                } else {
                    group = Some(cpu.group);
                }
            }
        }
    }

    Ok(())
}

fn configure_spawned_thread(handle: HANDLE, config: &ThreadConfig<'_>) -> Result<(), ThreadError> {
    if let Some(name) = config.name {
        set_thread_name(handle, name)?;
    }

    if placement_request_has_constraints(&config.placement) {
        let _ = apply_affinity_request(handle, &config.placement, true)?;
    }

    Ok(())
}

fn placement_request_has_constraints(request: &ThreadPlacementRequest<'_>) -> bool {
    request.logical_cpu_count() != 0
}

fn set_thread_name(handle: HANDLE, name: &str) -> Result<(), ThreadError> {
    let wide = HSTRING::from(name);
    unsafe { SetThreadDescription(handle, &wide).map_err(|error| map_hresult(error.code().0)) }
}

fn apply_affinity_request(
    handle: HANDLE,
    request: &ThreadPlacementRequest<'_>,
    startup_phase: bool,
) -> Result<ThreadPlacementOutcome, ThreadError> {
    if request.logical_cpu_count() == 0 {
        if matches!(request.mode, ThreadConstraintMode::Require)
            && matches!(
                request.migration,
                crate::contract::pal::runtime::thread::ThreadMigrationPolicy::Disallow
            )
        {
            return Err(ThreadError::placement_denied());
        }
        return Ok(ThreadPlacementOutcome::unsupported());
    }

    let affinity = affinity_from_request(request)?;
    let ok = unsafe { SetThreadGroupAffinity(handle, &raw const affinity, None) };
    if !ok.as_bool() {
        return Err(map_win32_error(unsafe { GetLastError() }));
    }

    Ok(ThreadPlacementOutcome {
        guarantee: WINDOWS_THREAD_SUPPORT.placement.logical_cpu_affinity,
        phase: if startup_phase {
            match request.phase {
                ThreadPlacementPhase::PreStartRequired => ThreadPlacementPhase::PreStartRequired,
                _ => ThreadPlacementPhase::PreStartPreferred,
            }
        } else {
            ThreadPlacementPhase::PostStartAllowed
        },
        location: location_from_affinity(affinity),
    })
}

fn affinity_from_request(
    request: &ThreadPlacementRequest<'_>,
) -> Result<GROUP_AFFINITY, ThreadError> {
    let mut mask = 0usize;
    let mut group = None;

    for target in request.targets {
        if let ThreadPlacementTarget::LogicalCpus(cpus) = target {
            for cpu in *cpus {
                group = Some(group.map_or(cpu.group, |current: ThreadProcessorGroupId| {
                    if current == cpu.group {
                        current
                    } else {
                        ThreadProcessorGroupId(u16::MAX)
                    }
                }));
                mask |= 1usize << usize::from(cpu.index);
            }
        }
    }

    let Some(group) = group else {
        return Err(ThreadError::placement_denied());
    };
    if group.0 == u16::MAX {
        return Err(ThreadError::placement_denied());
    }

    Ok(GROUP_AFFINITY {
        Mask: mask,
        Group: group.0,
        Reserved: [0; 3],
    })
}

fn placement_outcome_for_handle(handle: HANDLE) -> Result<ThreadPlacementOutcome, ThreadError> {
    let mut affinity = GROUP_AFFINITY::default();
    let ok = unsafe { GetThreadGroupAffinity(handle, &raw mut affinity) };
    if !ok.as_bool() {
        return Err(map_win32_error(unsafe { GetLastError() }));
    }

    Ok(ThreadPlacementOutcome {
        guarantee: WINDOWS_THREAD_SUPPORT.placement.observation,
        phase: ThreadPlacementPhase::Inherit,
        location: location_from_affinity(affinity),
    })
}

fn location_from_affinity(affinity: GROUP_AFFINITY) -> ThreadExecutionLocation {
    let mut location = ThreadExecutionLocation::unknown();
    if affinity.Mask.count_ones() == 1 {
        let index = affinity.Mask.trailing_zeros();
        if let Ok(index) = u16::try_from(index) {
            location.logical_cpu =
                Some(crate::contract::pal::runtime::thread::ThreadLogicalCpuId {
                    group: ThreadProcessorGroupId(affinity.Group),
                    index,
                });
        }
    }
    location
}

fn current_execution_location() -> ThreadExecutionLocation {
    let processor: PROCESSOR_NUMBER = unsafe { GetCurrentProcessorNumberEx() };
    let mut location = ThreadExecutionLocation::unknown();
    location.logical_cpu = Some(crate::contract::pal::runtime::thread::ThreadLogicalCpuId {
        group: ThreadProcessorGroupId(processor.Group),
        index: u16::from(processor.Number),
    });
    location
}

fn scheduler_observation_for_handle(
    handle: HANDLE,
) -> Result<ThreadSchedulerObservation, ThreadError> {
    let priority = unsafe { GetThreadPriority(handle) };
    if priority == THREAD_PRIORITY_ERROR_RETURN {
        return Err(map_win32_error(unsafe { GetLastError() }));
    }

    Ok(ThreadSchedulerObservation {
        class: None,
        base_priority: Some(ThreadPriority(priority)),
        effective_priority: None,
    })
}

fn wait_for_exit(handle: HANDLE) -> Result<(), ThreadError> {
    match unsafe { WaitForSingleObject(handle, INFINITE) } {
        WAIT_OBJECT_0 => Ok(()),
        WAIT_TIMEOUT => Err(ThreadError::timeout()),
        WAIT_FAILED => Err(map_win32_error(unsafe { GetLastError() })),
        other => Err(ThreadError::platform(other.0 as i32)),
    }
}

fn thread_handle(handle: &WindowsThreadHandle) -> Result<HANDLE, ThreadError> {
    handle.handle.ok_or(ThreadError::state_conflict())
}

fn duration_to_sleep_ms(duration: Duration) -> Result<u128, ThreadError> {
    let millis = duration.as_millis();
    if millis != 0 || duration.subsec_nanos() == 0 {
        return Ok(millis);
    }
    millis.checked_add(1).ok_or(ThreadError::invalid())
}

fn encode_exit_code(termination: ThreadTermination) -> u32 {
    match termination.kind {
        ThreadTerminationKind::Returned => termination
            .code
            .and_then(|code| u32::try_from(code.0).ok())
            // Win32 reserves 259 (`STILL_ACTIVE`) as a liveness sentinel for unfinished threads,
            // so this PAL never republishes it as a successful user return code.
            .filter(|code| *code != 259)
            .unwrap_or(0),
        ThreadTerminationKind::Canceled => 1,
        ThreadTerminationKind::Aborted => 2,
        ThreadTerminationKind::Signaled => 3,
        ThreadTerminationKind::Unknown => 4,
    }
}

fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn wait_unpoisoned<'a, T>(condvar: &Condvar, guard: MutexGuard<'a, T>) -> MutexGuard<'a, T> {
    condvar.wait(guard).unwrap_or_else(PoisonError::into_inner)
}

const fn map_win32_error(error: WIN32_ERROR) -> ThreadError {
    match error {
        ERROR_INVALID_PARAMETER => ThreadError::invalid(),
        ERROR_ACCESS_DENIED => ThreadError::permission_denied(),
        ERROR_BUSY => ThreadError::busy(),
        ERROR_TIMEOUT => ThreadError::timeout(),
        ERROR_NOT_ENOUGH_MEMORY | ERROR_OUTOFMEMORY => ThreadError::resource_exhausted(),
        ERROR_NOT_SUPPORTED => ThreadError::unsupported(),
        ERROR_INVALID_HANDLE => ThreadError::state_conflict(),
        _ => ThreadError::platform(error.0 as i32),
    }
}

const fn map_hresult(code: i32) -> ThreadError {
    let raw = code as u32;
    let facility = (raw >> 16) & 0x1fff;
    if facility == 7 {
        return map_win32_error(WIN32_ERROR(raw & 0xffff));
    }
    ThreadError::platform(code)
}

#[cfg(test)]
mod tests {
    use super::system_thread;
    use crate::contract::pal::runtime::thread::{
        ThreadBaseContract,
        ThreadConfig,
        ThreadConstraintMode,
        ThreadEntryReturn,
        ThreadJoinPolicy,
        ThreadLifecycle,
        ThreadObservationControlContract,
        ThreadPlacementControlContract,
        ThreadPlacementTarget,
        ThreadRunState,
        ThreadSchedulerCaps,
    };

    unsafe fn returns_context_code(context: *mut ()) -> ThreadEntryReturn {
        let code = unsafe { *context.cast::<usize>() };
        ThreadEntryReturn::new(code)
    }

    #[test]
    fn support_surface_matches_windows_backend_truth() {
        let support = system_thread().support();

        assert!(support.lifecycle.caps.contains(
            crate::contract::pal::runtime::thread::ThreadLifecycleCaps::SPAWN
                | crate::contract::pal::runtime::thread::ThreadLifecycleCaps::JOIN
                | crate::contract::pal::runtime::thread::ThreadLifecycleCaps::DETACH
                | crate::contract::pal::runtime::thread::ThreadLifecycleCaps::NAME
                | crate::contract::pal::runtime::thread::ThreadLifecycleCaps::CURRENT_THREAD_ID
                | crate::contract::pal::runtime::thread::ThreadLifecycleCaps::CURRENT_OBSERVE
                | crate::contract::pal::runtime::thread::ThreadLifecycleCaps::HANDLE_OBSERVE
                | crate::contract::pal::runtime::thread::ThreadLifecycleCaps::EXIT_CODE
        ));
        assert!(support.placement.caps.contains(
            crate::contract::pal::runtime::thread::ThreadPlacementCaps::LOGICAL_CPU_AFFINITY
                | crate::contract::pal::runtime::thread::ThreadPlacementCaps::PRESTART_APPLICATION
                | crate::contract::pal::runtime::thread::ThreadPlacementCaps::POSTSTART_APPLICATION
                | crate::contract::pal::runtime::thread::ThreadPlacementCaps::CURRENT_CPU_OBSERVE
                | crate::contract::pal::runtime::thread::ThreadPlacementCaps::EFFECTIVE_OBSERVE
        ));
        assert!(support.scheduler.caps.contains(
            ThreadSchedulerCaps::YIELD
                | ThreadSchedulerCaps::SLEEP_FOR
                | ThreadSchedulerCaps::MONOTONIC_NOW
                | ThreadSchedulerCaps::QUERY_PRIORITY
        ));
        assert!(support.stack.caps.is_empty());
    }

    #[test]
    fn current_observation_reports_running_thread() {
        let provider = system_thread();
        let observed = provider.observe_current().unwrap();

        assert_eq!(observed.id, provider.current_thread_id().unwrap());
        assert_eq!(observed.run_state, ThreadRunState::Running);
        assert!(observed.location.logical_cpu.is_some());
        assert!(observed.scheduler.base_priority.is_some());
    }

    #[test]
    fn spawn_join_round_trip_returns_exit_code() {
        let provider = system_thread();
        let mut code = 7usize;
        let config = ThreadConfig::new();

        let handle =
            unsafe { provider.spawn(&config, returns_context_code, (&raw mut code).cast()) }
                .unwrap();
        let termination = provider.join(handle).unwrap();

        assert_eq!(
            termination,
            crate::contract::pal::runtime::thread::ThreadTermination::returned(7)
        );
    }

    #[test]
    fn detached_spawn_rejects_join_and_detach() {
        let provider = system_thread();
        let mut code = 1usize;
        let mut config = ThreadConfig::new();
        config.join_policy = ThreadJoinPolicy::Detached;

        let handle =
            unsafe { provider.spawn(&config, returns_context_code, (&raw mut code).cast()) }
                .unwrap();

        assert_eq!(
            provider.join(handle).unwrap_err().kind(),
            crate::contract::pal::runtime::thread::ThreadError::state_conflict().kind()
        );
    }

    #[test]
    fn spawn_can_commit_single_cpu_affinity_before_user_entry() {
        let provider = system_thread();
        let current = provider.observe_current().unwrap();
        let cpu = current.location.logical_cpu.unwrap();
        let cpus = [cpu];
        let targets = [ThreadPlacementTarget::LogicalCpus(&cpus)];
        let mut code = 9usize;
        let mut config = ThreadConfig::new();
        config.placement.targets = &targets;
        config.placement.mode = ThreadConstraintMode::Require;

        let handle =
            unsafe { provider.spawn(&config, returns_context_code, (&raw mut code).cast()) }
                .unwrap();
        let placement = provider.placement(&handle).unwrap();

        assert_eq!(placement.location.logical_cpu, Some(cpu));
        let termination = provider.join(handle).unwrap();
        assert_eq!(
            termination.code,
            Some(crate::contract::pal::runtime::thread::ThreadExitCode(9))
        );
    }
}
