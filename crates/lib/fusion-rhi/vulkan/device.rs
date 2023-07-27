use crate::vulkan::VulkanApp;

use ash::{
    Instance,
    Device,
    vk,
};

use std::ptr;

pub struct VulkanDevice {
    device: Device,
    gpu: vk::PhysicalDevice,
    gpu_props: vk::PhysicalDeviceProperties,
}

impl VulkanDevice {
    pub fn create_device(instance: &Instance) {
       
        #[allow(deprecated)]
        let device_info = vk::DeviceCreateInfo {
            s_type: vk::StructureType::DEVICE_CREATE_INFO,
            p_next: ptr::null(),
            flags: vk::DeviceCreateFlags::empty(),
            queue_create_info_count: todo!(),
            p_queue_create_infos: todo!(),
            enabled_layer_count: 0,
            pp_enabled_layer_names: ptr::null(),
            enabled_extension_count: todo!(),
            pp_enabled_extension_names: todo!(),
            p_enabled_features: todo!(),
        };
    }

    pub fn get_gpu_props(instance: &Instance, device: &vk::PhysicalDevice) -> vk::PhysicalDeviceProperties {
        let mut physical_device_properties_2 = vk::PhysicalDeviceProperties2::default();
        physical_device_properties_2.s_type = vk::StructureType::PHYSICAL_DEVICE_PROPERTIES_2;

        unsafe {
            instance.get_physical_device_properties2(*device, &mut physical_device_properties_2)
        };

        return physical_device_properties_2.properties;
    }

    pub fn find_queue_families(
        instance: &Instance,
        surface: &ash::extensions::khr::Surface,
        surface_khr: vk::SurfaceKHR,
        gpu: &vk::PhysicalDevice,
    ) -> (Option<u32>, Option<u32>) {
        let mut graphics = None;
        let mut present = None;

        let props = unsafe {
            instance.get_physical_device_queue_family_properties(*gpu)
        };

        for (index, family) in props.iter().filter(|f| f.queue_count > 0).enumerate() {
            let index = index as u32;

            if family.queue_flags.contains(vk::QueueFlags::GRAPHICS) && graphics.is_none() {
                graphics = Some(index);
            }

            let present_support = unsafe {
                surface
                    .get_physical_device_surface_support(*gpu, index, surface_khr)
                    .unwrap()
            };
            if present_support && present.is_none() {
                present = Some(index);
            }

            if graphics.is_some() && present.is_some() {
                break;
            }
        }

        (graphics, present)
    }

    pub fn check_device_extension_support(instance: &Instance, gpu: &vk::PhysicalDevice) -> bool {
        let required_extentions: [&'static std::ffi::CStr; 1] = [ash::extensions::khr::Swapchain::name()];
        
        let extension_props = unsafe {
            instance
                .enumerate_device_extension_properties(*gpu)
                .unwrap()
        };

        for required in required_extentions.iter() {
            let found = extension_props.iter().any(|ext| {
                let name = unsafe { std::ffi::CStr::from_ptr(ext.extension_name.as_ptr()) };
                required == &name
            });

            if !found {
                return false;
            }
        }

        return true;
    }
}
