use fusion_rhi_core::{
    AppInfo,
    RHI
};

#[derive(Clone)]
pub struct MetalRHI<'m> {
    rhi_name: &'m str,
}

impl<'m> Default for MetalRHI<'m> {
    fn default() -> Self {
        Self {
            rhi_name: "Metal"
        }
    }
}

impl<'m> MetalRHI<'m> {

}

impl<'m> RHI<MetalRHI<'m>> for MetalRHI<'m> {
    fn init(&self, app_info: &AppInfo) -> fusion_rhi_core::rhi_error::Result<'_, MetalRHI<'m>> {
        todo!()
    }

    fn post_init(&mut self) {
        todo!()
    }

    fn shutdown(&mut self) {
        todo!()
    }
}
