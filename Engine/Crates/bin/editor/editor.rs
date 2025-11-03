use fusion_rhi as rhi;
use fusion_rhi_core as rhi_core;
use fusion_rhi_core::RHI;
use fusion_rhi_vulkan as rhi_vulkan;

fn main() {
    println!("Hello, world!");
    
    let mut app_info = rhi_core::AppInfo::default();
    app_info.engine_name = c"Fusion";
    app_info.app_name = c"Fusion Editor";
    app_info.with_validation_layers = true;
    
    let vulkan_builder = rhi_vulkan::VulkanRHIBuilder::new()
        .app_info(app_info)
        .with_validation_layers();
    
    let vulkan_rhi = rhi_vulkan::VulkanRHI::init(&vulkan_builder);
    match vulkan_rhi {
        Ok(r) => {
            println!("RHI Initialized.");
        },
        Err(e) => {
            println!("RHI Failed to initialize. {}", e);
        }
    }
}