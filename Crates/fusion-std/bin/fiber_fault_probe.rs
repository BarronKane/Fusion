use std::hint::spin_loop;
use std::num::NonZeroUsize;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use fusion_std::thread::{
    FiberStackBacking,
    FiberStackClass,
    FiberTaskAttributes,
    FiberTelemetry,
    GreenPool,
    GreenPoolConfig,
    ThreadPool,
    ThreadPoolConfig,
};

#[inline(never)]
fn consume_stack(depth: usize) -> usize {
    let mut page = [0_u8; 4096];
    let value = depth.to_le_bytes()[0];
    for offset in (0..page.len()).step_by(256) {
        unsafe {
            core::ptr::write_volatile(&raw mut page[offset], value);
        }
    }

    if depth == 0 {
        return usize::from(page[0]);
    }

    let child = consume_stack(depth - 1);
    child + usize::from(page[depth % page.len()])
}

fn main() -> ExitCode {
    #[cfg(not(target_os = "linux"))]
    {
        return ExitCode::SUCCESS;
    }

    #[cfg(target_os = "linux")]
    {
        let Ok(carrier) = ThreadPool::new(&ThreadPoolConfig::new()) else {
            return ExitCode::from(10);
        };
        let Ok(fibers) = GreenPool::new(
            &GreenPoolConfig {
                stack_backing: FiberStackBacking::Elastic {
                    initial_size: NonZeroUsize::new(4 * 1024).expect("non-zero initial stack"),
                    max_size: NonZeroUsize::new(64 * 1024).expect("non-zero max stack"),
                },
                growth_chunk: 1,
                max_fibers_per_carrier: 1,
                telemetry: FiberTelemetry::Full,
                ..GreenPoolConfig::new()
            },
            &carrier,
        ) else {
            return ExitCode::from(11);
        };

        let entered = Arc::new(AtomicBool::new(false));
        let release = Arc::new(AtomicBool::new(false));
        let entered_for_job = Arc::clone(&entered);
        let release_for_job = Arc::clone(&release);
        let Ok(handle) =
            fibers.spawn_with_attrs(FiberTaskAttributes::new(FiberStackClass::MIN), move || {
                let _ = std::hint::black_box(consume_stack(8));
                entered_for_job.store(true, Ordering::Release);
                while !release_for_job.load(Ordering::Acquire) {
                    spin_loop();
                }
            })
        else {
            return ExitCode::from(12);
        };

        let deadline = Instant::now() + Duration::from_secs(5);
        while !entered.load(Ordering::Acquire) && Instant::now() < deadline {
            std::thread::yield_now();
        }
        if !entered.load(Ordering::Acquire) {
            return ExitCode::from(13);
        }

        let mut observed_growth = false;
        while Instant::now() < deadline {
            if let Some(stats) = fibers.stack_stats()
                && (stats.total_growth_events > 0 || stats.peak_committed_pages > 1)
            {
                observed_growth = true;
                break;
            }
            std::thread::yield_now();
        }

        release.store(true, Ordering::Release);
        if handle.join().is_err() {
            return ExitCode::from(14);
        }
        if fibers.shutdown().is_err() {
            return ExitCode::from(15);
        }
        if carrier.shutdown().is_err() {
            return ExitCode::from(16);
        }

        if observed_growth {
            ExitCode::SUCCESS
        } else {
            ExitCode::from(17)
        }
    }
}
