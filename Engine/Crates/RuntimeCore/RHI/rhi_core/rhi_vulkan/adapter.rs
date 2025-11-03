use ash::vk;
use fusion_rhi_core::api::adapter::Adapter;
use fusion_rhi_core::rhi_error::{RHIError, RHIErrorEnum, Result};

use ash::vk::{Handle, PhysicalDevice, PhysicalDeviceFeatures, PhysicalDeviceFeatures2, PhysicalDeviceProperties2, QueueFamilyProperties2};

use crate::VulkanRHI;

impl VulkanRHI {
    pub(crate) fn vk_find_queue_families<'a>(instance: &ash::Instance, device: &PhysicalDevice) -> Result<Vec<QueueFamilyProperties2<'a>>> {
        // TODO: Get rid of Vec for no_std.
        let queue_families = unsafe {
            let num = unsafe { instance.get_physical_device_queue_family_properties2_len(*device) };

            if num == 0 {
                return Err(RHIError::new(
                    "Vulkan",
                    RHIErrorEnum::InitializationError,
                    "No queue families found."
                ))
            }

            let mut indices = vec![QueueFamilyProperties2::default(); num];
            instance.get_physical_device_queue_family_properties2(*device, &mut *indices);
            indices
        };

        Ok(queue_families)
    }

    pub(crate) fn vk_is_device_suitable(instance: &ash::Instance, device: &PhysicalDevice) -> Result<bool> {
        let mut device_properties = PhysicalDeviceProperties2::default();
        let mut device_features = PhysicalDeviceFeatures2::default();

        unsafe {
            instance.get_physical_device_properties2(*device, &mut device_properties);
            instance.get_physical_device_features2(*device, &mut device_features);
        }

        let queue_families = VulkanRHI::vk_find_queue_families(instance, device)?;
        let has_graphics_family = queue_families
            .iter()
            .any(|q| q.queue_family_properties.queue_flags.contains(vk::QueueFlags::GRAPHICS));



        if !has_graphics_family {
            return Ok(false);
        }

        Ok(device_properties.properties.device_type == vk::PhysicalDeviceType::DISCRETE_GPU &&
            device_features.features.geometry_shader == vk::TRUE)
    }

    pub(crate) fn vk_pick_physical_device(instance: &ash::Instance) -> Result<vk::PhysicalDevice> {
        // TODO: Get rid of Vec for no_std.
        let devices: Vec<PhysicalDevice>;

        unsafe {
            let e_devices = instance.enumerate_physical_devices();
            devices = match e_devices {
                Ok(d) => {
                    d
                },
                Err(e) => {
                    let error = RHIError {
                        rhi: "Vulkan",
                        kind: RHIErrorEnum::InitializationError,
                        message: "Failed to enumerate physical devices."
                    };
                    return Err(error);
                }
            };
        }

        if devices.len() == 0 {
            return Err(RHIError {
                rhi: "Instance",
                kind: RHIErrorEnum::InitializationError,
                message: "No physical devices found."
            });
        }

        let mut examined_devices: Vec<PhysicalDevice> = Vec::new();
        for device in devices {
            let result = VulkanRHI::vk_is_device_suitable(instance, &device);
            if result.is_ok() {
                if result.is_ok_and({|r| r == true }) {
                    examined_devices.push(device);
                }
            }
        }

        if examined_devices.len() == 0 {
            return Err(RHIError {
                rhi: "Vulkan",
                kind: RHIErrorEnum::InitializationError,
                message: "No suitable physical devices found."
            });
        }

        // TODO: Pick the best device.
        Ok(examined_devices[0])
    }

    pub(crate) fn vk_create_logical_device(instance: &ash::Instance, physical_device: &PhysicalDevice) -> Result<ash::Device> {

        let queue_families = VulkanRHI::vk_find_queue_families(instance, physical_device)?;

        let index = match queue_families
            .iter()
            .enumerate()
            .find(|(_, f)| {
                f.queue_family_properties.queue_flags.contains(vk::QueueFlags::GRAPHICS)
            })
            .map(|(i, _)| i as u32) {
            Some(i) => i,
            None => {
                return Err(RHIError::new(
                    "Vulkan",
                    RHIErrorEnum::InitializationError,
                    "No queue family with graphics support found."
                ));
            }

        };

        let mut vk_device_queue_create_info = vk::DeviceQueueCreateInfo::default();
        vk_device_queue_create_info.queue_family_index = index;
        vk_device_queue_create_info.queue_count = 1;
        vk_device_queue_create_info.p_queue_priorities = &1.0;

        let queues = vec![vk_device_queue_create_info];

        // TODO: pull this out of here.
        let device_features = PhysicalDeviceFeatures::default();
        let device_features2 = PhysicalDeviceFeatures2::default()
            .features(device_features);

        let device_create_info = vk::DeviceCreateInfo::default()
            .queue_create_infos(&queues)
            .enabled_features(&device_features2.features);

        let device_result = unsafe { instance.create_device(*physical_device, &device_create_info, None) };

        match device_result {
            Ok(d) => {
                Ok(d)
            },
            Err(e) => {
                Err(RHIError::new(
                    "Vulkan",
                    RHIErrorEnum::InitializationError,
                    "Failed to create logical device."
                ))
            }
        }
    }
}

impl Adapter for VulkanRHI {

}
