use crate::vulkan::VulkanApp;

use ash::{
    Instance,
    Device,
    vk, extensions::khr::GetPhysicalDeviceProperties2,
};

use std::ptr;

pub struct VulkanDevice {
    device: Device,
    gpu: vk::PhysicalDevice,
    gpu_props: vk::PhysicalDeviceProperties2,
}

impl VulkanDevice {
    pub fn create_device(instance: &Instance) {
        

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

    pub fn get_gpu_props(instance: &Instance, device: &vk::PhysicalDevice) -> vk::PhysicalDeviceProperties2 {
        let mut physical_device_properties_2 = vk::PhysicalDeviceProperties2::default();
        physical_device_properties_2.s_type = vk::StructureType::PHYSICAL_DEVICE_PROPERTIES_2;

        unsafe {
            instance.get_physical_device_properties2(*device, &mut physical_device_properties_2)
        };

        return physical_device_properties_2;
    }
}
