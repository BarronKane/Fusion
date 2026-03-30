//! macOS fusion-pal user-space context backend selection.
//!
//! Native support is provided on `x86_64` and `aarch64` with ISA-specific assembly
//! save/restore paths. Other macOS ISAs remain explicitly unsupported.

#[cfg(target_arch = "x86_64")]
#[path = "x86_64.rs"]
mod implementation;

#[cfg(target_arch = "aarch64")]
#[path = "aarch64.rs"]
mod implementation;

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
mod implementation {
    use crate::contract::pal::runtime::context::{
        ContextSupport,
        UnsupportedContext,
        UnsupportedSavedContext,
    };

    /// Selected macOS context provider type.
    pub type PlatformContext = UnsupportedContext;
    /// Selected macOS saved-context type.
    pub type PlatformSavedContext = UnsupportedSavedContext;

    /// Returns the selected macOS context provider.
    #[must_use]
    pub const fn system_context() -> PlatformContext {
        PlatformContext::new()
    }

    /// Returns unsupported macOS context truth for unsupported macOS ISAs.
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
