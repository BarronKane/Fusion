use ash::vk;
use fusion_rhi_core::api::adapter::Adapter;
use fusion_rhi_core::rhi_error::{RHIError, RHIErrorEnum, Result};

use ash::vk::{Handle, PhysicalDevice, PhysicalDeviceFeatures2, PhysicalDeviceProperties2};

use crate::VulkanRHI;

impl<'v> VulkanRHI<'v> {
    fn is_device_suitable(&self, device: PhysicalDevice) -> Result<bool> {
        let mut device_properties = PhysicalDeviceProperties2::default();
        let mut device_features = PhysicalDeviceFeatures2::default();
        
        unsafe {
            self.instance.get_physical_device_properties2(device, &mut device_properties);
            self.instance.get_physical_device_features2(device, &mut device_features);
        }

        Ok(device_properties.properties.device_type == vk::PhysicalDeviceType::DISCRETE_GPU &&
            device_features.features.geometry_shader == vk::TRUE)
    }
}

impl<'v> Adapter for VulkanRHI<'v> {
    fn pick_physical_device(&self) -> Result<'_, ()> {
        let mut devices: Vec<PhysicalDevice>;
        
        unsafe {
            let e_devices = self.instance.enumerate_physical_devices();
            devices = match e_devices {
                Ok(d) => {
                    d
                },
                Err(e) => {
                    let error = RHIError {
                        rhi: "Vulkan",
                        kind: &RHIErrorEnum::InitializationError,
                        message: "Failed to enumerate physical devices."
                    };
                    return Err(error);
                }
            };
        }
        
        if devices.len() == 0 {
            return Err(RHIError {
                rhi: "Vulkan",
                kind: &RHIErrorEnum::InitializationError,
                message: "No physical devices found."
            });
        }
        
        let mut examined_devices: Vec<PhysicalDevice> = Vec::new();
        for device in devices {
            if self.is_device_suitable(device)? {
                examined_devices.push(device);
            }
        }
        
        if examined_devices.len() == 0 {
            return Err(RHIError {
                rhi: "Vulkan",
                kind: &RHIErrorEnum::InitializationError,
                message: "No suitable physical devices found."
            });
        }
        
        // TODO: Pick the best device.
        
        Ok(())
    }
}
