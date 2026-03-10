//! Integration-test canvas for `fusion_sys::mem::resource`.
//!
//! The split here is deliberate:
//! - `all` covers capability-gated behavior that should be expressed consistently on any
//!   backend, even when the concrete answer is "unsupported".
//! - `linux` covers behavior that depends on the current Linux PAL implementation.
//! - `support` holds tiny helpers shared by both test sets.

mod all;
#[cfg(target_os = "linux")]
mod linux;
mod support;
