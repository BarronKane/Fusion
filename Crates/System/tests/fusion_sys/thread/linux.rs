extern crate std;

use core::sync::atomic::{AtomicU32, Ordering};

use fusion_sys::thread::{
    ThreadConfig, ThreadConstraintMode, ThreadIdentityStability, ThreadPlacementPhase,
    ThreadPlacementRequest, ThreadSchedulerClass, ThreadSchedulerModel, system_thread,
};
use rustix::thread::{self as rustix_thread, CpuSet};

#[repr(C)]
struct AffinityContext {
    observed_cpu: AtomicU32,
}

unsafe fn affinity_entry(context: *mut ()) -> fusion_sys::thread::ThreadEntryReturn {
    let thread = system_thread();
    let observation = thread
        .observe_current()
        .expect("current thread observation should succeed");
    let context = unsafe { &*(context.cast::<AffinityContext>()) };
    if let Some(cpu) = observation.location.logical_cpu {
        context
            .observed_cpu
            .store(u32::from(cpu.index), Ordering::Release);
    }
    fusion_sys::thread::ThreadEntryReturn::new(0)
}

#[test]
fn linux_thread_support_reports_expected_backend_truth() {
    let support = system_thread().support();

    assert_eq!(support.scheduler.model, ThreadSchedulerModel::Preemptive);
    assert_eq!(
        support.lifecycle.identity_stability,
        ThreadIdentityStability::ThreadLifetime
    );
    assert!(
        support
            .placement
            .caps
            .contains(fusion_sys::thread::ThreadPlacementCaps::LOGICAL_CPU_AFFINITY)
    );
    assert!(
        !support
            .lifecycle
            .caps
            .contains(fusion_sys::thread::ThreadLifecycleCaps::SUSPEND)
    );
}

#[test]
fn linux_thread_priority_ranges_are_class_aware() {
    let thread = system_thread();

    assert_eq!(
        thread
            .priority_range(ThreadSchedulerClass::Deadline)
            .expect("query should succeed"),
        None
    );
    assert!(
        thread
            .priority_range(ThreadSchedulerClass::FixedPriorityRealtime)
            .expect("query should succeed")
            .is_some()
    );
}

#[test]
fn linux_thread_applies_requested_affinity_before_user_entry() {
    let cpuset = rustix_thread::sched_getaffinity(None).expect("affinity query should succeed");
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

    let thread = system_thread();
    let context = AffinityContext {
        observed_cpu: AtomicU32::new(u32::MAX),
    };
    let cpus = [fusion_sys::thread::ThreadLogicalCpuId {
        group: fusion_sys::thread::ThreadProcessorGroupId(0),
        index: selected_cpu,
    }];
    let config = ThreadConfig {
        placement: ThreadPlacementRequest {
            logical_cpus: &cpus,
            mode: ThreadConstraintMode::Require,
            phase: ThreadPlacementPhase::PreStartRequired,
            ..ThreadPlacementRequest::new()
        },
        start_mode: fusion_sys::thread::ThreadStartMode::PlacementCommitted,
        ..ThreadConfig::new()
    };

    let handle = unsafe {
        thread
            .spawn_raw(
                &config,
                affinity_entry,
                (&raw const context).cast_mut().cast(),
            )
            .expect("thread should spawn")
    };

    thread.join(handle).expect("thread should join");
    assert_eq!(
        context.observed_cpu.load(Ordering::Acquire),
        u32::from(selected_cpu)
    );
}
