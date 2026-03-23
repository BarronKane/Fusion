#![feature(test)]

extern crate test;

#[path = "support/mod.rs"]
mod support;

use test::Bencher;

macro_rules! bench_wrap {
    ($name:ident) => {
        #[bench]
        fn $name(b: &mut Bencher) {
            support::$name(b);
        }
    };
}

bench_wrap!(baseline_direct_noop);
bench_wrap!(fiber_low_level_create);
bench_wrap!(fiber_low_level_resume_yield_round_trip);
bench_wrap!(current_fiber_pool_spawn_join_noop);
bench_wrap!(current_fiber_pool_spawn_with_stack_join_noop);
bench_wrap!(current_fiber_pool_spawn_join_yield_once);
bench_wrap!(current_fiber_pool_spawn_join_yield_ten_local_state);
bench_wrap!(current_fiber_pool_spawn_join_recursive_stack);
bench_wrap!(reactor_readiness_batch_16);
bench_wrap!(reactor_readiness_batch_64);
