pub mod rhi_error;

#[path = "types/types.rs"]
pub mod types;
#[path = "api/api.rs"]
pub mod api;

use rhi_error::Result as Result;
//use rhi_error::RHIError as RHIError;

use core::{
    ffi::CStr
};

#[derive(Default, Clone)]
pub struct AppInfo {
    pub app_name: &'static CStr,
    pub engine_name: &'static CStr,
    pub with_validation_layers: bool,
}

pub trait RHI<B: Sized, T: Sized> {
    fn init(builder: &B) -> Result<T>;
    fn post_init(&mut self);
    fn shutdown(&mut self);
}
