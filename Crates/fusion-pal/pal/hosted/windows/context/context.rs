//! Windows fusion-pal user-space context backend selection.
//!
//! Native support is currently provided on `x86_64` and `aarch64` Windows user space. Other
//! Windows ISAs stay explicitly unsupported rather than inheriting a fake coroutine story.

#[cfg(target_arch = "aarch64")]
mod aarch64;
#[cfg(target_arch = "x86_64")]
mod x86_64;

#[cfg(target_arch = "aarch64")]
use aarch64 as implementation;
#[cfg(target_arch = "x86_64")]
use x86_64 as implementation;

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
mod implementation {
    use crate::contract::pal::runtime::context::{
        ContextSupport,
        UnsupportedContext,
        UnsupportedSavedContext,
    };

    /// Selected Windows context provider type.
    pub type PlatformContext = UnsupportedContext;
    /// Selected Windows saved-context type.
    pub type PlatformSavedContext = UnsupportedSavedContext;

    /// Returns the selected Windows context provider.
    #[must_use]
    pub const fn system_context() -> PlatformContext {
        PlatformContext::new()
    }

    /// Returns unsupported Windows context truth for unsupported Windows ISAs.
    #[must_use]
    pub const fn system_context_support() -> ContextSupport {
        ContextSupport::unsupported()
    }
}

pub use implementation::{
    PlatformContext,
    PlatformSavedContext,
    system_context,
    system_context_support,
};
