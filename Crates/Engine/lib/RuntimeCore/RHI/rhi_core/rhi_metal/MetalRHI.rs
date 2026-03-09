#![allow(
    dead_code,
    reason = "Metal backend is scaffolded but not implemented yet."
)]

use fusion_rhi_core::RHI;

pub struct MetalRHIBuilder {}

#[derive(Clone)]
pub struct MetalRHI {
    rhi_name: &'static str,
}

impl Default for MetalRHI {
    fn default() -> Self {
        Self { rhi_name: "Metal" }
    }
}

impl MetalRHI {}

impl RHI<MetalRHIBuilder, Self> for MetalRHI {
    fn init(_builder: &MetalRHIBuilder) -> fusion_rhi_core::rhi_error::Result<Self> {
        todo!()
    }

    fn post_init(&mut self) {
        todo!()
    }

    fn shutdown(&mut self) {
        todo!()
    }
}
