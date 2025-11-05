use ash::khr::{surface, swapchain};
use crate::VulkanRHI;
// use fusion_rhi_core::api::adapter::Device;
use fusion_rhi_core::rhi_error::{RHIError, RHIErrorEnum, Result};

use ash::vk;
use ash::khr;
use ash::vk::Handle;

impl VulkanRHI {
    fn vk_create_swapchain(instance: &ash::Instance, physical_device: &vk::PhysicalDevice, device: &ash::Device, old_swapchain: Option<vk::SwapchainKHR>, surface_instance: &khr::surface::Instance, surface: &vk::SurfaceKHR) -> Result<vk::SwapchainKHR> {
        let old_swapchain = old_swapchain.unwrap_or(vk::SwapchainKHR::null());

        let swap_chain_support_result = unsafe { surface_instance.get_physical_device_surface_capabilities(*physical_device, *surface) };

        // TODO: Strip vec for no_std.
        // TODO: Get SurfaceCapabilitiesKHR2.
        let details: vk::SurfaceCapabilitiesKHR;
        match swap_chain_support_result {
            Ok(s) => {
                details = s;
            },
            Err(e) => {
                return Err(RHIError::new(
                    "Vulkan",
                    RHIErrorEnum::LogicalDeviceError,
                    "Could not query swap chain support."
                ));
            }
        }

        // TODO: Get from surface window.
        let new_extent = {
            if details.current_extent.width != u32::MAX {
                details.current_extent
            } else {
                vk::Extent2D {
                    width: 1920,
                    height: 1080,
                }
            }
        };

        let mut image_count = details.min_image_count + 1;
        if details.max_image_count > 0 && image_count > details.max_image_count {
            image_count = details.max_image_count;
        }


        // TODO: strip vec for no_std.
        let formats_result = unsafe { surface_instance.get_physical_device_surface_formats(*physical_device, *surface) };
        let formats: Vec<vk::SurfaceFormatKHR>;
        match formats_result {
            Ok(f) => {
                formats = f;
            },
            Err(e) => {
                return Err(RHIError::new(
                    "Vulkan",
                    RHIErrorEnum::LogicalDeviceError,
                    "Could not query swap chain formats."
                ));
            }
        }

        // choose format
        let format_result = formats.iter().find(|f| f.format == vk::Format::B8G8R8A8_SRGB);
        let format;
        match format_result {
            Some(f) => {
                format = f.format;
            },
            None => {
                return Err(RHIError::new(
                    "Vulkan",
                    RHIErrorEnum::LogicalDeviceError,
                    "Could not find a suitable swap chain format."
                ));
            }
        }

        let presents_result = unsafe { surface_instance.get_physical_device_surface_present_modes(*physical_device, *surface) };
        let presents: Vec<vk::PresentModeKHR>;
        match presents_result {
            Ok(p) => {
                presents = p;
            },
            Err(e) => {
                return Err(RHIError::new(
                    "Vulkan",
                    RHIErrorEnum::LogicalDeviceError,
                    "Could not query swap chain present modes."
                ));
            }
        }

        let present_mode_result = presents.iter().find(|p| *p == &vk::PresentModeKHR::MAILBOX);
        let present_mode;
        match present_mode_result {
            Some(p) => {
                present_mode = *p;
            },
            None => {
                return Err(RHIError::new(
                    "Vulkan",
                    RHIErrorEnum::LogicalDeviceError,
                    "Could not find a suitable swap chain present mode."
                ));
            }
        }



        let color_space = vk::ColorSpaceKHR::SRGB_NONLINEAR;

        let mut raw_flags = vk::SwapchainCreateFlagsKHR::empty();
        raw_flags = raw_flags | vk::SwapchainCreateFlagsKHR::MUTABLE_FORMAT;

        // TODO: Strip vec for no_std.
        let mut raw_view_formats: Vec<vk::Format> = vec![];

        let queue_families_result = VulkanRHI::vk_find_queue_families(instance, physical_device);
        let queue_families;
        match queue_families_result {
            Ok(q) => {
                queue_families = q;
            },
            Err(e) => {
                return Err(e);
            }
        }


        let mut info = vk::SwapchainCreateInfoKHR::default()
            .flags(raw_flags)
            .surface(*surface)
            .present_mode(present_mode)
            .image_format(format)
            .image_extent(new_extent)
            .image_color_space(color_space)
            .image_array_layers(1)
            .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT)
            //.queue_family_indices()
            .min_image_count(image_count)
            .old_swapchain(old_swapchain);

