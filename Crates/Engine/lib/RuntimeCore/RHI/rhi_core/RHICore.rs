pub mod rhi_error;

#[path = "api/api.rs"]
pub mod api;
#[path = "types/types.rs"]
pub mod types;

use rhi_error::Result;
//use rhi_error::RHIError as RHIError;

use core::ffi::CStr;

#[derive(Default, Clone)]
pub struct AppInfo {
    pub app_name: &'static CStr,
    pub engine_name: &'static CStr,
    pub with_validation_layers: bool,
}

pub trait RHI<B: Sized, T: Sized> {
    /// Initializes the backend with the given builder configuration.
    ///
    /// # Errors
    ///
    /// Returns an error when backend initialization fails.
    fn init(builder: &B) -> Result<T>;
    fn post_init(&mut self);
    fn shutdown(&mut self);
}
