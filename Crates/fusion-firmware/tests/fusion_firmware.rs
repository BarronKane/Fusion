#![cfg(all(feature = "std", not(target_os = "none")))]

use std::sync::OnceLock as StdOnceLock;

use fusion_std::sync::Mutex;

pub(crate) fn lock_fusion_firmware_tests() -> fusion_std::sync::MutexGuard<'static, ()> {
    static LOCK: StdOnceLock<Mutex<()>> = StdOnceLock::new();
    match LOCK.get_or_init(|| Mutex::new(())).lock() {
        Ok(guard) => guard,
        Err(error) => panic!(
            "fusion-firmware integration tests should serialize shared runtime state: {error}"
        ),
    }
}

#[path = "fusion_firmware/root_execution.rs"]
mod root_execution;
#[path = "fusion_firmware/runtime.rs"]
mod runtime;
