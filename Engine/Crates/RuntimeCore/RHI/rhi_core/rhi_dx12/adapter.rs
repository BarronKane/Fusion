use fusion_rhi_core::api::adapter::Adapter;
use fusion_rhi_core::rhi_error::{
    Result,
    RHIErrorEnum,
    RHIError
};

#[allow(unused_imports)] // TODO: Remove this.
use windows::{
    core::*, Win32::Foundation::*, Win32::Graphics::Direct3D::Fxc::*, Win32::Graphics::Direct3D::*,
    Win32::Graphics::Direct3D12::*, Win32::Graphics::Dxgi::Common::*, Win32::Graphics::Dxgi::*,
    Win32::System::LibraryLoader::*, Win32::System::Threading::*,
    Win32::UI::WindowsAndMessaging::*,
};

use crate::DX12RHI;



impl DX12RHI {
    fn is_device_suitable(&self, device: &IDXGIAdapter1) -> bool {
        let desc_result = unsafe { device.GetDesc1() };
        let desc = match desc_result {
            Ok(d) => {
                d
            },
            Err(_) => {
                return false;
            }
        };

        // Checks for Dx12 support.
        if unsafe {
            D3D12CreateDevice(
                device,
                D3D_FEATURE_LEVEL_11_0,
                core::ptr::null_mut::<Option<ID3D12Device>>(),
            )
        }.is_err()
        {
            return false;
        }

        if (DXGI_ADAPTER_FLAG(desc.Flags as _) & DXGI_ADAPTER_FLAG_SOFTWARE)
            == DXGI_ADAPTER_FLAG_NONE {
            return true;
        }

        true
    }

    fn pick_physical_device(&mut self) -> Result<()> {
        let factory4 = self.try_get_dxgi_factory()?;
        // TODO: Get rid of Vec.
        let mut devices: Vec<IDXGIAdapter1> = Vec::new();

        unsafe {
            for i in 0.. {
                let device_result = factory4.EnumAdapters1(i);

                let device = match device_result {
                    Ok(d) => {
                        d
                    },
                    Err(_) => {
                        break;
                    }
                };

                devices.push(device);
            }
        }

        if devices.len() == 0 {
            return Err(RHIError {
                rhi: self.rhi_name,
                kind: RHIErrorEnum::InitializationError,
                message: "No physical devices found."
            });
        }

        let mut examined_devices: Vec<IDXGIAdapter1> = Vec::new();
        for device in devices {
            if self.is_device_suitable(&device) {
                examined_devices.push(device);
            }
        }

        if examined_devices.len() == 0 {
            return Err(RHIError {
                rhi: self.rhi_name,
                kind: RHIErrorEnum::InitializationError,
                message: "No suitable physical devices found."
            });
        }

        // TODO: Pick the best device.
        self.adapter = Some(examined_devices[0].clone());

        Ok(())
    }
}

impl Adapter for DX12RHI {

}


