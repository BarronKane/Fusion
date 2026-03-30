//! Selected platform facade wiring for the fusion-pal.
//!
//! The raw backend families live under [`crate::pal`]. This module consumes those PAL families and
//! exposes the canonical selected public surface through uniform paths such as
//! `fusion_pal::sys::mem`.
//!
//! Safety-critical note:
//! This module is the top of the selected public boundary for `fusion-pal`. Any dependency,
//! syscall wrapper, foreign-function interface, or operating-system import used beneath
//! [`crate::pal`] becomes part of the assurance case for the selected target.
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
    all(target_os = "none", feature = "sys-cortex-m"),
    target_os = "ios",
    target_os = "linux",
    target_os = "macos",
    target_os = "windows"
)))]
compile_error!(
    "fusion-pal currently supports Linux, Windows, macOS, iOS, and Cortex-M (feature `sys-cortex-m`) targets."
);

#[cfg(all(feature = "sys-cortex-m", not(target_os = "none")))]
compile_error!(
    "fusion-pal feature `sys-cortex-m` requires a bare-metal target (target_os = \"none\")."
);

#[cfg(all(feature = "soc-rp2350", not(feature = "sys-cortex-m")))]
compile_error!("fusion-pal Cortex-M SoC features require `sys-cortex-m`.");

#[cfg(all(feature = "sys-fusion-kn", not(target_os = "linux")))]
compile_error!("fusion-pal feature `sys-fusion-kn` currently supports only Linux targets.");

/// Dynamic bare-metal hardware-discovery and firmware-enumeration family facade.
pub mod hal {
    pub use crate::pal::hal::*;
}

/// Hosted implementation-family facade.
pub mod hosted {
    pub use crate::pal::hosted::*;
}

/// Static SoC implementation-family facade.
pub mod soc {
    pub use crate::pal::soc::*;
}

#[cfg(target_os = "ios")]
use crate::pal::hosted::ios as platform;
#[cfg(all(target_os = "none", feature = "sys-cortex-m"))]
use crate::pal::soc::cortex_m as platform;

#[cfg(all(target_os = "linux", not(feature = "sys-fusion-kn")))]
use crate::pal::hosted::linux as platform;

#[cfg(all(target_os = "linux", feature = "sys-fusion-kn"))]
use crate::pal::hosted::fusion_kn as platform;

#[cfg(target_os = "macos")]
use crate::pal::hosted::macos as platform;

#[cfg(target_os = "windows")]
use crate::pal::hosted::windows as platform;

