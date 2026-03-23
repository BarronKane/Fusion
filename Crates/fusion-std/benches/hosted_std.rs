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

bench_wrap!(thread_pool_lifecycle_only_workers_1);
bench_wrap!(thread_pool_lifecycle_only_workers_2);
bench_wrap!(thread_pool_lifecycle_only_workers_4);
bench_wrap!(thread_pool_dispatch_round_trip_workers_1);
bench_wrap!(thread_pool_dispatch_round_trip_workers_2);
bench_wrap!(thread_pool_dispatch_round_trip_workers_4);
bench_wrap!(thread_pool_throughput_noop_workers_1);
bench_wrap!(thread_pool_throughput_noop_workers_2);
bench_wrap!(thread_pool_throughput_noop_workers_4);
bench_wrap!(thread_pool_lifecycle_batch_noop_workers_1);
bench_wrap!(thread_pool_lifecycle_batch_noop_workers_2);
bench_wrap!(thread_pool_lifecycle_batch_noop_workers_4);
bench_wrap!(green_pool_spawn_join_noop);
bench_wrap!(green_pool_spawn_with_stack_join_noop);
bench_wrap!(green_pool_spawn_join_yield_once);
bench_wrap!(green_pool_throughput_noop_carriers_1);
bench_wrap!(green_pool_throughput_noop_carriers_2);
bench_wrap!(green_pool_throughput_noop_carriers_4);
bench_wrap!(green_pool_throughput_yield_once_carriers_1);
bench_wrap!(green_pool_throughput_yield_once_carriers_2);
bench_wrap!(green_pool_throughput_yield_once_carriers_4);
bench_wrap!(green_pool_lifecycle_noop_carriers_1);
bench_wrap!(green_pool_lifecycle_noop_carriers_2);
bench_wrap!(green_pool_lifecycle_noop_carriers_4);
bench_wrap!(green_pool_lifecycle_yield_once_carriers_1);
bench_wrap!(green_pool_lifecycle_yield_once_carriers_2);
bench_wrap!(green_pool_lifecycle_yield_once_carriers_4);
bench_wrap!(green_pool_bootstrap_only_carriers_1);
bench_wrap!(green_pool_bootstrap_only_carriers_2);
bench_wrap!(green_pool_bootstrap_only_carriers_4);
