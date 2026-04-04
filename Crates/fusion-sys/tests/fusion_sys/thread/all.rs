extern crate std;

use core::sync::atomic::{
    AtomicU32,
    Ordering,
};
use core::time::Duration;

use fusion_sys::event::{
    EventInterest,
    EventRegistrationMode,
};
use fusion_sys::thread::{
    MonotonicDeadlineWaitKind,
    MonotonicRuntimeTimeCaps,
    RuntimeBackingPreference,
    SystemThreadPool,
    SystemThreadPoolConfig,
    SystemWorkItem,
    ThreadConfig,
    ThreadEntryReturn,
    ThreadErrorKind,
    ThreadGuarantee,
    ThreadLifecycleCaps,
    ThreadSchedulerCaps,
    ThreadStackCaps,
    ThreadSystem,
    system_monotonic_time,
    system_thread,
};
use std::sync::Arc;
use std::thread;

#[repr(C)]
struct ExitContext<'a> {
    touched: &'a AtomicU32,
}

unsafe fn exit_entry(context: *mut ()) -> ThreadEntryReturn {
    let context = unsafe { &*(context.cast::<ExitContext<'_>>()) };
    context.touched.store(1, Ordering::Release);
    ThreadEntryReturn::new(9)
}

static DETACHED_TOUCH: AtomicU32 = AtomicU32::new(0);

unsafe fn detached_entry(_context: *mut ()) -> ThreadEntryReturn {
    DETACHED_TOUCH.store(1, Ordering::Release);
    ThreadEntryReturn::new(0)
}

unsafe fn pool_entry(context: *mut ()) {
    let touched = unsafe { &*(context.cast::<AtomicU32>()) };
    touched.fetch_add(1, Ordering::AcqRel);
}

struct BlockingPoolContext {
    started: Arc<AtomicU32>,
    release: Arc<AtomicU32>,
}

struct CancelPoolContext {
    executed: Arc<AtomicU32>,
    canceled: Arc<AtomicU32>,
}

unsafe fn blocking_pool_entry(context: *mut ()) {
    let context = unsafe { &*(context.cast::<BlockingPoolContext>()) };
    context.started.fetch_add(1, Ordering::AcqRel);
    while context.release.load(Ordering::Acquire) == 0 {
        core::hint::spin_loop();
    }
}

unsafe fn cancelable_pool_entry(context: *mut ()) {
    let context = unsafe { &*(context.cast::<CancelPoolContext>()) };
    context.executed.fetch_add(1, Ordering::AcqRel);
}

unsafe fn cancelable_pool_drop(context: *mut ()) {
    let context = unsafe { &*(context.cast::<CancelPoolContext>()) };
    context.canceled.fetch_add(1, Ordering::AcqRel);
}

#[test]
fn support_surface_is_exposed() {
    let thread = system_thread();
    let support = thread.support();

    if support.lifecycle.caps.contains(ThreadLifecycleCaps::SPAWN) {
        assert!(
            support
                .lifecycle
                .caps
                .contains(ThreadLifecycleCaps::CURRENT_THREAD_ID)
        );
    }
}

#[test]
fn runtime_construction_support_matches_platform_truth() {
    let support = ThreadSystem::new().runtime_construction_support();

    #[cfg(target_os = "linux")]
    {
        assert!(support.can_acquire_runtime_backing);
        assert_eq!(
            support.preferred_backing,
            RuntimeBackingPreference::PlatformAcquired
        );
    }

    #[cfg(all(target_arch = "arm", target_os = "none"))]
    {
        assert!(!support.can_acquire_runtime_backing);
        assert_eq!(
            support.preferred_backing,
            RuntimeBackingPreference::ExplicitBound
        );
    }
}

#[test]
fn spawn_and_join_follow_backend_truth() {
    let thread = system_thread();
    let support = thread.support();
    let touched = AtomicU32::new(0);
    let context = ExitContext { touched: &touched };

    let result = unsafe {
        thread.spawn_raw(
            &ThreadConfig::new(),
            exit_entry,
            (&raw const context).cast_mut().cast(),
        )
    };

    if support.lifecycle.caps.contains(ThreadLifecycleCaps::SPAWN) {
        let handle = result.expect("thread should spawn on supported backend");
        let termination = thread.join(handle).expect("thread should join");
        assert_eq!(termination.code.map(|code| code.0), Some(9));
        assert_eq!(touched.load(Ordering::Acquire), 1);
    } else {
        let error = result.expect_err("unsupported backend should reject spawn");
        assert_eq!(error.kind(), ThreadErrorKind::Unsupported);
    }
}

#[test]
fn current_thread_queries_follow_backend_truth() {
    let thread = system_thread();
    let support = thread.support();

    if support
        .lifecycle
        .caps
        .contains(ThreadLifecycleCaps::CURRENT_THREAD_ID)
    {
        assert!(thread.current_thread_id().is_ok());
        assert!(thread.observe_current().is_ok());
    } else {
        assert_eq!(
            thread
                .current_thread_id()
                .expect_err("unsupported current id")
                .kind(),
            ThreadErrorKind::Unsupported
        );
        assert_eq!(
            thread
                .observe_current()
                .expect_err("unsupported current observe")
                .kind(),
            ThreadErrorKind::Unsupported
        );
    }
}

#[test]
fn detached_threads_follow_backend_truth() {
    let thread = system_thread();
    let support = thread.support();
    DETACHED_TOUCH.store(0, Ordering::Release);

    let config = ThreadConfig {
        join_policy: fusion_sys::thread::ThreadJoinPolicy::Detached,
        ..ThreadConfig::new()
    };
    let result = unsafe { thread.spawn_raw(&config, detached_entry, core::ptr::null_mut()) };

    if support.lifecycle.caps.contains(ThreadLifecycleCaps::SPAWN) {
        let handle = result.expect("detached thread should spawn");
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert_eq!(DETACHED_TOUCH.load(Ordering::Acquire), 1);
        assert_eq!(
            thread
                .join(handle)
                .expect_err("detached thread should not join")
                .kind(),
            ThreadErrorKind::StateConflict
        );
    } else {
        assert_eq!(
            result
                .expect_err("unsupported backend should reject detached spawn")
                .kind(),
            ThreadErrorKind::Unsupported
        );
    }
}

#[test]
fn current_stack_observation_follows_backend_truth() {
    let thread = system_thread();
    let support = thread.support();

    if support.stack.caps.contains(ThreadStackCaps::USAGE_OBSERVE) {
        let observation = thread
            .observe_current_stack()
            .expect("stack observation should succeed");
        assert!(observation.configured_bytes.is_some());
    } else {
        assert_eq!(
            thread
                .observe_current_stack()
                .expect_err("unsupported stack observation")
                .kind(),
            ThreadErrorKind::Unsupported
        );
    }
}

#[test]
fn sleep_for_is_honest() {
    let thread = system_thread();
    let support = thread.support();

    if support
        .scheduler
        .caps
        .contains(fusion_sys::thread::ThreadSchedulerCaps::SLEEP_FOR)
    {
        assert!(thread.sleep_for(Duration::from_millis(1)).is_ok());
    } else {
        assert_eq!(
            thread
                .sleep_for(Duration::from_millis(1))
                .expect_err("unsupported sleep")
                .kind(),
            ThreadErrorKind::Unsupported
        );
    }
}

#[test]
fn monotonic_now_is_honest() {
    let thread = system_thread();
    let support = thread.support();

    if support
        .scheduler
        .caps
        .contains(fusion_sys::thread::ThreadSchedulerCaps::MONOTONIC_NOW)
    {
        let first = thread
            .monotonic_now()
            .expect("monotonic time should be readable");
        let second = thread
            .monotonic_now()
            .expect("monotonic time should remain readable");
        assert!(second >= first);
    } else {
        assert_eq!(
            thread
                .monotonic_now()
                .expect_err("unsupported monotonic clock")
                .kind(),
            ThreadErrorKind::Unsupported
        );
    }
}

#[test]
fn monotonic_runtime_time_support_tracks_thread_scheduler_truth() {
    let clock = system_monotonic_time();
    let runtime_support = clock.support();
    let scheduler_support = system_thread().support().scheduler;

    assert_eq!(
        runtime_support.caps.contains(MonotonicRuntimeTimeCaps::NOW),
        scheduler_support
            .caps
            .contains(ThreadSchedulerCaps::MONOTONIC_NOW)
    );
    assert_eq!(
        runtime_support
            .caps
            .contains(MonotonicRuntimeTimeCaps::SLEEP_FOR),
        scheduler_support
            .caps
            .contains(ThreadSchedulerCaps::SLEEP_FOR)
    );
    if runtime_support.caps.contains(MonotonicRuntimeTimeCaps::NOW) {
        assert_eq!(runtime_support.observation, scheduler_support.observation);
        assert!(runtime_support.raw_bits.is_some());
        assert!(runtime_support.tick_hz.is_some());
        assert!(runtime_support.canonicalization.is_some());
        assert!(
            runtime_support
                .caps
                .contains(MonotonicRuntimeTimeCaps::RAW_DEADLINE_COMPARE)
        );
    } else {
        assert_eq!(runtime_support.observation, ThreadGuarantee::Unsupported);
        assert_eq!(runtime_support.raw_bits, None);
        assert_eq!(runtime_support.tick_hz, None);
        assert_eq!(runtime_support.canonicalization, None);
        assert_eq!(
            runtime_support
                .caps
                .contains(MonotonicRuntimeTimeCaps::RAW_DEADLINE_COMPARE),
            false
        );
    }
    if runtime_support
        .caps
        .contains(MonotonicRuntimeTimeCaps::SLEEP_FOR)
    {
        assert!(
            runtime_support
                .caps
                .contains(MonotonicRuntimeTimeCaps::SLEEP_UNTIL)
        );
        assert!(runtime_support.deadline_wait.is_some());
    } else {
        assert_eq!(
            runtime_support
                .caps
                .contains(MonotonicRuntimeTimeCaps::SLEEP_UNTIL),
            false
        );
        assert_eq!(runtime_support.deadline_wait, None);
    }
}

#[test]
fn monotonic_runtime_time_now_is_honest() {
    let clock = system_monotonic_time();
    let support = clock.support();

    if support.caps.contains(MonotonicRuntimeTimeCaps::NOW) {
        let first = clock
            .now()
            .expect("monotonic runtime time should be readable");
        let second = clock
            .now()
            .expect("monotonic runtime time should remain readable");
        assert!(second >= first);
    } else {
        assert_eq!(
            clock
                .now()
                .expect_err("unsupported monotonic runtime time")
                .kind(),
            ThreadErrorKind::Unsupported
        );
    }
}

#[test]
fn monotonic_runtime_time_now_instant_is_honest() {
    let clock = system_monotonic_time();
    let support = clock.support();

    if support.caps.contains(MonotonicRuntimeTimeCaps::NOW) {
        let first = clock
            .now_instant()
            .expect("monotonic runtime instant should be readable");
        let second = clock
            .now_instant()
            .expect("monotonic runtime instant should remain readable");
        assert!(second >= first);
    } else {
        assert_eq!(
            clock
                .now_instant()
                .expect_err("unsupported monotonic runtime instant")
                .kind(),
            ThreadErrorKind::Unsupported
        );
    }
}

#[test]
fn monotonic_runtime_time_instant_duration_round_trip_is_honest() {
    let clock = system_monotonic_time();
    let support = clock.support();

    if support.caps.contains(MonotonicRuntimeTimeCaps::NOW) {
        let instant = clock
            .now_instant()
            .expect("monotonic runtime instant should be readable");
        let duration = clock
            .duration_from_instant(instant)
            .expect("instant should convert back into duration");
        let reconstructed = clock
            .instant_from_duration(duration)
            .expect("duration should convert back into canonical instant");
        assert!(reconstructed <= instant);
    }
}

#[test]
fn monotonic_runtime_time_sleep_for_is_honest() {
    let clock = system_monotonic_time();
    let support = clock.support();

    if support.caps.contains(MonotonicRuntimeTimeCaps::SLEEP_FOR) {
        assert!(clock.sleep_for(Duration::from_millis(1)).is_ok());
    } else {
        assert_eq!(
            clock
                .sleep_for(Duration::from_millis(1))
                .expect_err("unsupported monotonic runtime sleep")
                .kind(),
            ThreadErrorKind::Unsupported
        );
    }
}

#[test]
fn monotonic_runtime_time_sleep_until_is_honest() {
    let clock = system_monotonic_time();
    let support = clock.support();

    if support
        .caps
        .contains(MonotonicRuntimeTimeCaps::NOW | MonotonicRuntimeTimeCaps::SLEEP_UNTIL)
    {
        let start = clock
            .now_instant()
            .expect("monotonic runtime instant should be readable");
        let deadline = clock
            .checked_add_duration(start, Duration::from_millis(1))
            .expect("deadline should fit");
        clock
            .sleep_until(deadline)
            .expect("sleep_until should be supported");
    } else {
        assert_eq!(
            clock
                .instant_from_duration(Duration::from_millis(1))
                .expect_err("unsupported monotonic runtime instant conversion")
                .kind(),
            ThreadErrorKind::Unsupported
        );
    }
}

#[test]
fn monotonic_runtime_time_deadline_support_is_shaped_honestly() {
    let support = system_monotonic_time().support();

    if let Some(deadline_wait) = support.deadline_wait {
        match deadline_wait.kind {
            MonotonicDeadlineWaitKind::ReservedOneShotAlarm => {
                assert!(
                    support
                        .caps
                        .contains(MonotonicRuntimeTimeCaps::ONE_SHOT_ALARM)
                );
                assert!(deadline_wait.irqn.is_some());
            }
            MonotonicDeadlineWaitKind::RelativeSleep => {
                assert!(deadline_wait.irqn.is_none());
            }
        }
    } else {
        assert_eq!(
            support
                .caps
                .contains(MonotonicRuntimeTimeCaps::ONE_SHOT_ALARM),
            false
        );
    }
}

#[test]
fn monotonic_runtime_time_deadline_support_surfaces_event_registration_honestly() {
    let support = system_monotonic_time().support();

    match support.deadline_wait {
        Some(deadline_wait) => match deadline_wait.kind {
            MonotonicDeadlineWaitKind::ReservedOneShotAlarm => {
                let registration = support
                    .deadline_wait_registration()
                    .expect("reserved one-shot alarm should surface one registration");
                assert_eq!(registration.interest, EventInterest::READABLE);
                assert_eq!(registration.mode, EventRegistrationMode::LevelAckOnPoll);
                assert_eq!(
                    registration.source.0,
                    usize::from(
                        deadline_wait
                            .irqn
                            .expect("reserved one-shot alarm should name an irq")
                    )
                );
            }
            MonotonicDeadlineWaitKind::RelativeSleep => {
                assert_eq!(support.deadline_wait_registration(), None);
            }
        },
        None => assert_eq!(support.deadline_wait_registration(), None),
    }
}

#[test]
fn monotonic_runtime_time_one_shot_alarm_controls_are_honest() {
    let clock = system_monotonic_time();
    let support = clock.support();

    if support
        .caps
        .contains(MonotonicRuntimeTimeCaps::ONE_SHOT_ALARM)
    {
        let start = clock
            .now_instant()
            .expect("monotonic runtime instant should be readable");
        let deadline = clock
            .checked_add_duration(start, Duration::from_millis(1))
            .expect("deadline should fit");
        assert!(
            clock
                .one_shot_alarm_timeout_until(deadline)
                .expect("one-shot timeout conversion should succeed")
                .is_some()
        );
        assert!(
            clock
                .arm_one_shot_alarm_until(deadline)
                .expect("one-shot alarm should arm")
        );
        assert!(
            !clock
                .one_shot_alarm_fired()
                .expect("one-shot alarm status should be readable")
        );
        clock
            .cancel_one_shot_alarm()
            .expect("one-shot alarm should cancel");
    } else {
        assert_eq!(
            clock
                .cancel_one_shot_alarm()
                .expect_err("unsupported one-shot alarm cancel should fail")
                .kind(),
            ThreadErrorKind::Unsupported
        );
        assert_eq!(
            clock
                .one_shot_alarm_fired()
                .expect_err("unsupported one-shot alarm status should fail")
                .kind(),
            ThreadErrorKind::Unsupported
        );
    }
}

#[test]
fn system_thread_pool_executes_submitted_work_and_drains_on_shutdown() {
    let pool = SystemThreadPool::new(ThreadSystem::new(), &SystemThreadPoolConfig::new())
        .expect("thread pool should build on supported backend");
    let completed = AtomicU32::new(0);

    for _ in 0..8 {
        pool.submit(SystemWorkItem::new(
            pool_entry,
            (&raw const completed).cast_mut().cast(),
        ))
        .expect("pool should accept submitted work");
    }

    pool.shutdown().expect("pool should drain queued work");
    assert_eq!(completed.load(Ordering::Acquire), 8);
}

#[test]
fn system_thread_pool_cancels_queued_work_with_cleanup_hook() {
    let config = SystemThreadPoolConfig {
        shutdown_policy: fusion_sys::thread::SystemShutdownPolicy::CancelPending,
        ..SystemThreadPoolConfig::new()
    };
    let pool = SystemThreadPool::new(ThreadSystem::new(), &config)
        .expect("thread pool should build on supported backend");

    let started = Arc::new(AtomicU32::new(0));
    let release = Arc::new(AtomicU32::new(0));
    let blocking = BlockingPoolContext {
        started: Arc::clone(&started),
        release: Arc::clone(&release),
    };
    let executed = Arc::new(AtomicU32::new(0));
    let canceled = Arc::new(AtomicU32::new(0));
    let canceled_context = CancelPoolContext {
        executed: Arc::clone(&executed),
        canceled: Arc::clone(&canceled),
    };

    pool.submit(SystemWorkItem::new(
        blocking_pool_entry,
        (&raw const blocking).cast_mut().cast(),
    ))
    .expect("blocking work should enter the queue");
    while started.load(Ordering::Acquire) == 0 {
        core::hint::spin_loop();
    }

    pool.submit(SystemWorkItem::with_cancel(
        cancelable_pool_entry,
        (&raw const canceled_context).cast_mut().cast(),
        cancelable_pool_drop,
    ))
    .expect("cancelable work should queue behind the blocker");

    let shutdown = thread::spawn(move || {
        pool.shutdown()
            .expect("pool should shut down and cancel queued work");
    });

    while canceled.load(Ordering::Acquire) == 0 {
        core::hint::spin_loop();
    }
    release.store(1, Ordering::Release);
    shutdown.join().expect("shutdown thread should join");

    assert_eq!(executed.load(Ordering::Acquire), 0);
    assert_eq!(canceled.load(Ordering::Acquire), 1);
}
