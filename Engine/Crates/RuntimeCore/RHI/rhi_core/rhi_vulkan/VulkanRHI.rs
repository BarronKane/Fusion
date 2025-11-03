mod adapter;
mod callbacks;

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
use ash::ext;
use ash::prelude::VkResult;

#[derive(Clone)]
pub struct VulkanRHI {
    rhi_name: &'static str,
    instance: ash::Instance,
    entry: ash::Entry,
    // TODO: Strip vec for no_std.
    validation_layers: Vec<*const core::ffi::c_char>,
    debug_messenger: Option<vk::DebugUtilsMessengerEXT>,
    physical_device: vk::PhysicalDevice,
    device: ash::Device,
}

#[derive(Clone)]
pub struct VulkanRHIBuilder {
    rhi_name: &'static str,
    app_info: AppInfo,
    with_validation_layers: bool,

    // Vulkan Stuff
    validation_layers: Vec<*const core::ffi::c_char>,
    // Vulkan Stuff

    inner: Option<VulkanRHI>,
}

impl Default for VulkanRHIBuilder {
    fn default() -> Self {
        VulkanRHIBuilder {
            rhi_name: "Vulkan",
            app_info: AppInfo::default(),
            with_validation_layers: false,

            // Vulkan Stuff
            validation_layers: Vec::new(),
            // Vulkan Stuff

            inner: None
        }
    }
}

impl VulkanRHIBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn app_info(mut self, app_info: AppInfo) -> Self {
        self.app_info = app_info;
        self
    }

    pub fn with_validation_layers(mut self) -> Self {
        self.with_validation_layers = true;
        self
    }

    pub(crate) fn vk_create_instance(app_info: &AppInfo, entry: &ash::Entry, with_validation: bool) -> Result<ash::Instance> {
        let vk_app_info: vk::ApplicationInfo = vk::ApplicationInfo::default()
            .application_name(app_info.app_name)
            .application_version(vk::make_api_version(0, 0, 1, 0)) // TODO: automate
            .engine_name(app_info.engine_name)
            .engine_version(vk::make_api_version(0, 0, 1, 0)) // TODO: automate
            .api_version(vk::make_api_version(0, 1, 3, 0)); // TODO: While local, also automate, maybe?

        let mut vk_create_info: vk::InstanceCreateInfo = vk::InstanceCreateInfo::default()
            .application_info(&vk_app_info);

        // Store the validation layers to extend their lifetime
        let validation_layers;
        if with_validation {
            let validation_layers_result = VulkanRHIBuilder::create_validation_layers();
            match validation_layers_result {
                Ok(v) => {
                    validation_layers = v;
                    vk_create_info = vk_create_info.enabled_layer_names(&validation_layers.as_slice());
                }
                Err(e) => {
                    println!("Failed to create debug messenger: {}", e);
                }
            }
        }

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
                    kind: RHIErrorEnum::InitializationError,
                    message: "Failed to create Vulkan Instance." // TODO: Is there a vulkan message?
                };
                Err(error)
            }
        }
    }

    pub(crate) fn vk_check_validation_layer_support(entry: &ash::Entry, layers: Vec<*const core::ffi::c_char>) -> Result<bool> {
        let layer_props_result = unsafe { entry.enumerate_instance_layer_properties() };
        match layer_props_result {
            Ok(lp) => {
                for layer in layers.iter() {
                    let found = lp.iter().any(|l| l.layer_name.as_ptr() == *layer);
                    if !found {
                        return Ok(false);
                    }
                }

                Ok(true)
            }
            Err(e) => {
                Err(RHIError::new(
                    "Vulkan",
                    RHIErrorEnum::InitializationError,
                    "Failed to enumerate instance layer properties."
                ))
            }
        }
    }

    fn set_debug_severity(error: bool, warning: bool, info: bool, verbose: bool) -> vk::DebugUtilsMessageSeverityFlagsEXT {
        let mut severity = vk::DebugUtilsMessageSeverityFlagsEXT::empty();

        if error { severity |= vk::DebugUtilsMessageSeverityFlagsEXT::ERROR; }
        if warning { severity |= vk::DebugUtilsMessageSeverityFlagsEXT::WARNING; }
        if info { severity |= vk::DebugUtilsMessageSeverityFlagsEXT::INFO; }
        if verbose { severity |= vk::DebugUtilsMessageSeverityFlagsEXT::VERBOSE; }

        severity
    }

    fn set_debug_type(general: bool, validation: bool, performance: bool) -> vk::DebugUtilsMessageTypeFlagsEXT {
        let mut debug_type = vk::DebugUtilsMessageTypeFlagsEXT::empty();

        if general { debug_type |= vk::DebugUtilsMessageTypeFlagsEXT::GENERAL; }
        if validation { debug_type |= vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION; }
        if performance { debug_type |= vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE; }

        debug_type
    }

    fn create_debug_info(entry: &ash::Entry, instance: &ash::Instance) -> Result<vk::DebugUtilsMessengerEXT> {
        let debug_info = vk::DebugUtilsMessengerCreateInfoEXT::default()
            .message_severity(VulkanRHIBuilder::set_debug_severity(true, true, true, true))
            .message_type(VulkanRHIBuilder::set_debug_type(true, true, true))
            .pfn_user_callback(Some(callbacks::vulkan_debug_callback));

        let debug_utils_loader = ext::debug_utils::Instance::new(entry, instance);
        let debug_call_back_result =  unsafe { debug_utils_loader
            .create_debug_utils_messenger(&debug_info, None) };

        match debug_call_back_result {
            Ok(d) => {
                Ok(d)
            }
            Err(e) => {
                Err(RHIError::new(
                    "Vulkan",
                    RHIErrorEnum::InitializationError,
                    "Failed to create debug messenger."
                ))
            }
        }
    }

    pub(crate) fn create_validation_layers() -> Result<Vec<*const core::ffi::c_char>> {
        // TODO: replace vec for no_std.
        // TODO: Pull this out.
        //let layer_names = ["VK_LAYER_KHRONOS_validation"];
        let layer_names_raw: Result<Vec<*const core::ffi::c_char>> = {
            let first = std::ffi::CString::new("VK_LAYER_KHRONOS_validation");
            match first {
                Ok(f) => {
                    Ok(vec![f.as_ptr()])
                }
                Err(e) => {
                    Err(RHIError::new(
                        "Vulkan",
                        RHIErrorEnum::InitializationError,
                        "Failed to create debug messenger."
                    ))
                }
            }
        };

         layer_names_raw
    }
}

