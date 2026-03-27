#![cfg(all(feature = "std", not(target_os = "none")))]

use fusion_std::sync::Mutex;
use std::sync::OnceLock as StdOnceLock;

pub(crate) fn lock_fusion_std_tests() -> fusion_std::sync::MutexGuard<'static, ()> {
    static LOCK: StdOnceLock<Mutex<()>> = StdOnceLock::new();
    match LOCK.get_or_init(|| Mutex::new(())).lock() {
        Ok(guard) => guard,
        Err(error) => {
            panic!("fusion-std integration tests should serialize shared runtime state: {error}")
        }
    }
}

#[path = "fusion_std/all.rs"]
mod all;

#[cfg(target_os = "linux")]
#[path = "fusion_std/linux.rs"]
mod linux;
