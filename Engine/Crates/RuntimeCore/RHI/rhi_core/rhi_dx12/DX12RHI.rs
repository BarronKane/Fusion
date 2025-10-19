use fusion_rhi_core::{
    AppInfo,
    RHI
};
use fusion_rhi_core::rhi_error::{
    RHIError,
    Result,
    RHIErrorEnum
};

#[allow(unused_imports)] // TODO: Remove this.
use windows::{
    core::*, Win32::Foundation::*, Win32::Graphics::Direct3D::Fxc::*, Win32::Graphics::Direct3D::*,
    Win32::Graphics::Direct3D12::*, Win32::Graphics::Dxgi::Common::*, Win32::Graphics::Dxgi::*,
    Win32::System::LibraryLoader::*, Win32::System::Threading::*,
    Win32::UI::WindowsAndMessaging::*,
};

use core::{
    ffi::{CStr},
    option::Option,
};

#[derive(Clone)]
pub struct DX12RHI<'d> {
    rhi_name: &'d str,
    dxgi_factory: Option<IDXGIFactory4>
}

impl<'d> Default for DX12RHI<'d> {
    fn default() -> Self {
        Self {
            rhi_name: "DX12",
            dxgi_factory: None
        }
    }
}

impl<'d> DX12RHI<'d> {
    pub fn new() -> DX12RHI<'d> {
        DX12RHI::default()
    }

    fn init_dx12(&self, app_info: &AppInfo) -> Result<'d, DX12RHI<'d>> {
        let dxgi_factory_flags = DXGI_CREATE_FACTORY_FLAGS(0); // TODO: Debug Assertions.

        let factory = unsafe {
            CreateDXGIFactory2(dxgi_factory_flags)
        };

        match factory {
            Ok(f) => {
                let mut initialized_dx12 = self.clone();
                initialized_dx12.dxgi_factory = Some(f);
                Ok(initialized_dx12)
            },
            Err(_) => {
                let error = RHIError {
                    rhi: self.rhi_name,
                    kind: &RHIErrorEnum::InitializationError,
                    message: "Failed to create dxgi_factory." // TODO: Add winapi message?
                };
                Result::Err(error)
            }
        }
    }
}

impl<'d> RHI<DX12RHI<'d>> for DX12RHI<'d> {
    fn init(&self, app_info: &AppInfo) -> Result<'d, Self> {
        let initialized_dx12: DX12RHI = self.init_dx12(app_info)?;
        Ok(initialized_dx12)
    }
    fn post_init(&mut self) {
        unimplemented!()
    }
    fn shutdown(&mut self) {
        unimplemented!()
    }
}
