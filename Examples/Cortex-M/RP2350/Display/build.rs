use std::env;
use std::fs;
use std::num::NonZeroUsize;
use std::path::PathBuf;

use fusion_std::thread::{
    FiberPoolBootstrap,
    FiberStackClass,
    RuntimeSizingStrategy,
    generated_default_fiber_stack_bytes,
};

const ANALYZER_BOOTSTRAP_STACK_BYTES_ENV: &str = "FUSION_FIBER_ANALYZER_BOOTSTRAP_STACK_BYTES";
const DISPLAY_FIBER_COUNT: usize = 2;
const MIN_DISPLAY_FIBER_STACK_BYTES: usize = 32 * 1024;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_SIZING_GLOBAL_NEAREST_ROUND_UP");
    println!("cargo:rerun-if-env-changed=FUSION_FIBER_TASK_METADATA");
    println!("cargo:rerun-if-env-changed=FUSION_ASYNC_POLL_STACK_METADATA");
    println!("cargo:rerun-if-env-changed={ANALYZER_BOOTSTRAP_STACK_BYTES_ENV}");

    let stack_bytes =
        selected_stack_bytes().expect("generated display fiber stack metadata should exist");
    let fiber_slab = fiber_pool_slab_request(stack_bytes, rp2350_fiber_sizing());

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR should exist"));
    let output = format!(
        "#[allow(dead_code)] pub const DISPLAY_FIBER_STACK_BYTES: usize = {stack_bytes};\n\
         #[allow(dead_code)] pub const DISPLAY_FIBER_COUNT: usize = {fiber_count};\n\
         #[allow(dead_code)] pub const FIBER_POOL_SLAB_ALIGN: usize = {fiber_align};\n\
         #[allow(dead_code)] pub const FIBER_POOL_SLAB_BYTES: usize = {fiber_bytes};\n\
         #[allow(dead_code)] #[repr(align({fiber_align}))] struct FiberPoolAlignedBacking([u8; FIBER_POOL_SLAB_BYTES]);\n\
         static DISPLAY_FIBERS_INIT: fusion_std::sync::ThinMutex = fusion_std::sync::ThinMutex::new();\n\
         static DISPLAY_FIBERS_READY: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);\n\
         static mut FIBER_POOL_SLAB_BACKING: FiberPoolAlignedBacking = FiberPoolAlignedBacking([0; FIBER_POOL_SLAB_BYTES]);\n\
         static mut DISPLAY_FIBERS_STORAGE: core::mem::MaybeUninit<fusion_std::thread::CurrentFiberPool> = core::mem::MaybeUninit::uninit();\n\
         fn fibers() -> &'static fusion_std::thread::CurrentFiberPool {{\n\
             if !DISPLAY_FIBERS_READY.load(core::sync::atomic::Ordering::Acquire) {{\n\
                 let _guard = DISPLAY_FIBERS_INIT.lock().expect(\"rp2350 display fiber init lock should acquire\");\n\
                 if !DISPLAY_FIBERS_READY.load(core::sync::atomic::Ordering::Relaxed) {{\n\
                     let fibers = unsafe {{\n\
                         fusion_std::thread::FiberPoolBootstrap::uniform_static_target(\n\
                             DISPLAY_FIBER_COUNT,\n\
                             core::num::NonZeroUsize::new(DISPLAY_FIBER_STACK_BYTES)\n\
                                 .expect(\"display fiber stack should be non-zero\"),\n\
                         )\n\
                         .from_static_slab((&raw mut FIBER_POOL_SLAB_BACKING).cast::<u8>(), FIBER_POOL_SLAB_BYTES)\n\
                     }}\n\
                     .expect(\"display fiber pool should build from explicit static backing\");\n\
                     unsafe {{ core::ptr::addr_of_mut!(DISPLAY_FIBERS_STORAGE).write(core::mem::MaybeUninit::new(fibers)); }}\n\
                     DISPLAY_FIBERS_READY.store(true, core::sync::atomic::Ordering::Release);\n\
                 }}\n\
             }}\n\
             unsafe {{ (&*core::ptr::addr_of!(DISPLAY_FIBERS_STORAGE)).assume_init_ref() }}\n\
         }}\n\
         pub fn spawn<F, T>(job: F) -> Result<fusion_std::thread::CurrentFiberHandle<T>, fusion_sys::fiber::FiberError>\n\
         where\n\
             F: FnOnce() -> T + Send + 'static,\n\
             T: 'static,\n\
         {{\n\
             fibers().spawn(job)\n\
         }}\n\
         pub fn drive_once() -> Result<bool, fusion_sys::fiber::FiberError> {{\n\
             fibers().drive_once()\n\
         }}\n",
        fiber_count = DISPLAY_FIBER_COUNT,
        fiber_align = fiber_slab.align,
        fiber_bytes = fiber_slab.bytes,
    );
    fs::write(out_dir.join("rp2350_backing.rs"), output)
        .expect("generated RP2350 display backing constants should write");
}

#[derive(Clone, Copy)]
struct SlabRequest {
    bytes: usize,
    align: usize,
}

fn rp2350_fiber_sizing() -> RuntimeSizingStrategy {
    if env::var_os("CARGO_FEATURE_SIZING_GLOBAL_NEAREST_ROUND_UP").is_some() {
        RuntimeSizingStrategy::GlobalNearestRoundUp
    } else {
        RuntimeSizingStrategy::Exact
    }
}

fn fiber_pool_slab_request(stack_bytes: usize, sizing: RuntimeSizingStrategy) -> SlabRequest {
    let combined = FiberPoolBootstrap::uniform_static_target(
        DISPLAY_FIBER_COUNT,
        NonZeroUsize::new(stack_bytes).expect("display fiber stack should be non-zero"),
    )
    .with_sizing_strategy(sizing)
    .cortex_m_backing_plan()
    .expect("exact static display fiber-pool backing plan should build")
    ;
    SlabRequest {
        bytes: combined.slab.bytes,
        align: combined.slab.align,
    }
}

fn analyzer_bootstrap_stack_size() -> Option<NonZeroUsize> {
    let raw = env::var_os(ANALYZER_BOOTSTRAP_STACK_BYTES_ENV)?;
    let bytes = raw.to_string_lossy().parse::<usize>().ok()?;
    let bytes = NonZeroUsize::new(bytes)?;
    FiberStackClass::from_stack_bytes(bytes)
        .ok()
        .map(FiberStackClass::size_bytes)
}

fn selected_stack_bytes() -> Result<usize, String> {
    if let Some(stack_size) = analyzer_bootstrap_stack_size() {
        return Ok(stack_size.get().max(MIN_DISPLAY_FIBER_STACK_BYTES));
    }
    generated_default_fiber_stack_bytes()
        .map(|bytes| bytes.max(MIN_DISPLAY_FIBER_STACK_BYTES))
        .map_err(|error| format!("{error:?}"))
}
