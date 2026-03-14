//! Integration-test canvas for `fusion_sys::event`.
//!
//! `all` validates the public event wrapper in a capability-gated way so unsupported
//! backends still have to fail honestly. `linux` checks the currently implemented Linux
//! readiness backend specifically.

mod all;
#[cfg(target_os = "linux")]
mod linux;
