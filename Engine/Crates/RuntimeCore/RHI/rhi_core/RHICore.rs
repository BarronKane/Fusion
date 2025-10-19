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

pub struct AppInfo<'n> {
    pub app_name: &'n CStr,
    pub engine_name: &'n CStr,
}

pub trait RHI<T: Sized> {
    fn init(&self, app_info: &AppInfo) -> Result<'_, T>;
    fn post_init(&mut self);
    fn shutdown(&mut self);
}
