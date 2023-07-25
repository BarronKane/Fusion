use ash::{
    extensions::{
        ext::DebugUtils, 
        khr::Surface
    },
    vk, Device, Entry, Instance
};

pub struct VkContext {
    _entry: Entry,
    instance: Instance,
    debug_callback: Option<(DebugUtils, vk::DebugUtilsMessengerEXT)>,
    surface: Surface,
    surface_khr: vk::SurfaceKHR,
    physical_device: vk::PhysicalDevice,
    device: Device,
}

impl VkContext {
    pub fn new(
        entry: Entry,
        instance: Instance,
        debug_callback: Option<(DebugUtils, vk::DebugUtilsMessengerEXT)>,
        surface: Surface,
        surface_khr: vk::SurfaceKHR,
        physical_device: vk::PhysicalDevice,
        device: Device,
    ) -> Self {
        VkContext {
            _entry: entry,
            instance,
            debug_callback,
            surface,
            surface_khr,
            physical_device,
            device,
        }
    }

    pub fn instance(&self) -> &Instance {
        &self.instance
    }

    pub fn surface(&self) -> &Surface {
        &self.surface
    }

    pub fn surface_khr(&self) -> vk::SurfaceKHR {
        self.surface_khr
    }

    pub fn physical_device(&self) -> vk::PhysicalDevice {
        self.physical_device
    }

    pub fn device(&self) -> &Device {
        &self.device
    }
}

impl Drop for VkContext {
    fn drop(&mut self) {
        unsafe {
            self.device.destroy_device(None);
            self.surface.destroy_surface(self.surface_khr, None);
            if let Some((utils, messenger)) = self.debug_callback.take() {
                utils.destroy_debug_utils_messenger(messenger, None);
            }
            self.instance.destroy_instance(None);
        }
    }
}
