pub mod context;
pub mod debug;
pub mod device;
pub mod swapchain;

use crate::App;

use tracing::{
    error,
    warn,
    info,
    debug,
};

use std::{
    ffi::{CStr, CString, c_char},
    mem::{align_of, size_of},
};

use ash::extensions::{
    ext::DebugUtils,
    khr::{Surface, Swapchain},
};
use ash::{
    vk, Device, Entry, Instance
};

use raw_window_handle::{HasRawDisplayHandle, HasRawWindowHandle};
use winit::{
    dpi::PhysicalSize,
    event::{ElementState, Event, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::{Window, WindowBuilder},
};

use self::context::VkContext;

struct VulkanApp {
    b_enable_validation_layers: bool,

    //device: device::VulkanDevice,
}

impl App for VulkanApp {
    fn new(window: &Window) -> Self {
        let entry = Entry::linked();

        let instance = Self::create_instance(&entry, &window, true);

        let surface = Surface::new(&entry, &instance);
        let surface_khr = unsafe {
            ash_window::create_surface(&entry, &instance, window.raw_display_handle(), window.raw_window_handle(), None).unwrap()
        };

        let debug_callback = VulkanApp::setup_debug_messenger(true, &entry, &instance);

        let device = Self::select_device(&instance, &surface, surface_khr);
        
        /*
        let vk_context = VkContext::new(
            entry, 
            instance, 
            debug_callback, 
            surface, 
            surface_khr, 
            physical_device, 
            device
        );
        */

        let va = VulkanApp {
            b_enable_validation_layers: true,
        };

        return va;
    }    
}

impl VulkanApp {
    fn create_instance(entry: &Entry, window: &Window, validation: bool) -> Instance {
        let app_name = CString::new("Vulkan Application").unwrap();
        let engine_name = CString::new("No Engine").unwrap();
        let app_info = vk::ApplicationInfo::builder()
            .application_name(app_name.as_c_str())
            .application_version(vk::make_api_version(0, 0, 1, 0))
            .engine_name(engine_name.as_c_str())
            .engine_version(vk::make_api_version(0, 0, 1, 0))
            .api_version(vk::make_api_version(0, 1, 3, 0))
            .build();

        let extension_names = ash_window::enumerate_required_extensions(window.raw_display_handle())
            .expect("Unable to create vulkan instance: EXTENSIONS");
        let mut extension_names = extension_names
            .iter()
            .map(|ext| *ext)
            .collect::<Vec<_>>();
        if validation {
            extension_names.push(vk::ExtDebugUtilsFn::name().as_ptr());
        }

        let extension_names_final = extension_names.as_slice();

        let layer_names_ptrs = Self::get_layer_names_ptrs();

        let mut instance_create_info = vk::InstanceCreateInfo::builder()
            .application_info(&app_info)
            .enabled_extension_names(extension_names_final);
        if validation {
            Self::check_validation_layer_support(&entry);
            instance_create_info = instance_create_info.enabled_layer_names(&layer_names_ptrs);
        }

        unsafe {
            entry.create_instance(&instance_create_info, None).unwrap()
        }
    }

    fn select_device(instance: &Instance, surface: &Surface, surface_khr: vk::SurfaceKHR) -> device::VulkanDevice {
        let devices = unsafe {
            instance.enumerate_physical_devices().unwrap()
        };

        let mut other_gpus: Vec<device::VulkanDevice> = Vec::new();
        let mut integrated_gpus: Vec<device::VulkanDevice> = Vec::new();
        let mut discreet_gpus: Vec<device::VulkanDevice> = Vec::new();
        let mut virtual_gpus: Vec<device::VulkanDevice> = Vec::new();
        let mut cpu_gpus: Vec<device::VulkanDevice> = Vec::new();

        let _total_devices = devices
            .into_iter()
            .map(|gpu| {
                let device = device::VulkanDevice::create_device(instance, surface, surface_khr, gpu);
                match device.gpu_props.device_type {
                    vk::PhysicalDeviceType::OTHER => other_gpus.push(device),
                    vk::PhysicalDeviceType::INTEGRATED_GPU => integrated_gpus.push(device),
                    vk::PhysicalDeviceType::DISCRETE_GPU => discreet_gpus.push(device),
                    vk::PhysicalDeviceType::VIRTUAL_GPU => virtual_gpus.push(device),
                    vk::PhysicalDeviceType::CPU => cpu_gpus.push(device),
                    _ => other_gpus.push(device)
                }
            })
            .count();

        let mut device: Option<device::VulkanDevice> = None;

        // TODO: GPU Vendor
        if discreet_gpus.len() > 0 {
            for vulkan_device in discreet_gpus {
                if Self::is_device_suitable(instance, surface, surface_khr, &vulkan_device.gpu) {
                    device = Some(vulkan_device);
                    break;
                }
            }
        } else if integrated_gpus.len() > 0 {
            warn!("No discreet GPUs detected! Try updating your graphics driver?");
            info!("Looking for integrated gpu.");
            for vulkan_device in integrated_gpus {
                if Self::is_device_suitable(instance, surface, surface_khr, &vulkan_device.gpu) {
                    device = Some(vulkan_device);
                    break;
                }
            }            
        }

        if device.is_none() {
            error!("No viable GPU device detected! Try updating your graphics driver?")
        }

        if device.is_some() {
            return device.unwrap();
        } else {
            panic!("Panicing, no viable GPU found.")
        }
    }

    fn is_device_suitable(
        instance: &Instance, 
        surface: &Surface,
        surface_khr: vk::SurfaceKHR,
        gpu: &vk::PhysicalDevice
    ) -> bool {
        let (graphics, present) = device::VulkanDevice::find_queue_families(instance, surface, surface_khr, gpu);
        let extention_support = device::VulkanDevice::check_device_extension_support(instance, gpu);
        let is_swapchain_adequate = {
            let details = swapchain::SwapchainSupportDetails::new(*gpu, surface, surface_khr);
            !details.formats.is_empty() && !details.present_modes.is_empty()
        };
        let features = unsafe { instance.get_physical_device_features(*gpu) };
        
        return graphics.is_some()
            && present.is_some()
            && extention_support
            && is_swapchain_adequate
            && features.sampler_anisotropy == vk::TRUE;
    }
}

impl Drop for VulkanApp {
    fn drop(&mut self) {
        
    }
}
