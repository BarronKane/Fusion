use fusion_rhi_core as rhi_core;
use fusion_rhi_core::RHI;
use fusion_rhi_vulkan as rhi_vulkan;

fn main() {
    println!("Hello, world!");

    let app_info = rhi_core::AppInfo {
        engine_name: c"Fusion",
        app_name: c"Fusion Editor",
        with_validation_layers: true,
    };

    let vulkan_builder = rhi_vulkan::VulkanRHIBuilder::new()
        .app_info(app_info)
        .with_validation_layers();

    let vulkan_rhi = rhi_vulkan::VulkanRHI::init(&vulkan_builder);
    match vulkan_rhi {
        Ok(_r) => {
            println!("RHI Initialized.");
        }
        Err(e) => {
            println!("RHI Failed to initialize. {e}");
        }
    }
}
