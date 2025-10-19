use fusion_rhi_core::{
    AppInfo, RHI
};
#[cfg(not(target_vendor = "apple"))]
pub use fusion_rhi_vulkan::VulkanRHI;

#[cfg(target_os = "windows")]
pub use fusion_rhi_dx12::DX12RHI;

#[cfg(target_vendor = "apple")]
pub use fusion_rhi_metal::MetalRHI;

pub enum TargetRenderingAPI<'v, 'd> {
    #[cfg(not(target_vendor = "apple"))]
    Vulkan(VulkanRHI<'v>),

    #[cfg(target_os = "windows")]
    DX12(DX12RHI<'d>),

    #[cfg(target_vendor = "apple")]
    Metal(MetalRHI<'m>)
}
