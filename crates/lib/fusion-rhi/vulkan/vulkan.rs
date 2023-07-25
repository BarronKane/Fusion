pub mod context;
pub mod debug;
pub mod device;

use crate::App;

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
}

impl App for VulkanApp {
    fn new(window: &Window) -> Self {
        let entry = Entry::linked();

        let instance = Self::create_instance(&entry, &window, true);

        let surface = Surface::new(&entry, &instance);
        let surface_khr = unsafe {
            ash_window::create_surface(&entry, &instance, window.raw_display_handle(), window.raw_window_handle(), None)
        };

        let debug_callback = VulkanApp::setup_debug_messenger(true, &entry, &instance);

        
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
}

impl Drop for VulkanApp {
    fn drop(&mut self) {
        
    }
}
