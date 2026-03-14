//! Linux fusion-pal thread backend.
//!
//! This backend uses `pthread` lifecycle primitives together with Linux scheduler and
//! affinity syscalls where they can be surfaced honestly. The result is intentionally
//! narrower than “all the thread things Linux can theoretically do”, because this layer is
//! supposed to tell the truth rather than audition for folklore.

use core::cell::UnsafeCell;
use core::ffi::{c_char, c_void};
use core::mem::MaybeUninit;
use core::ptr;
use core::sync::atomic::{AtomicU32, Ordering};
use core::time::Duration;

use libc::{
    self, PTHREAD_CREATE_DETACHED, PTHREAD_CREATE_JOINABLE, PTHREAD_STACK_MIN, pthread_attr_t,
    pthread_t, sched_param,
};
use rustix::io::Errno;
use rustix::process::Pid;
use rustix::thread::{self as rustix_thread, CpuSet, futex};

use crate::pal::thread::{
    RawThreadEntry, ThreadAuthoritySet, ThreadBase, ThreadConfig, ThreadConstraintMode,
    ThreadEntryReturn, ThreadError, ThreadExecutionLocation, ThreadGuarantee, ThreadId,
    ThreadIdentityStability, ThreadJoinPolicy, ThreadLifecycle, ThreadLifecycleCaps,
    ThreadLifecycleSupport, ThreadLocalitySupport, ThreadMigrationPolicy, ThreadObservation,
    ThreadObservationControl, ThreadPlacementCaps, ThreadPlacementControl, ThreadPlacementOutcome,
    ThreadPlacementPhase, ThreadPlacementRequest, ThreadPlacementSupport, ThreadPlacementTarget,
    ThreadPriority, ThreadPriorityOrder, ThreadPriorityRange, ThreadRunState, ThreadSchedulerCaps,
    ThreadSchedulerClass, ThreadSchedulerControl, ThreadSchedulerModel, ThreadSchedulerObservation,
    ThreadSchedulerRequest, ThreadSchedulerSupport, ThreadStackBacking, ThreadStackCaps,
    ThreadStackLocalityPolicy, ThreadStackLockPolicy, ThreadStackObservation,
    ThreadStackObservationControl, ThreadStackPrefaultPolicy, ThreadStackRequest,
    ThreadStackSupport, ThreadStartMode, ThreadSupport, ThreadSuspendControl, ThreadTermination,
    ThreadTerminationKind,
};

const STARTUP_PENDING: u32 = 0;
const STARTUP_READY: u32 = 1;
const STARTUP_FAILED: u32 = 2;

const LINUX_THREAD_SUPPORT: ThreadSupport = ThreadSupport {
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
        implementation: crate::pal::thread::ThreadImplementationKind::Native,
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
        implementation: crate::pal::thread::ThreadImplementationKind::Native,
    },
    scheduler: ThreadSchedulerSupport {
        caps: ThreadSchedulerCaps::YIELD
            .union(ThreadSchedulerCaps::SLEEP_FOR)
            .union(ThreadSchedulerCaps::PRIORITY)
            .union(ThreadSchedulerCaps::QUERY_PRIORITY)
            .union(ThreadSchedulerCaps::CLASS)
            .union(ThreadSchedulerCaps::QUERY_CLASS)
            .union(ThreadSchedulerCaps::REALTIME_FIXED)
            .union(ThreadSchedulerCaps::REALTIME_ROUND_ROBIN),
        model: ThreadSchedulerModel::Preemptive,
        priority: ThreadGuarantee::Enforced,
        realtime: ThreadGuarantee::Enforced,
        deadline: ThreadGuarantee::Unsupported,
        observation: ThreadGuarantee::Enforced,
        default_priority_range: Some(ThreadPriorityRange {
            min: ThreadPriority(0),
            max: ThreadPriority(0),
            ordering: ThreadPriorityOrder::HigherIsStronger,
        }),
        authorities: ThreadAuthoritySet::OPERATING_SYSTEM,
        implementation: crate::pal::thread::ThreadImplementationKind::Native,
    },
    stack: ThreadStackSupport {
        caps: ThreadStackCaps::EXPLICIT_SIZE
            .union(ThreadStackCaps::GUARD_SIZE)
            .union(ThreadStackCaps::CALLER_PROVIDED)
            .union(ThreadStackCaps::USAGE_OBSERVE),
        explicit_size: ThreadGuarantee::Enforced,
        caller_provided: ThreadGuarantee::Enforced,
        prefault: ThreadGuarantee::Unsupported,
        lock: ThreadGuarantee::Unsupported,
        locality: ThreadGuarantee::Unsupported,
        usage_observation: ThreadGuarantee::Unknown,
        authorities: ThreadAuthoritySet::OPERATING_SYSTEM,
        implementation: crate::pal::thread::ThreadImplementationKind::Native,
    },
    locality: ThreadLocalitySupport {
        caps: crate::pal::thread::ThreadLocalityCaps::empty(),
        first_touch: ThreadGuarantee::Unsupported,
        location_observation: ThreadGuarantee::Unsupported,
        memory_policy: ThreadGuarantee::Unsupported,
        authorities: ThreadAuthoritySet::empty(),
        implementation: crate::pal::thread::ThreadImplementationKind::Unsupported,
    },
};