impl RHI<VulkanRHIBuilder, VulkanRHI> for VulkanRHI {
    fn init(builder: &VulkanRHIBuilder) -> Result<Self> {
        // TODO: Replaced with ash::Entry::Load().
        let entry = ash::Entry::linked();

        let with_validation_result = VulkanRHIBuilder::vk_check_validation_layer_support(&entry, VulkanRHIBuilder::create_validation_layers()?);
        let with_validation = with_validation_result.unwrap_or_else(|e| {
            println!("Failed to create debug messenger: {}", e);
            false
        });

        let instance = VulkanRHIBuilder::vk_create_instance(&builder.app_info, &entry, with_validation)?;

        let mut debug_info: Option<vk::DebugUtilsMessengerEXT> = None;
        if with_validation {
            debug_info = Some(VulkanRHIBuilder::create_debug_info(&entry, &instance)?);
        }

        let physical_device = VulkanRHI::vk_pick_physical_device(&instance)?;
        let device = VulkanRHI::vk_create_logical_device(&instance, &physical_device)?;

        Ok(VulkanRHI {
            rhi_name: "Vulkan",
            instance,
            entry,
            validation_layers: builder.validation_layers.clone(),
            debug_messenger: debug_info,
            physical_device,
            device,
        })
    }

    fn post_init(&mut self) {
        unimplemented!()
    }
    fn shutdown(&mut self) {
        unimplemented!()
    }
}

impl Drop for VulkanRHIBuilder {
    fn drop(&mut self) {
        // TODO
    }
}
