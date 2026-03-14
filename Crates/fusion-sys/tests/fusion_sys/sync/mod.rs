//! Integration-test canvas for `fusion_sys::sync`.
//!
//! The split here mirrors the memory tests:
//! - `all` covers the public `fusion-sys` sync contract in a capability-gated way, so
//!   unsupported backends must still fail honestly.
//! - `linux` checks the current Linux backend truth specifically, because backend
//!   implementation selection is part of the promise this layer makes.

mod all;
#[cfg(target_os = "linux")]
mod linux;
