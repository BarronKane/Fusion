//! Integration-test canvas for `fusion_sys::fiber`.
//!
//! `all` validates the public stackful/context surface in a capability-gated way so
//! unsupported backends still have to fail honestly. `linux` checks the current Linux
//! backend truth specifically.

mod all;
#[cfg(target_os = "linux")]
mod linux;
