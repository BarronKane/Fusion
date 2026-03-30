use std::env;
use std::fs;
use std::path::PathBuf;

use fusion_std::thread::{
    CurrentFiberAsyncBootstrap,
    FiberStackClass,
    RuntimeSizingStrategy,
    generated_default_fiber_stack_bytes,
};

const ANALYZER_BOOTSTRAP_STACK_BYTES_ENV: &str = "FUSION_FIBER_ANALYZER_BOOTSTRAP_STACK_BYTES";
const BENCH_FIBER_COUNT: usize = 1;
const BENCH_ASYNC_CAPACITY: usize = 2;
const MIN_BENCH_FIBER_STACK_BYTES: usize = 32 * 1024;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_SIZING_GLOBAL_NEAREST_ROUND_UP");
    println!("cargo:rerun-if-env-changed=FUSION_FIBER_TASK_METADATA");
    println!("cargo:rerun-if-env-changed=FUSION_ASYNC_POLL_STACK_METADATA");
    println!("cargo:rerun-if-env-changed={ANALYZER_BOOTSTRAP_STACK_BYTES_ENV}");

    let request = runtime_slab_request();
    let stack_bytes =
        selected_stack_bytes().expect("generated benchmark fiber stack metadata should exist");
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR should exist"));
    let output = format!(
        "#[allow(dead_code)] pub const BENCH_FIBER_STACK_BYTES: usize = {stack_bytes};\n\
         #[allow(dead_code)] pub const BENCH_FIBER_COUNT: usize = {BENCH_FIBER_COUNT};\n\
         #[allow(dead_code)] pub const BENCH_ASYNC_CAPACITY: usize = {BENCH_ASYNC_CAPACITY};\n\
         #[allow(dead_code)] pub const BENCH_SLAB_ALIGN: usize = {slab_align};\n\
         #[allow(dead_code)] pub const BENCH_SLAB_BYTES: usize = {slab_bytes};\n\
         #[allow(dead_code)] #[repr(align({slab_align}))] struct BenchAlignedBacking([u8; BENCH_SLAB_BYTES]);\n\
         static BENCH_RUNTIME_INIT: fusion_std::sync::ThinMutex = fusion_std::sync::ThinMutex::new();\n\
         static BENCH_RUNTIME_READY: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);\n\
         static mut BENCH_SLAB_BACKING: BenchAlignedBacking = BenchAlignedBacking([0; BENCH_SLAB_BYTES]);\n\
         static mut BENCH_RUNTIME_STORAGE: core::mem::MaybeUninit<fusion_std::thread::CurrentFiberAsyncRuntime> = core::mem::MaybeUninit::uninit();\n\
         fn runtime() -> &'static fusion_std::thread::CurrentFiberAsyncRuntime {{\n\
             if !BENCH_RUNTIME_READY.load(core::sync::atomic::Ordering::Acquire) {{\n\
                 let _guard = BENCH_RUNTIME_INIT.lock().expect(\"rp2350 benchmark runtime init lock should acquire\");\n\
                 if !BENCH_RUNTIME_READY.load(core::sync::atomic::Ordering::Relaxed) {{\n\
                     let runtime = unsafe {{\n\
                         fusion_std::thread::CurrentFiberAsyncBootstrap::generated_static_target(BENCH_FIBER_COUNT, BENCH_ASYNC_CAPACITY)\n\
                             .expect(\"generated benchmark runtime bootstrap metadata should exist\")\n\
                             .from_static_slab((&raw mut BENCH_SLAB_BACKING).cast::<u8>(), BENCH_SLAB_BYTES)\n\
                     }}\n\
                     .expect(\"benchmark runtime should build from one owning slab\");\n\
                     unsafe {{ core::ptr::addr_of_mut!(BENCH_RUNTIME_STORAGE).write(core::mem::MaybeUninit::new(runtime)); }}\n\
                     BENCH_RUNTIME_READY.store(true, core::sync::atomic::Ordering::Release);\n\
                 }}\n\
             }}\n\
             unsafe {{ (&*core::ptr::addr_of!(BENCH_RUNTIME_STORAGE)).assume_init_ref() }}\n\
         }}\n\
         pub fn spawn_fiber<F, T>(job: F) -> Result<fusion_std::thread::CurrentFiberHandle<T>, fusion_sys::fiber::FiberError>\n\
         where\n\
             F: FnOnce() -> T + Send + 'static,\n\
             T: 'static,\n\
         {{\n\
             runtime().fibers().spawn(job)\n\
         }}\n\
         #[allow(dead_code)] pub fn drive_fibers_once() -> Result<bool, fusion_sys::fiber::FiberError> {{\n\
             runtime().fibers().drive_once()\n\
         }}\n\
         pub fn shutdown_fibers() -> Result<(), fusion_sys::fiber::FiberError> {{\n\
             runtime().fibers().shutdown()\n\
         }}\n\
         pub fn spawn_async<F>(future: F) -> Result<fusion_std::thread::TaskHandle<F::Output>, fusion_std::thread::ExecutorError>\n\
         where\n\
             F: core::future::Future + core::marker::Send + 'static,\n\
             F::Output: core::marker::Send + 'static,\n\
         {{\n\
             runtime().executor().spawn(future)\n\
         }}\n\
         pub fn block_on_async<F>(future: F) -> Result<F::Output, fusion_std::thread::ExecutorError>\n\
         where\n\
             F: core::future::Future + 'static,\n\
             F::Output: 'static,\n\
         {{\n\
             runtime().executor().block_on(future)\n\
         }}\n",
        slab_align = request.align,
        slab_bytes = request.bytes,
    );
    fs::write(out_dir.join("rp2350_backing.rs"), output)
        .expect("generated RP2350 benchmark backing constants should write");
}