/// Linux thread provider handle.
#[derive(Debug, Clone, Copy, Default)]
pub struct LinuxThread;

/// Linux owned thread handle.
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct LinuxThreadHandle {
    pthread: pthread_t,
    tid: ThreadId,
    joinable: bool,
}

/// Selected thread handle type for Linux builds.
pub type PlatformThreadHandle = LinuxThreadHandle;

/// Target-selected thread provider alias for Linux builds.
pub type PlatformThread = LinuxThread;

/// Returns the process-wide Linux thread provider handle.
#[must_use]
pub const fn system_thread() -> PlatformThread {
    PlatformThread::new()
}

impl LinuxThread {
    /// Creates a new Linux thread provider handle.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl ThreadBase for LinuxThread {
    type Handle = LinuxThreadHandle;

    fn support(&self) -> ThreadSupport {
        LINUX_THREAD_SUPPORT
    }
}

// SAFETY: this implementation owns thread creation and lifecycle through pthread-backed
// handles and does not expose interior invariants except through the fusion-pal contract itself.
unsafe impl ThreadLifecycle for LinuxThread {
    unsafe fn spawn(
        &self,
        config: &ThreadConfig<'_>,
        entry: RawThreadEntry,
        context: *mut (),
    ) -> Result<Self::Handle, ThreadError> {
        validate_spawn_config(config)?;

        let mut attr = PthreadAttr::new()?;
        configure_pthread_attr(&mut attr, config)?;

        let mut startup = LinuxThreadStartup::new(config, entry, context);
        let mut pthread = MaybeUninit::<pthread_t>::uninit();
        let create_result = unsafe {
            libc::pthread_create(
                pthread.as_mut_ptr(),
                attr.as_ptr(),
                linux_thread_entry,
                (&raw mut startup).cast::<c_void>(),
            )
        };
        if create_result != 0 {
            return Err(map_create_error(create_result));
        }

        let pthread = unsafe { pthread.assume_init() };
        wait_for_startup(&startup.ready)?;
        let startup_result = read_startup_result(&startup);
        if let Err(error) = startup_result {
            if config.join_policy == ThreadJoinPolicy::Joinable {
                let mut ignored = ptr::null_mut();
                let _ = unsafe { libc::pthread_join(pthread, &raw mut ignored) };
            }
            return Err(error);
        }

        Ok(LinuxThreadHandle {
            pthread,
            tid: startup_result?,
            joinable: config.join_policy == ThreadJoinPolicy::Joinable,
        })
    }

    fn current_thread_id(&self) -> Result<ThreadId, ThreadError> {
        current_thread_id_linux()
    }

    fn join(&self, handle: Self::Handle) -> Result<ThreadTermination, ThreadError> {
        if !handle.joinable {
            return Err(ThreadError::state_conflict());
        }

        let mut result = ptr::null_mut();
        let rc = unsafe { libc::pthread_join(handle.pthread, &raw mut result) };
        if rc != 0 {
            return Err(map_join_error(rc));
        }

        if is_pthread_canceled(result) {
            return Ok(ThreadTermination {
                kind: ThreadTerminationKind::Canceled,
                code: None,
            });
        }

        Ok(ThreadTermination::from_entry_return(ThreadEntryReturn {
            code: crate::pal::thread::ThreadExitCode(result as usize),
        }))
    }

    fn detach(&self, handle: Self::Handle) -> Result<(), ThreadError> {
        if !handle.joinable {
            return Err(ThreadError::state_conflict());
        }

        let rc = unsafe { libc::pthread_detach(handle.pthread) };
        if rc != 0 {
            return Err(map_platform_error(rc));
        }

        Ok(())
    }
}

impl ThreadSuspendControl for LinuxThread {
    fn suspend(&self, _handle: &Self::Handle) -> Result<(), ThreadError> {
        Err(ThreadError::unsupported())
    }

    fn resume(&self, _handle: &Self::Handle) -> Result<(), ThreadError> {
        Err(ThreadError::unsupported())
    }
}

