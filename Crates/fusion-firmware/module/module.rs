//! Runtime-loadable Fusion driver module ABI.

use fusion_hal::contract::drivers::driver::DriverMetadata;
#[cfg(test)]
use fusion_hal::contract::drivers::driver::{
    DriverBindingSource,
    DriverClass,
    DriverContractKey,
    DriverIdentity,
};

include!("../../fusion-hal/fdxe/shared.rs");

#[cfg(target_os = "none")]
unsafe extern "C" {
    static __fusion_fdxe_modules_start: u8;
    static __fusion_fdxe_modules_end: u8;
}

/// Registers all statically embedded driver modules from the firmware image.
///
/// # Errors
///
/// Returns an error if the linker section layout is invalid or any embedded module fails normal
/// FDXE validation.
#[cfg(target_os = "none")]
pub fn register_static_modules(registry: &mut FdxeRegistry<'_>) -> Result<(), FdxeModuleError> {
    let modules = static_modules()?;
    registry.register_static_modules(modules)
}

/// Returns the linker-collected slice of statically embedded module records.
///
/// # Errors
///
/// Returns an error if the linker section bounds are not aligned to the embedded-record layout.
#[cfg(target_os = "none")]
pub fn static_modules() -> Result<&'static [FdxeStaticModuleV1], FdxeModuleError> {
    let start = core::ptr::addr_of!(__fusion_fdxe_modules_start) as usize;
    let end = core::ptr::addr_of!(__fusion_fdxe_modules_end) as usize;

    if end < start {
        return Err(FdxeModuleError::layout_mismatch());
    }

    let record_size = core::mem::size_of::<FdxeStaticModuleV1>();
    if record_size == 0 {
        return Err(FdxeModuleError::layout_mismatch());
    }

    let byte_len = end - start;
    if byte_len % record_size != 0 {
        return Err(FdxeModuleError::layout_mismatch());
    }

    let align = core::mem::align_of::<FdxeStaticModuleV1>();
    if start % align != 0 {
        return Err(FdxeModuleError::layout_mismatch());
    }

    let count = byte_len / record_size;
    // SAFETY: the linker script defines a contiguous section of `FdxeStaticModuleV1` records.
    let modules = unsafe { core::slice::from_raw_parts(start as *const FdxeStaticModuleV1, count) };
    Ok(modules)
}