#[derive(Clone, Copy)]
struct SlabRequest {
    bytes: usize,
    align: usize,
}

fn rp2350_runtime_sizing() -> RuntimeSizingStrategy {
    if env::var_os("CARGO_FEATURE_SIZING_GLOBAL_NEAREST_ROUND_UP").is_some() {
        RuntimeSizingStrategy::GlobalNearestRoundUp
    } else {
        RuntimeSizingStrategy::Exact
    }
}

fn runtime_slab_request() -> SlabRequest {
    let combined = runtime_bootstrap()
        .with_sizing_strategy(rp2350_runtime_sizing())
        .cortex_m_exact_static_backing_plan()
        .expect("exact static benchmark runtime backing plan should build");
    SlabRequest {
        bytes: combined.slab.bytes,
        align: combined.slab.align,
    }
}

fn runtime_bootstrap() -> CurrentFiberAsyncBootstrap<'static> {
    if let Some(stack_size) = analyzer_bootstrap_stack_size() {
        return CurrentFiberAsyncBootstrap::uniform_static_target(
            BENCH_FIBER_COUNT,
            stack_size,
            BENCH_ASYNC_CAPACITY,
        );
    }
    CurrentFiberAsyncBootstrap::generated_static_target(BENCH_FIBER_COUNT, BENCH_ASYNC_CAPACITY).expect(
        "generated fiber stack metadata should exist; build via `cargo pico-benchmark` or run `fusion_std_fiber_task_pipeline` first",
    )
}

fn analyzer_bootstrap_stack_size() -> Option<core::num::NonZeroUsize> {
    let raw = env::var_os(ANALYZER_BOOTSTRAP_STACK_BYTES_ENV)?;
    let bytes = raw.to_string_lossy().parse::<usize>().ok()?;
    let bytes = core::num::NonZeroUsize::new(bytes)?;
    FiberStackClass::from_stack_bytes(bytes)
        .ok()
        .map(FiberStackClass::size_bytes)
}

fn selected_stack_bytes() -> Result<usize, String> {
    if let Some(stack_size) = analyzer_bootstrap_stack_size() {
        return Ok(stack_size.get().max(MIN_BENCH_FIBER_STACK_BYTES));
    }
    generated_default_fiber_stack_bytes()
        .map(|bytes| bytes.max(MIN_BENCH_FIBER_STACK_BYTES))
        .map_err(|error| format!("{error:?}"))
}
