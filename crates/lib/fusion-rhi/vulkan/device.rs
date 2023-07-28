use crate::vulkan::VulkanApp;

use ash::{
    extensions::khr::Surface,
    Instance,
    Device,
    vk::{self, DeviceQueueCreateFlags},
};

use std::ptr;

pub struct VulkanDevice {
    pub device: Device,
    pub gpu: vk::PhysicalDevice,
    pub gpu_props: vk::PhysicalDeviceProperties,
}

impl VulkanDevice {
    pub fn create_device(instance: &Instance, surface: &Surface, surface_khr: vk::SurfaceKHR, gpu: vk::PhysicalDevice) -> Self {
        let (graphics_family_index, present_family_index) = Self::find_queue_families(instance, surface, surface_khr, &gpu);
        let (graphics_family_index, present_family_index) = (graphics_family_index.unwrap(), present_family_index.unwrap());
        let queue_priorities = [1.0f32];
        
        let queue_create_infos = {
            let mut indices = vec![graphics_family_index, present_family_index];
            indices.dedup();

            indices
                .iter()
                .map(|i| {
                    vk::DeviceQueueCreateInfo {
                        s_type: vk::StructureType::DEVICE_QUEUE_CREATE_INFO,
                        p_next: ptr::null(),
                        flags: DeviceQueueCreateFlags::empty(),
                        queue_family_index: *i,
                        queue_count: u32::default(),
                        p_queue_priorities: queue_priorities.as_ptr(),
                    }
                })
                .collect::<Vec<_>>()
        };

        let device_extensions = Self::get_required_extensions();
        let device_extensions_ptrs = device_extensions
            .iter()
            .map(|ext| ext.as_ptr())
            .collect::<Vec<_>>();

        // TODO: Check version support.
        // TODO: Full feature suite.
        let mut physical_device_features_2 = vk::PhysicalDeviceFeatures2::default();
        unsafe {
            instance.get_physical_device_features2(gpu, &mut physical_device_features_2)
        };
        let mut physical_device_features = physical_device_features_2.features;
        physical_device_features.sampler_anisotropy = true as u32;

        #[allow(deprecated)]
        let mut device_info = vk::DeviceCreateInfo {
            s_type: vk::StructureType::DEVICE_CREATE_INFO,
            p_next: ptr::null(),
            flags: vk::DeviceCreateFlags::empty(),
            queue_create_info_count: queue_create_infos.len() as u32,
            p_queue_create_infos: queue_create_infos.as_ptr(),

            // Depricated.
            enabled_layer_count: 0,
            // Depricated.
            pp_enabled_layer_names: ptr::null(),

            enabled_extension_count: device_extensions_ptrs.len() as u32,
            pp_enabled_extension_names: device_extensions_ptrs.as_ptr(),
            p_enabled_features: &physical_device_features,
        };        

        let device = unsafe {
            instance
                .create_device(gpu, &device_info, None)
                .expect("Failed to create logical device.")
        };

        let gpu_props = Self::get_gpu_props(instance, &gpu);

        return VulkanDevice { 
            device: device, 
            gpu: gpu, 
            gpu_props
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
        let required_extentions = Self::get_required_extensions();
        
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

    pub fn get_required_extensions() -> [&'static std::ffi::CStr; 1] {
        [ash::extensions::khr::Swapchain::name()]
    }
}
