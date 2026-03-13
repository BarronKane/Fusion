extern crate std;

use core::sync::atomic::{AtomicU32, Ordering};
use core::time::Duration;

use fusion_sys::thread::{
    ThreadConfig, ThreadEntryReturn, ThreadErrorKind, ThreadLifecycleCaps, ThreadStackCaps,
    system_thread,
};

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
        assert_eq!(error.kind, ThreadErrorKind::Unsupported);
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
                .kind,
            ThreadErrorKind::Unsupported
        );
        assert_eq!(
            thread
                .observe_current()
                .expect_err("unsupported current observe")
                .kind,
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
                .kind,
            ThreadErrorKind::StateConflict
        );
    } else {
        assert_eq!(
            result
                .expect_err("unsupported backend should reject detached spawn")
                .kind,
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
                .kind,
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
                .kind,
            ThreadErrorKind::Unsupported
        );
    }
}
