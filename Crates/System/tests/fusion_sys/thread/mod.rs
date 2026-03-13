//! Integration-test canvas for `fusion_sys::thread`.
//!
//! `all` covers the public thread wrapper contract in a capability-gated way so unsupported
//! backends still have to fail honestly.
//! `linux` checks the currently implemented Linux backend truth specifically.

mod all;
#[cfg(target_os = "linux")]
mod linux;
