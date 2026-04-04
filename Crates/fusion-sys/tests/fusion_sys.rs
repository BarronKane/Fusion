#![cfg(all(feature = "std", not(target_os = "none")))]

#[path = "fusion_sys/fusion_sys.rs"]
mod fusion_sys;