/// Public execution-context module re-exported from the selected platform backend.
pub mod execution_context {
    pub use super::platform::context::{
        PlatformContext,
        PlatformSavedContext,
        system_context,
        system_context_support,
    };
    pub use crate::contract::pal::runtime::context::*;
}
/// Public atomic substrate module re-exported from the selected platform backend.
pub mod atomic {
    pub use super::platform::atomic::{
        PLATFORM_ATOMIC_WAIT_WORD32_IMPLEMENTATION,
        PLATFORM_ATOMIC_WORD32_IMPLEMENTATION,
        PlatformAtomic,
        PlatformAtomicWord32,
        system_atomic,
    };
    pub use crate::contract::pal::runtime::atomic::*;
}
/// Public native visible-context contract surface.
pub mod context {
    pub use crate::contract::pal::domain::{
        ContextBase,
        ContextCaps,
        ContextId,
        ContextImplementationKind,
        ContextKind,
        ContextProjectionKind,
        ContextSupport,
        CourierId,
        DomainError,
        DomainErrorKind,
        DomainId,
        UnsupportedContext,
    };
}
/// Public CPU- and topology-oriented hardware module re-exported from the selected backend.
pub mod cpu {
    #[cfg(all(target_os = "none", feature = "sys-cortex-m"))]
    pub use super::platform::hal::core;
    #[cfg(all(target_os = "none", feature = "sys-cortex-m"))]
    pub use super::platform::hal::soc;
    pub use super::platform::hal::{
        PLATFORM_CACHE_LINE_ALIGN_BYTES,
        PlatformCachePadded as CachePadded,
        PlatformHardware as PlatformCpu,
        system_hardware as system_cpu,
    };
}
/// Public courier contract surface.
pub mod courier {
    pub use crate::contract::pal::domain::{
        ContextId,
        CourierBase,
        CourierCaps,
        CourierId,
        CourierImplementationKind,
        CourierSupport,
        CourierVisibility,
        CourierVisibilityControl,
        DomainError,
        DomainErrorKind,
        DomainId,
        UnsupportedCourier,
    };
}
/// Public channel contract surface.
pub mod channel {
    pub use crate::contract::pal::interconnect::channel::*;
}
/// Public insight/debug side-channel contract surface.
pub mod insight {
    pub use crate::contract::pal::interconnect::insight::*;
}
/// Public event module re-exported from the selected platform backend.
pub mod event {
    pub use super::platform::event::{PlatformEvent, PlatformPoller, system_event};
    pub use crate::contract::pal::runtime::event::*;
}
/// Public GPIO driver module re-exported from the selected platform backend.
pub mod gpio {
    pub use super::platform::gpio::{PlatformGpio, PlatformGpioPin, system_gpio};
    pub use crate::contract::drivers::gpio::*;
}
/// Public hosted-fiber helper module re-exported from the selected platform backend.
pub mod fiber {
    pub use super::platform::fiber::{
        PlatformFiberHost,
        PlatformFiberSignalStack,
        PlatformFiberWakeSignal,
        system_fiber_host,
    };
    pub use crate::contract::pal::runtime::fiber::{
        FiberHostError,
        FiberHostErrorKind,
        FiberHostSupport,
        PlatformElasticFaultHandler,
        PlatformWakeToken,
    };
}
#[cfg(feature = "sys-fusion-kn")]
/// Public mediated Fusion kernel backend module.
pub mod fusion_kn {
    pub use super::hosted::fusion_kn::*;
}
/// Public protocol contract surface.
pub mod protocol {
    pub use crate::contract::pal::interconnect::protocol::*;
}
/// Public native domain contract surface.
pub mod domain {
    pub use crate::contract::pal::domain::{
        CourierId,
        DomainBase,
        DomainCaps,
        DomainError,
        DomainErrorKind,
        DomainId,
        DomainImplementationKind,
        DomainKind,
        DomainSupport,
        UnsupportedDomain,
    };
}
/// Public memory module re-exported from the selected platform backend.
pub mod mem {
    pub use super::platform::mem::{PlatformMem, system_mem};
    pub use crate::contract::pal::mem::*;
}
/// Public programmable-IO module re-exported from the selected platform backend.
pub mod pcu {
    pub use super::platform::pcu::{PlatformPcu, system_pcu};
    pub use crate::contract::drivers::pcu::*;
}
/// Public power module re-exported from the selected platform backend.
pub mod power {
    pub use super::platform::power::{PlatformPower, system_power};
    pub use crate::contract::pal::power::*;
}
/// Public synchronization module re-exported from the selected platform backend.
pub mod sync {
    pub use super::platform::sync::{
        PLATFORM_RAW_MUTEX_IMPLEMENTATION,
        PLATFORM_RAW_ONCE_IMPLEMENTATION,
        PLATFORM_RAW_RWLOCK_IMPLEMENTATION,
        PlatformRawMutex,
        PlatformRawOnce,
        PlatformRawRwLock,
        PlatformSemaphore,
        PlatformSync,
        system_sync,
    };
    pub use crate::contract::pal::runtime::sync::*;
}
/// Public thread module re-exported from the selected platform backend.
pub mod thread {
    pub use super::platform::thread::{PlatformThread, PlatformThreadHandle, system_thread};
    pub use crate::contract::pal::runtime::thread::*;
}
/// Public transport-layer contract surface.
pub mod transport {
    pub use crate::contract::pal::interconnect::transport::*;
}
/// Public vector module re-exported from the selected platform backend.
pub mod vector {
    pub use super::platform::vector::{
        PlatformSealedVectorTable,
        PlatformVector,
        PlatformVectorBuilder,
        bind_reserved_pendsv_dispatch,
        system_vector,
        take_pending_active_scope,
    };
    pub use crate::contract::pal::vector::*;
}
