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

use core::{
    option::Option,
    ffi::{CStr},
};

use ash;
use ash::vk;

#[derive(Clone)]
pub struct VulkanRHI<'v> {
    rhi_name: &'v str,
    instance: ash::Instance,
    //device: ash::Device,
}

impl<'v> VulkanRHI<'v> {
    fn create_instance(app_info: &AppInfo, entry: &ash::Entry) -> Result<'v, ash::Instance> {
        let vk_app_info: vk::ApplicationInfo = vk::ApplicationInfo::default()
            .application_name(app_info.app_name)
            .application_version(vk::make_api_version(0, 0, 1, 0)) // TODO: automate
            .engine_name(app_info.engine_name)
            .engine_version(vk::make_api_version(0, 0, 1, 0)) // TODO: automate
            .api_version(vk::make_api_version(0, 1, 3, 0)); // TODO: While local, also automate, maybe?

        let vk_create_info: vk::InstanceCreateInfo = vk::InstanceCreateInfo::default()
            .application_info(&vk_app_info);

        let instance = unsafe {
            entry.create_instance(&vk_create_info, None)
        };

        match instance {
            Ok(i) => {
                Ok(i)
            },
            Err(e) => {
                let error = RHIError {
                    rhi: "Vulkan",
                    kind: &RHIErrorEnum::InitializationError,
                    message: "Failed to create Vulkan Instance." // TODO: Is there a vulkan message?
                };
                Result::Err(error)
            }
        }
    }

    fn init_vulkan(app_info: &AppInfo) -> Result<'v, Self> {
        let entry = ash::Entry::linked();
        let vk_instance = Self::create_instance(app_info, &entry)?;

        //let device = VulkanRHI::pick_physical_device()

        let mut initialized_vulkan = VulkanRHI {
            rhi_name: "Vulkan",
            instance: vk_instance,

        };

        Ok(initialized_vulkan)
    }
}

impl<'v> RHI<VulkanRHI<'v>> for VulkanRHI<'v> {
    fn init(&self, app_info: &AppInfo) -> Result<'v, Self> {
        let initialized_vulkan: VulkanRHI = VulkanRHI::init_vulkan(app_info)?;
        Ok(initialized_vulkan)
    }


    fn post_init(&mut self) {
        unimplemented!()
    }
    fn shutdown(&mut self) {
        unimplemented!()
    }
}
