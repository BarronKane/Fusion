//! Linux fusion-pal user-space context backend selection.
//!
//! This partition is intentionally scoped to Linux user-space targets, not every platform that
//! happens to share the same ISA.
//!
//! Current native support is:
//! - `x86_64` Linux user space
//! - `aarch64` Linux user space
//!
//! That means the `aarch64` backend covers the normal 64-bit Arm Linux world: servers, SBCs,
//! embedded Linux boards, and other devices running the standard Linux AArch64 user-space ABI.
//! It does not imply support for 32-bit ARM, bare-metal environments, Windows on Arm, or Darwin.
//!
//! Apple Silicon hardware is architecturally relevant because it is `aarch64`, but this backend
//! only applies when that hardware is running Linux. macOS and iOS would need their own
//! platform-specific context partition even if the register-save logic ends up looking similar.
//!
//! Native assembly is used where the ISA backend exists. Unsupported targets stay unsupported
//! instead of inheriting a deprecated userspace coroutine API out of impatience.

#[cfg(target_arch = "x86_64")]
#[path = "x86_64.rs"]
mod implementation;

#[cfg(target_arch = "aarch64")]
#[path = "aarch64.rs"]
mod implementation;

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
mod implementation {
    use crate::pal::context::{UnsupportedContext, UnsupportedSavedContext};

    /// Selected Linux context provider type.
    pub type PlatformContext = UnsupportedContext;
    /// Selected Linux saved-context type.
    pub type PlatformSavedContext = UnsupportedSavedContext;

    /// Returns the selected Linux context provider.
    #[must_use]
    pub const fn system_context() -> PlatformContext {
        PlatformContext::new()
    }
}

pub use implementation::{PlatformContext, PlatformSavedContext, system_context};
