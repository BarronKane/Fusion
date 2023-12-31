#[path = "./rhi/rhi.rs"]
pub mod rhi;

#[path = "./dx12/dx12.rs"]
pub mod dx12;

#[path = "./vulkan/vulkan.rs"]
pub mod vulkan;

use winit::window::Window;
use winit::event_loop::EventLoop;

pub trait App {
    fn new(window: &Window) -> Self;
}
