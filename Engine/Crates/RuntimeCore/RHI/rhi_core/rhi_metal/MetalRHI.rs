use fusion_rhi_core::{
    AppInfo,
    RHI
};

pub struct MetalRHIBuilder {
    
}

#[derive(Clone)]
pub struct MetalRHI {
    rhi_name: &'static str,
}

impl Default for MetalRHI {
    fn default() -> Self {
        Self {
            rhi_name: "Metal"
        }
    }
}

impl<'m> MetalRHI {

}

impl<'m> RHI<MetalRHIBuilder, MetalRHI> for MetalRHI {
    fn init(builder: &MetalRHIBuilder) -> fusion_rhi_core::rhi_error::Result<MetalRHI> {
        todo!()
    }

    fn post_init(&mut self) {
        todo!()
    }

    fn shutdown(&mut self) {
        todo!()
    }
}