impl ThreadSchedulerControl for LinuxThread {
    fn priority_range(
        &self,
        class: ThreadSchedulerClass,
    ) -> Result<Option<ThreadPriorityRange>, ThreadError> {
        priority_range_for_class(class)
    }

    fn set_scheduler(
        &self,
        handle: &Self::Handle,
        request: &ThreadSchedulerRequest,
    ) -> Result<ThreadSchedulerObservation, ThreadError> {
        apply_scheduler_request(handle.pthread, request)
    }

    fn scheduler(&self, handle: &Self::Handle) -> Result<ThreadSchedulerObservation, ThreadError> {
        scheduler_observation_for_pthread(handle.pthread)
    }

    fn yield_now(&self) -> Result<(), ThreadError> {
        rustix_thread::sched_yield();
        Ok(())
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
}

impl ThreadPlacementControl for LinuxThread {
    fn set_placement(
        &self,
        handle: &Self::Handle,
        request: &ThreadPlacementRequest<'_>,
    ) -> Result<ThreadPlacementOutcome, ThreadError> {
        apply_affinity_request(Some(handle.tid), request)
    }

    fn placement(&self, handle: &Self::Handle) -> Result<ThreadPlacementOutcome, ThreadError> {
        placement_outcome_for_thread(handle.tid)
    }
}

impl ThreadObservationControl for LinuxThread {
    fn observe_current(&self) -> Result<ThreadObservation, ThreadError> {
        let location = current_execution_location();
        Ok(ThreadObservation {
            id: current_thread_id_linux()?,
            run_state: ThreadRunState::Running,
            location,
            scheduler: scheduler_observation_for_pthread(unsafe { libc::pthread_self() })?,
            placement: ThreadPlacementOutcome {
                guarantee: LINUX_THREAD_SUPPORT.placement.observation,
                phase: ThreadPlacementPhase::Inherit,
                location,
            },
        })
    }

    fn observe(&self, handle: &Self::Handle) -> Result<ThreadObservation, ThreadError> {
        let placement = placement_outcome_for_thread(handle.tid)?;
        Ok(ThreadObservation {
            id: handle.tid,
            run_state: ThreadRunState::Unknown,
            location: placement.location,
            scheduler: scheduler_observation_for_pthread(handle.pthread)?,
            placement,
        })
    }
}

impl ThreadStackObservationControl for LinuxThread {
    fn observe_current_stack(&self) -> Result<ThreadStackObservation, ThreadError> {
        stack_observation_for_pthread(unsafe { libc::pthread_self() })
    }

    fn observe_stack(&self, handle: &Self::Handle) -> Result<ThreadStackObservation, ThreadError> {
        stack_observation_for_pthread(handle.pthread)
    }
}

struct LinuxThreadStartup<'a> {
    ready: AtomicU32,
    result: UnsafeCell<Result<ThreadId, ThreadError>>,
    config: &'a ThreadConfig<'a>,
    entry: RawThreadEntry,
    context: *mut (),
}

impl<'a> LinuxThreadStartup<'a> {
    const fn new(config: &'a ThreadConfig<'a>, entry: RawThreadEntry, context: *mut ()) -> Self {
        Self {
            ready: AtomicU32::new(STARTUP_PENDING),
            result: UnsafeCell::new(Err(ThreadError::busy())),
            config,
            entry,
            context,
        }
    }
}

// SAFETY: the startup record is shared only between the creator and the newly spawned
// thread, with publication synchronized through `ready`.
unsafe impl Sync for LinuxThreadStartup<'_> {}

struct PthreadAttr {
    raw: MaybeUninit<pthread_attr_t>,
    initialized: bool,
}

impl PthreadAttr {
    fn new() -> Result<Self, ThreadError> {
        let mut raw = MaybeUninit::<pthread_attr_t>::uninit();
        let rc = unsafe { libc::pthread_attr_init(raw.as_mut_ptr()) };
        if rc != 0 {
            return Err(map_platform_error(rc));
        }
        Ok(Self {
            raw,
            initialized: true,
        })
    }

    const fn uninit() -> Self {
        Self {
            raw: MaybeUninit::uninit(),
            initialized: false,
        }
    }

    const fn mark_initialized(&mut self) {
        self.initialized = true;
    }

    const fn as_ptr(&self) -> *const pthread_attr_t {
        self.raw.as_ptr()
    }

    const fn as_mut_ptr(&mut self) -> *mut pthread_attr_t {
        self.raw.as_mut_ptr()
    }
}

impl Drop for PthreadAttr {
    fn drop(&mut self) {
        if self.initialized {
            let _ = unsafe { libc::pthread_attr_destroy(self.as_mut_ptr()) };
        }
    }
}