        let swatchain_loader = khr::swapchain::Device::new(instance, device);
        let swapchain = unsafe { swatchain_loader.create_swapchain(&info, None) };
        match swapchain {
            Ok(s) => {
                return Ok(s);
            },
            Err(e) => {
                return Err(RHIError::new(
                    "Vulkan",
                    RHIErrorEnum::LogicalDeviceError,
                    "Could not create swap chain."
                ));
            }
        }
    }

    fn create_image_views(image_views: &mut Vec<vk::ImageView>, swap_chain_images: &Vec<vk::Image>, device: &ash::Device) -> Result<()> {
        image_views.resize(swap_chain_images.len(), vk::ImageView::null());
        for i in 0..swap_chain_images.len() {
            let mut image_view_create_info = vk::ImageViewCreateInfo::default()
                .image(swap_chain_images[i])
                .view_type(vk::ImageViewType::TYPE_2D)
                // TODO: Get from swapchain.
                .format(vk::Format::B8G8R8A8_SRGB);

            image_view_create_info.components.r = vk::ComponentSwizzle::IDENTITY;
            image_view_create_info.components.g = vk::ComponentSwizzle::IDENTITY;
            image_view_create_info.components.b = vk::ComponentSwizzle::IDENTITY;
            image_view_create_info.components.a = vk::ComponentSwizzle::IDENTITY;

            image_view_create_info.subresource_range.aspect_mask = vk::ImageAspectFlags::COLOR;
            image_view_create_info.subresource_range.base_mip_level = 0;
            image_view_create_info.subresource_range.level_count = 1;
            image_view_create_info.subresource_range.base_array_layer = 0;
            image_view_create_info.subresource_range.layer_count = 1;

            let image_view = unsafe { device.create_image_view(&image_view_create_info, None) };
            match image_view {
                Ok(v) => {
                    image_views[i] = v;
                },
                Err(e) => {
                    // TODO: Replace with our logger.
                    println!("Failed to create image view: {}", e);
                    return Err(RHIError::new(
                        "Vulkan",
                        RHIErrorEnum::LogicalDeviceError,
                        "Could not create image view."
                    ));
                }
            }
        }

        Ok(())
    }

    fn vk_create_render_pass(device: &ash::Device) -> Result<vk::RenderPass> {
        let mut attachment_description: vk::AttachmentDescription2 = vk::AttachmentDescription2::default();
        // TODO: Get this from swapchain.
        attachment_description.format = vk::Format::B8G8R8A8_SRGB;
        attachment_description.samples = vk::SampleCountFlags::TYPE_1;
        attachment_description.load_op = vk::AttachmentLoadOp::CLEAR;
        attachment_description.store_op = vk::AttachmentStoreOp::STORE;
        attachment_description.stencil_load_op = vk::AttachmentLoadOp::DONT_CARE;
        attachment_description.stencil_store_op = vk::AttachmentStoreOp::DONT_CARE;
        attachment_description.initial_layout = vk::ImageLayout::UNDEFINED;
        attachment_description.final_layout = vk::ImageLayout::PRESENT_SRC_KHR;

        let mut attachment_reference: vk::AttachmentReference2 = vk::AttachmentReference2::default();
        attachment_reference.attachment = 0;
        attachment_reference.layout = vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL;

        let mut subpass_description: vk::SubpassDescription2 = vk::SubpassDescription2::default();
        subpass_description.pipeline_bind_point = vk::PipelineBindPoint::GRAPHICS;
        subpass_description.color_attachment_count = 1;
        subpass_description.p_color_attachments = &attachment_reference;

        let mut subpass_dependency: vk::SubpassDependency2 = vk::SubpassDependency2::default();
        subpass_dependency.src_subpass = vk::SUBPASS_EXTERNAL;
        subpass_dependency.dst_subpass = 0;
        subpass_dependency.src_stage_mask = vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT;
        subpass_dependency.src_access_mask = vk::AccessFlags::empty();
        subpass_dependency.dst_stage_mask = vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT;
        subpass_dependency.dst_access_mask = vk::AccessFlags::empty() | vk::AccessFlags::COLOR_ATTACHMENT_WRITE;

        let mut render_pass_info: vk::RenderPassCreateInfo2 = vk::RenderPassCreateInfo2::default();
        render_pass_info.attachment_count = 1;
        render_pass_info.p_attachments = &attachment_description;
        render_pass_info.subpass_count = 1;
        render_pass_info.p_subpasses = &subpass_description;
        render_pass_info.dependency_count = 1;
        render_pass_info.p_dependencies = &subpass_dependency;

        let render_pass = unsafe { device.create_render_pass2(&render_pass_info, None) };

        match render_pass {
            Ok(r) => {
                return Ok(r);
            },
            Err(e) => {
                return Err(RHIError::new(
                    "Vulkan",
                    RHIErrorEnum::LogicalDeviceError,
                    "Could not create render pass."
                ));
            }
        }
    }
}
