//! Public threading and runtime domains.

#[cfg(all(test, feature = "std"))]
use std::sync::Mutex;
#[cfg(all(test, feature = "std"))]
use std::sync::MutexGuard;
#[cfg(all(test, feature = "std"))]
use std::sync::OnceLock;

#[path = "executor/executor.rs"]
mod executor;
#[path = "fiber/fiber.rs"]
mod fiber;
mod fiber_audit;
mod graph;
mod pool;
mod red;
#[path = "runtime/runtime.rs"]
mod runtime;
mod runtime_audit;
mod system;
mod tier;

#[cfg(all(test, feature = "std"))]
pub(crate) fn runtime_test_guard() -> MutexGuard<'static, ()> {
    static HOSTED_TEST_GUARD: OnceLock<Mutex<()>> = OnceLock::new();
    HOSTED_TEST_GUARD
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub use executor::*;
pub use fiber::*;
pub use fiber_audit::*;
pub use graph::*;
pub use pool::*;
pub use red::*;
pub use runtime::*;
pub use runtime_audit::*;
pub use system::*;
pub use tier::*;
