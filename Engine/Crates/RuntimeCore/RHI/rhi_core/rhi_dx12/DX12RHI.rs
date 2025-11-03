mod adapter;

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

pub struct DX12RHIBuilder {}

#[derive(Clone)]
pub struct DX12RHI {
    rhi_name: &'static str,
    dxgi_factory: Option<IDXGIFactory4>,
    adapter: Option<IDXGIAdapter1>
}

impl Default for DX12RHI {
    fn default() -> Self {
        Self {
            rhi_name: "DX12",
            dxgi_factory: None,
            adapter: None,
        }
    }
}

impl DX12RHI {
    fn try_get_dxgi_factory(&'_ self) -> Result<&IDXGIFactory4> {
        match &self.dxgi_factory {
            Some(f) => {
                return Ok(f);
            }
            None => {
                let error = RHIError {
                    rhi: self.rhi_name,
                    kind: RHIErrorEnum::InitializationError,
                    message: "DXGI Factory not initialized."
                };
                Err(error)
            }
        }
    }

    fn init_dx12(&self, app_info: &AppInfo) -> Result<DX12RHI> {
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
                    kind: RHIErrorEnum::InitializationError,
                    message: "Failed to create dxgi_factory." // TODO: Add winapi message?
                };
                Result::Err(error)
            }
        }
    }
}

impl RHI<DX12RHIBuilder, DX12RHI> for DX12RHI {
    fn init(builder: &DX12RHIBuilder) -> Result<Self> {
        unimplemented!()
    }
    fn post_init(&mut self) {
        unimplemented!()
    }
    fn shutdown(&mut self) {
        unimplemented!()
    }
}
