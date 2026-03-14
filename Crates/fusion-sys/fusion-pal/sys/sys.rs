//! Selected platform backend wiring for the fusion-pal.
//!
//! Each supported target has a private `sys::<platform>` module that implements the fusion-pal
//! contracts. The public `sys::<module>` exports re-export the chosen backend through a
//! uniform path such as `fusion_pal::sys::mem`.
//!
//! Safety-critical note:
//! This module is the top of the platform-implementation boundary for `fusion-pal`. Any
//! dependency, syscall wrapper, foreign-function interface, or operating-system import used
//! beneath `sys::<platform>` becomes part of the assurance case for the selected target.
//! That matters directly for higher-assurance environments governed by standards such as
//! DO-178C and IEC 62304, where the auditability and qualification story of every boundary
//! crossing is not optional just because the code is fashionable and written in Rust.
//!
//! In practice, different platforms offer very different levels of control:
//! - Linux exposes a stable syscall ABI, so the backend can in principle own the full path
//!   from Rust to kernel entry. The current Linux backend uses `rustix` for that boundary,
//!   but this is an implementation choice rather than a philosophical requirement, and it
//!   may be replaced with a thinner directly owned syscall layer if stricter qualification
//!   or traceability requirements demand it.
//! - Apple targets do not offer a public stable syscall ABI in the same way, so serious fusion-pal
//!   implementations are typically forced through `libSystem` or equivalent system-provided
//!   foreign interfaces.
//! - Windows similarly pushes user-mode code through system DLL boundaries rather than a
//!   public stable raw syscall contract suitable for a portable fusion-pal to own directly.
//!
//! The fusion-pal therefore treats backend truth and backend auditability as first-class concerns.
//! Some targets may be able to support a stronger critical-safety claim than others, and the
//! library should say that plainly rather than smoothing the boundary over with “portable”
//! abstractions that become fiction under real assurance scrutiny.

#[cfg(not(any(
    target_os = "ios",
    target_os = "linux",
    target_os = "macos",
    target_os = "windows"
)))]
compile_error!("fusion-pal currently supports only Linux, Windows, macOS, and iOS targets.");

#[cfg(target_os = "ios")]
#[path = "ios/ios.rs"]
mod ios;
#[cfg(target_os = "ios")]
use ios as platform;

#[cfg(target_os = "linux")]
#[path = "linux/linux.rs"]
mod linux;
#[cfg(target_os = "linux")]
use linux as platform;

#[cfg(target_os = "macos")]
#[path = "macos/macos.rs"]
mod macos;
#[cfg(target_os = "macos")]
use macos as platform;

#[cfg(target_os = "windows")]
#[path = "windows/windows.rs"]
mod windows;
#[cfg(target_os = "windows")]
use windows as platform;

/// Public user-space context module re-exported from the selected private platform backend.
pub mod context;
/// Public event module re-exported from the selected private platform backend.
pub mod event;
#[cfg(feature = "sys-fusion-kn")]
/// Public mediated Fusion kernel backend module.
pub mod fusion_kn;
/// Public hardware module re-exported from the selected private platform backend.
pub mod hal;
/// Public memory module re-exported from the selected private platform backend.
pub mod mem;
/// Public synchronization module re-exported from the selected private platform backend.
pub mod sync;
/// Public thread module re-exported from the selected private platform backend.
pub mod thread;