extern "C" fn linux_thread_entry(arg: *mut c_void) -> *mut c_void {
    let (entry, context) = {
        let startup = unsafe { &*(arg.cast::<LinuxThreadStartup<'_>>()) };
        let config = startup.config;
        let entry = startup.entry;
        let context = startup.context;

        let startup_result = configure_spawned_thread(config);
        let startup_ok = startup_result.is_ok();
        write_startup_result(startup, startup_result);

        if !startup_ok {
            return ptr::null_mut();
        }

        (entry, context)
    };
    let result = unsafe { entry(context) };
    result.code.0 as *mut c_void
}

fn configure_spawned_thread(config: &ThreadConfig<'_>) -> Result<ThreadId, ThreadError> {
    let tid = current_thread_id_linux()?;

    if let Some(name) = config.name {
        set_current_thread_name(name)?;
    }

    if !scheduler_request_is_default(&config.scheduler) {
        let current = unsafe { libc::pthread_self() };
        let _ = apply_scheduler_request(current, &config.scheduler)?;
    }

    if placement_request_has_constraints(&config.placement) {
        let _ = apply_affinity_request(None, &config.placement)?;
    }

    Ok(tid)
}

fn validate_spawn_config(config: &ThreadConfig<'_>) -> Result<(), ThreadError> {
    validate_thread_name(config.name)?;
    validate_stack_request(&config.stack)?;
    validate_placement_request(&config.placement)?;
    validate_scheduler_request(&config.scheduler)?;

    if matches!(
        config.start_mode,
        ThreadStartMode::PlacementCommitted | ThreadStartMode::PlacementAndStackCommitted
    ) && !placement_request_is_default(&config.placement)
        && config.placement.phase == ThreadPlacementPhase::PostStartAllowed
    {
        return Err(ThreadError::placement_denied());
    }

    Ok(())
}

fn validate_thread_name(name: Option<&str>) -> Result<(), ThreadError> {
    if let Some(name) = name
        && (name.as_bytes().contains(&0) || name.len() > 15)
    {
        return Err(ThreadError::invalid());
    }
    Ok(())
}

fn validate_stack_request(request: &ThreadStackRequest) -> Result<(), ThreadError> {
    if matches!(
        request.prefault,
        ThreadStackPrefaultPolicy::Creator | ThreadStackPrefaultPolicy::Target
    ) {
        return Err(ThreadError::stack_denied());
    }

    if matches!(request.lock, ThreadStackLockPolicy::Required) {
        return Err(ThreadError::stack_denied());
    }

    if matches!(
        request.locality,
        ThreadStackLocalityPolicy::RequiredNumaNode(_)
    ) {
        return Err(ThreadError::stack_denied());
    }

    if let ThreadStackBacking::CallerProvided { len, .. } = request.backing {
        if let Some(size_bytes) = request.size_bytes
            && size_bytes != len
        {
            return Err(ThreadError::invalid());
        }

        if matches!(request.guard_bytes, Some(guard) if guard != 0) {
            return Err(ThreadError::stack_denied());
        }
    }

    Ok(())
}

fn validate_placement_request(request: &ThreadPlacementRequest<'_>) -> Result<(), ThreadError> {
    if request.has_non_logical_targets() && request.mode == ThreadConstraintMode::Require {
        return Err(ThreadError::placement_denied());
    }

    if matches!(request.migration, ThreadMigrationPolicy::Disallow)
        && request.logical_cpu_count() != 1
    {
        return Err(ThreadError::placement_denied());
    }

    for target in request.targets {
        if let ThreadPlacementTarget::LogicalCpus(cpus) = target {
            for cpu in *cpus {
                if cpu.group.0 != 0 {
                    return Err(ThreadError::placement_denied());
                }
            }
        }
    }

    Ok(())
}

fn validate_scheduler_request(request: &ThreadSchedulerRequest) -> Result<(), ThreadError> {
    let range = priority_range_for_class(request.class)?;
    if let Some(priority) = request.priority {
        let Some(range) = range else {
            return Err(ThreadError::scheduler_denied());
        };
        if !range.contains(priority) {
            return Err(ThreadError::scheduler_denied());
        }
    }

    if request.deadline.is_some() {
        return Err(ThreadError::scheduler_denied());
    }

    Ok(())
}

fn configure_pthread_attr(
    attr: &mut PthreadAttr,
    config: &ThreadConfig<'_>,
) -> Result<(), ThreadError> {
    let detach_state = match config.join_policy {
        ThreadJoinPolicy::Joinable => PTHREAD_CREATE_JOINABLE,
        ThreadJoinPolicy::Detached => PTHREAD_CREATE_DETACHED,
    };
    let rc = unsafe { libc::pthread_attr_setdetachstate(attr.as_mut_ptr(), detach_state) };
    if rc != 0 {
        return Err(map_platform_error(rc));
    }

    if let Some(guard_bytes) = config.stack.guard_bytes {
        let rc = unsafe { libc::pthread_attr_setguardsize(attr.as_mut_ptr(), guard_bytes) };
        if rc != 0 {
            return Err(ThreadError::stack_denied());
        }
    }

    match config.stack.backing {
        ThreadStackBacking::Default => {
            if let Some(size_bytes) = config.stack.size_bytes {
                let size = size_bytes.get();
                if size < PTHREAD_STACK_MIN {
                    return Err(ThreadError::stack_denied());
                }
                let rc = unsafe { libc::pthread_attr_setstacksize(attr.as_mut_ptr(), size) };
                if rc != 0 {
                    return Err(ThreadError::stack_denied());
                }
            }
        }
        ThreadStackBacking::CallerProvided { base, len } => {
            let rc = unsafe {
                libc::pthread_attr_setstack(
                    attr.as_mut_ptr(),
                    base.as_ptr().cast::<c_void>(),
                    len.get(),
                )
            };
            if rc != 0 {
                return Err(ThreadError::stack_denied());
            }
        }
    }

    Ok(())
}

fn scheduler_request_is_default(request: &ThreadSchedulerRequest) -> bool {
    request.class == ThreadSchedulerClass::Inherit
        && request.priority.is_none()
        && request.deadline.is_none()
}

fn placement_request_is_default(request: &ThreadPlacementRequest<'_>) -> bool {
    !request.has_targets() && request.migration == ThreadMigrationPolicy::Inherit
}

fn placement_request_has_constraints(request: &ThreadPlacementRequest<'_>) -> bool {
    !placement_request_is_default(request)
}

fn apply_affinity_request(
    target: Option<ThreadId>,
    request: &ThreadPlacementRequest<'_>,
) -> Result<ThreadPlacementOutcome, ThreadError> {
    if request.logical_cpu_count() == 0 {
        if matches!(request.migration, ThreadMigrationPolicy::Disallow) {
            return Err(ThreadError::placement_denied());
        }
        return Ok(ThreadPlacementOutcome::unsupported());
    }

    let cpuset = cpu_set_from_request(request)?;
    let pid = match target {
        Some(id) => Some(thread_id_to_pid(id)?),
        None => None,
    };
    rustix_thread::sched_setaffinity(pid, &cpuset).map_err(map_errno)?;

    Ok(ThreadPlacementOutcome {
        guarantee: LINUX_THREAD_SUPPORT.placement.logical_cpu_affinity,
        phase: if target.is_none() {
            match request.phase {
                ThreadPlacementPhase::PreStartRequired => ThreadPlacementPhase::PreStartRequired,
                _ => ThreadPlacementPhase::PreStartPreferred,
            }
        } else {
            ThreadPlacementPhase::PostStartAllowed
        },
        location: location_from_affinity(&cpuset),
    })
}

fn cpu_set_from_request(request: &ThreadPlacementRequest<'_>) -> Result<CpuSet, ThreadError> {
    let mut cpuset = CpuSet::new();
    for target in request.targets {
        if let ThreadPlacementTarget::LogicalCpus(cpus) = target {
            for cpu in *cpus {
                if cpu.group.0 != 0 {
                    return Err(ThreadError::placement_denied());
                }
                cpuset.set(usize::from(cpu.index));
            }
        }
    }
    Ok(cpuset)
}

fn location_from_affinity(cpuset: &CpuSet) -> ThreadExecutionLocation {
    let mut location = ThreadExecutionLocation::unknown();
    if cpuset.count() == 1 {
        for cpu in 0..CpuSet::MAX_CPU {
            if cpuset.is_set(cpu) {
                if let Ok(index) = u16::try_from(cpu) {
                    location.logical_cpu = Some(crate::pal::thread::ThreadLogicalCpuId {
                        group: crate::pal::thread::ThreadProcessorGroupId(0),
                        index,
                    });
                }
                break;
            }
        }
    }
    location
}

fn placement_outcome_for_thread(id: ThreadId) -> Result<ThreadPlacementOutcome, ThreadError> {
    let cpuset =
        rustix_thread::sched_getaffinity(Some(thread_id_to_pid(id)?)).map_err(map_errno)?;
    Ok(ThreadPlacementOutcome {
        guarantee: LINUX_THREAD_SUPPORT.placement.observation,
        phase: ThreadPlacementPhase::Inherit,
        location: location_from_affinity(&cpuset),
    })
}

fn current_execution_location() -> ThreadExecutionLocation {
    let mut location = ThreadExecutionLocation::unknown();
    let cpu = rustix_thread::sched_getcpu();
    if let Ok(index) = u16::try_from(cpu) {
        location.logical_cpu = Some(crate::pal::thread::ThreadLogicalCpuId {
            group: crate::pal::thread::ThreadProcessorGroupId(0),
            index,
        });
    }
    location
}

fn priority_range_for_class(
    class: ThreadSchedulerClass,
) -> Result<Option<ThreadPriorityRange>, ThreadError> {
    let Some(policy) = policy_for_priority_range(class) else {
        return Ok(None);
    };

    let min = unsafe { libc::sched_get_priority_min(policy) };
    let max = unsafe { libc::sched_get_priority_max(policy) };
    if min < 0 || max < 0 {
        return Err(map_errno(last_errno()));
    }

    Ok(Some(ThreadPriorityRange {
        min: ThreadPriority(min),
        max: ThreadPriority(max),
        ordering: ThreadPriorityOrder::HigherIsStronger,
    }))
}

const fn policy_for_priority_range(class: ThreadSchedulerClass) -> Option<libc::c_int> {
    match class {
        ThreadSchedulerClass::Default => Some(libc::SCHED_OTHER),
        ThreadSchedulerClass::Background => Some(libc::SCHED_BATCH),
        ThreadSchedulerClass::FixedPriorityRealtime => Some(libc::SCHED_FIFO),
        ThreadSchedulerClass::RoundRobinRealtime => Some(libc::SCHED_RR),
        ThreadSchedulerClass::Inherit
        | ThreadSchedulerClass::Deadline
        | ThreadSchedulerClass::VendorSpecific(_) => None,
    }
}

fn apply_scheduler_request(
    pthread: pthread_t,
    request: &ThreadSchedulerRequest,
) -> Result<ThreadSchedulerObservation, ThreadError> {
    if request.class == ThreadSchedulerClass::Inherit && request.priority.is_none() {
        return scheduler_observation_for_pthread(pthread);
    }

    let Some(policy) = policy_for_priority_range(request.class) else {
        return Err(ThreadError::scheduler_denied());
    };

    let mut param = sched_param { sched_priority: 0 };
    if let Some(priority) = request.priority {
        param.sched_priority = priority.0;
    }

    let rc = unsafe { libc::pthread_setschedparam(pthread, policy, &raw const param) };
    if rc != 0 {
        return Err(map_scheduler_error(rc));
    }

    scheduler_observation_for_pthread(pthread)
}

fn scheduler_observation_for_pthread(
    pthread: pthread_t,
) -> Result<ThreadSchedulerObservation, ThreadError> {
    let mut policy = 0;
    let mut param = sched_param { sched_priority: 0 };
    let rc = unsafe { libc::pthread_getschedparam(pthread, &raw mut policy, &raw mut param) };
    if rc != 0 {
        return Err(map_platform_error(rc));
    }

    Ok(ThreadSchedulerObservation {
        class: class_from_policy(policy),
        base_priority: Some(ThreadPriority(param.sched_priority)),
        // POSIX exposes the configured scheduler priority here, not any transient priority
        // inheritance boost a backend mutex might induce at runtime.
        effective_priority: None,
    })
}

const fn class_from_policy(policy: libc::c_int) -> Option<ThreadSchedulerClass> {
    match policy {
        libc::SCHED_OTHER => Some(ThreadSchedulerClass::Default),
        libc::SCHED_BATCH => Some(ThreadSchedulerClass::Background),
        libc::SCHED_FIFO => Some(ThreadSchedulerClass::FixedPriorityRealtime),
        libc::SCHED_RR => Some(ThreadSchedulerClass::RoundRobinRealtime),
        _ => None,
    }
}

fn stack_observation_for_pthread(
    pthread: pthread_t,
) -> Result<ThreadStackObservation, ThreadError> {
    let mut attr = PthreadAttr::uninit();
    let rc = unsafe { libc::pthread_getattr_np(pthread, attr.as_mut_ptr()) };
    if rc != 0 {
        return Err(map_platform_error(rc));
    }
    attr.mark_initialized();

    let mut stack_addr = ptr::null_mut();
    let mut stack_size = 0_usize;
    let rc = unsafe {
        libc::pthread_attr_getstack(attr.as_ptr(), &raw mut stack_addr, &raw mut stack_size)
    };
    if rc != 0 {
        return Err(map_platform_error(rc));
    }

    let mut guard_size = 0_usize;
    let rc = unsafe { libc::pthread_attr_getguardsize(attr.as_ptr(), &raw mut guard_size) };
    if rc != 0 {
        return Err(map_platform_error(rc));
    }

    Ok(ThreadStackObservation {
        configured_bytes: core::num::NonZeroUsize::new(stack_size),
        guard_bytes: Some(guard_size),
        high_water_bytes: None,
        current_bytes: None,
        overflow_detected: None,
    })
}

fn wait_for_startup(word: &AtomicU32) -> Result<(), ThreadError> {
    loop {
        match word.load(Ordering::Acquire) {
            STARTUP_READY | STARTUP_FAILED => return Ok(()),
            STARTUP_PENDING => {
                match futex::wait(word, futex::Flags::PRIVATE, STARTUP_PENDING, None) {
                    Ok(()) | Err(Errno::INTR | Errno::AGAIN) => {}
                    Err(errno) => return Err(map_errno(errno)),
                }
            }
            _ => return Err(ThreadError::invalid()),
        }
    }
}

fn write_startup_result(startup: &LinuxThreadStartup<'_>, result: Result<ThreadId, ThreadError>) {
    let success = result.is_ok();
    unsafe {
        *startup.result.get() = result;
    }
    let state = if success {
        STARTUP_READY
    } else {
        STARTUP_FAILED
    };
    startup.ready.store(state, Ordering::Release);
    let _ = futex::wake(&startup.ready, futex::Flags::PRIVATE, u32::MAX);
}

fn read_startup_result(startup: &LinuxThreadStartup<'_>) -> Result<ThreadId, ThreadError> {
    unsafe { *startup.result.get() }
}

fn current_thread_id_linux() -> Result<ThreadId, ThreadError> {
    let tid = rustix_thread::gettid().as_raw_pid();
    let tid = u64::try_from(tid).map_err(|_| ThreadError::invalid())?;
    Ok(ThreadId(tid))
}

fn thread_id_to_pid(id: ThreadId) -> Result<Pid, ThreadError> {
    let raw = i32::try_from(id.0).map_err(|_| ThreadError::invalid())?;
    Pid::from_raw(raw).ok_or_else(ThreadError::invalid)
}

fn set_current_thread_name(name: &str) -> Result<(), ThreadError> {
    let mut buffer = [0 as c_char; 16];
    for (index, byte) in name.bytes().enumerate() {
        buffer[index] = byte.cast_signed() as c_char;
    }
    let rc = unsafe { libc::pthread_setname_np(libc::pthread_self(), buffer.as_ptr()) };
    if rc != 0 {
        return Err(map_platform_error(rc));
    }
    Ok(())
}

fn duration_to_timespec(duration: Duration) -> Result<libc::timespec, ThreadError> {
    let secs = libc::time_t::try_from(duration.as_secs()).map_err(|_| ThreadError::invalid())?;
    let nanos = libc::c_long::from(duration.subsec_nanos());
    Ok(libc::timespec {
        tv_sec: secs,
        tv_nsec: nanos,
    })
}

const fn map_create_error(code: libc::c_int) -> ThreadError {
    match code {
        libc::EAGAIN => ThreadError::resource_exhausted(),
        libc::EPERM => ThreadError::permission_denied(),
        libc::EINVAL => ThreadError::invalid(),
        other => ThreadError::platform(other),
    }
}

const fn map_join_error(code: libc::c_int) -> ThreadError {
    match code {
        libc::ESRCH | libc::EINVAL | libc::EDEADLK => ThreadError::state_conflict(),
        other => ThreadError::platform(other),
    }
}

const fn map_scheduler_error(code: libc::c_int) -> ThreadError {
    match code {
        libc::EINVAL => ThreadError::scheduler_denied(),
        libc::EPERM | libc::EACCES => ThreadError::permission_denied(),
        other => ThreadError::platform(other),
    }
}

const fn map_platform_error(code: libc::c_int) -> ThreadError {
    match code {
        libc::EINVAL => ThreadError::invalid(),
        libc::EBUSY => ThreadError::busy(),
        libc::EPERM | libc::EACCES => ThreadError::permission_denied(),
        libc::ENOMEM | libc::EAGAIN => ThreadError::resource_exhausted(),
        libc::ETIMEDOUT => ThreadError::timeout(),
        libc::ESRCH => ThreadError::state_conflict(),
        other => ThreadError::platform(other),
    }
}

const fn map_errno(errno: Errno) -> ThreadError {
    match errno {
        Errno::INVAL => ThreadError::invalid(),
        Errno::BUSY => ThreadError::busy(),
        Errno::PERM | Errno::ACCESS => ThreadError::permission_denied(),
        Errno::NOMEM | Errno::AGAIN => ThreadError::resource_exhausted(),
        Errno::TIMEDOUT => ThreadError::timeout(),
        Errno::SRCH => ThreadError::state_conflict(),
        other => ThreadError::platform(other.raw_os_error()),
    }
}

fn is_pthread_canceled(result: *mut c_void) -> bool {
    result as isize == -1
}

fn last_errno() -> Errno {
    Errno::from_raw_os_error(unsafe { *libc::__errno_location() })
}

#[cfg(test)]
mod tests {
    extern crate std;

    use core::sync::atomic::{AtomicU32, Ordering};

    use super::*;

    #[repr(C)]
    struct ExitContext<'a> {
        touched: &'a AtomicU32,
    }

    unsafe fn exit_entry(context: *mut ()) -> ThreadEntryReturn {
        let context = unsafe { &*(context.cast::<ExitContext<'_>>()) };
        context.touched.store(1, Ordering::Release);
        ThreadEntryReturn::new(7)
    }

    #[test]
    fn linux_thread_spawns_and_joins() {
        let provider = system_thread();
        let touched = AtomicU32::new(0);
        let context = ExitContext { touched: &touched };
        let handle = unsafe {
            provider
                .spawn(
                    &ThreadConfig::new(),
                    exit_entry,
                    (&raw const context).cast_mut().cast(),
                )
                .expect("thread should spawn")
        };

        let termination = provider.join(handle).expect("thread should join");

        assert_eq!(termination.kind, ThreadTerminationKind::Returned);
        assert_eq!(
            termination.code,
            Some(crate::pal::thread::ThreadExitCode(7))
        );
        assert_eq!(touched.load(Ordering::Acquire), 1);
    }

    #[repr(C)]
    struct AffinityContext {
        observed_cpu: AtomicU32,
    }

    unsafe fn affinity_entry(context: *mut ()) -> ThreadEntryReturn {
        let provider = system_thread();
        let observation = provider
            .observe_current()
            .expect("current thread should observe");
        let context = unsafe { &*(context.cast::<AffinityContext>()) };
        if let Some(cpu) = observation.location.logical_cpu {
            context
                .observed_cpu
                .store(u32::from(cpu.index), Ordering::Release);
        }
        ThreadEntryReturn::new(0)
    }

    #[test]
    fn linux_thread_applies_prestart_affinity() {
        let cpuset = rustix_thread::sched_getaffinity(None).expect("affinity query should work");
        let mut selected = None;
        for cpu in 0..CpuSet::MAX_CPU {
            if cpuset.is_set(cpu) {
                if let Ok(index) = u16::try_from(cpu) {
                    selected = Some(index);
                }
                break;
            }
        }

        let Some(selected_cpu) = selected else {
            return;
        };

        let provider = system_thread();
        let context = AffinityContext {
            observed_cpu: AtomicU32::new(u32::MAX),
        };
        let cpus = [crate::pal::thread::ThreadLogicalCpuId {
            group: crate::pal::thread::ThreadProcessorGroupId(0),
            index: selected_cpu,
        }];
        let config = ThreadConfig {
            placement: ThreadPlacementRequest {
                targets: &[ThreadPlacementTarget::LogicalCpus(&cpus)],
                mode: ThreadConstraintMode::Require,
                phase: ThreadPlacementPhase::PreStartRequired,
                ..ThreadPlacementRequest::new()
            },
            start_mode: ThreadStartMode::PlacementCommitted,
            ..ThreadConfig::new()
        };

        let handle = unsafe {
            provider
                .spawn(
                    &config,
                    affinity_entry,
                    (&raw const context).cast_mut().cast(),
                )
                .expect("thread should spawn")
        };

        provider.join(handle).expect("thread should join");
        assert_eq!(
            context.observed_cpu.load(Ordering::Acquire),
            u32::from(selected_cpu)
        );
    }

    #[test]
    fn linux_thread_reports_stack_observation() {
        let provider = system_thread();
        let observation = provider
            .observe_current_stack()
            .expect("current stack observation should succeed");

        assert!(observation.configured_bytes.is_some());
        assert!(observation.guard_bytes.is_some());
    }

    #[test]
    fn linux_thread_priority_ranges_are_class_specific() {
        let provider = system_thread();

        assert_eq!(
            provider
                .priority_range(ThreadSchedulerClass::Deadline)
                .expect("query should work"),
            None
        );
        assert!(
            provider
                .priority_range(ThreadSchedulerClass::FixedPriorityRealtime)
                .expect("query should work")
                .is_some()
        );
    }

    #[test]
    fn linux_thread_support_reports_native_surface() {
        let support = system_thread().support();

        assert_eq!(support.scheduler.model, ThreadSchedulerModel::Preemptive);
        assert_eq!(
            support.lifecycle.identity_stability,
            ThreadIdentityStability::ThreadLifetime
        );
    }
}
