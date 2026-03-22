use core::num::NonZeroUsize;

use fusion_std::thread::{
    CurrentFiberPool,
    FiberPoolConfig,
    FiberStackClass,
    FiberStackClassConfig,
};

fn main() {
    let classes = [
        FiberStackClassConfig::new(FiberStackClass::MIN, 2).expect("small class should build"),
        FiberStackClassConfig::new(
            FiberStackClass::new(NonZeroUsize::new(8 * 1024).expect("non-zero large class"))
                .expect("large class should build"),
            1,
        )
        .expect("large class config should build"),
    ];
    let pool = CurrentFiberPool::new(
        &FiberPoolConfig::classed(&classes).expect("classed pool config should build"),
    )
    .expect("current-thread fiber pool should build");

    let first = pool
        .spawn(|| 1_u32)
        .expect("first closure task should fit the generated small class");
    let second = pool
        .spawn(|| 2_u32)
        .expect("second closure task should also fit the generated small class");

    assert_eq!(
        first
            .join()
            .expect("first fixture closure should complete cleanly"),
        1
    );
    assert_eq!(
        second
            .join()
            .expect("second fixture closure should complete cleanly"),
        2
    );

    pool.shutdown()
        .expect("fixture current-thread fiber pool should shut down cleanly");
}
