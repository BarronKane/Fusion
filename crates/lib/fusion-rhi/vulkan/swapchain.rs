use ash::extensions::khr::Surface;
use ash::vk;

pub struct SwapchainSupportDetails {
    pub capabilities: vk::SurfaceCapabilitiesKHR,
    pub formats: Vec<vk::SurfaceFormatKHR>,
    pub present_modes: Vec<vk::PresentModeKHR>,
}

impl SwapchainSupportDetails {
    pub fn new(gpu: vk::PhysicalDevice, surface: &Surface, surface_khr: vk::SurfaceKHR) -> Self {
        let capabilities = unsafe {
            surface
                .get_physical_device_surface_capabilities(gpu, surface_khr)
                .unwrap()
        };

        let formats = unsafe {
            surface
                .get_physical_device_surface_formats(gpu, surface_khr)
                .unwrap()
        };

        let present_modes = unsafe {
            surface
                .get_physical_device_surface_present_modes(gpu, surface_khr)
                .unwrap()
        };

        Self {
            capabilities,
            formats,
            present_modes,
        }
    }
}
