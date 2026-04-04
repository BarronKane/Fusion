#![feature(test)]

extern crate test;

#[path = "support/support.rs"]
mod support;

use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::OnceLock;
use test::Bencher;

fn compare_tokio_bench_guard() -> MutexGuard<'static, ()> {
    static COMPARE_TOKIO_BENCH_GUARD: OnceLock<Mutex<()>> = OnceLock::new();
    COMPARE_TOKIO_BENCH_GUARD
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

macro_rules! bench_wrap {
    ($name:ident) => {
        #[bench]
        fn $name(b: &mut Bencher) {
            let _guard = compare_tokio_bench_guard();
            support::$name(b);
        }
    };
}

macro_rules! bench_wrap_ignore {
    ($name:ident, $reason:expr) => {
        #[bench]
        #[ignore = $reason]
        fn $name(b: &mut Bencher) {
            let _guard = compare_tokio_bench_guard();
            support::$name(b);
        }
    };
}

bench_wrap!(current_async_runtime_spawn_join_noop);
bench_wrap!(current_async_runtime_spawn_join_yield_once);
bench_wrap_ignore!(
    current_async_runtime_cross_thread_wake_once,
    "cross-thread wake latency is benchmarked in isolation because the full bench sweep makes this probe flaky"
);
bench_wrap!(current_async_runtime_contention_yield_32x32);
bench_wrap!(thread_async_runtime_lifecycle_noop_workers_1);
bench_wrap!(thread_async_runtime_lifecycle_noop_workers_2);
bench_wrap!(thread_async_runtime_lifecycle_noop_workers_4);
bench_wrap!(thread_async_runtime_lifecycle_yield_once_workers_1);
bench_wrap!(thread_async_runtime_lifecycle_yield_once_workers_2);
bench_wrap!(thread_async_runtime_lifecycle_yield_once_workers_4);
bench_wrap!(tokio_current_thread_spawn_join_noop);
bench_wrap!(tokio_current_thread_spawn_join_yield_once);
bench_wrap_ignore!(
    tokio_current_thread_cross_thread_wake_once,
    "cross-thread wake latency is benchmarked in isolation because the full bench sweep makes this probe flaky"
);
bench_wrap!(tokio_current_thread_contention_yield_32x32);
bench_wrap!(tokio_multi_thread_lifecycle_noop_workers_1);
bench_wrap!(tokio_multi_thread_lifecycle_noop_workers_2);
bench_wrap!(tokio_multi_thread_lifecycle_noop_workers_4);
bench_wrap!(tokio_multi_thread_lifecycle_yield_once_workers_1);
bench_wrap!(tokio_multi_thread_lifecycle_yield_once_workers_2);
bench_wrap!(tokio_multi_thread_lifecycle_yield_once_workers_4);
