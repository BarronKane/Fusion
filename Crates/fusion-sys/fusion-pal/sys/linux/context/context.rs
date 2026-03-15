//! Linux fusion-pal user-space context backend selection.
//!
//! Native assembly is used where the ISA backend exists. The older `ucontext` path remains as
//! an emulated fallback on supported Linux targets until their native backends land.

#[cfg(target_arch = "x86_64")]
#[path = "x86_64.rs"]
mod implementation;

#[cfg(all(not(target_arch = "x86_64"), target_arch = "aarch64"))]
#[path = "ucontext.rs"]
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
