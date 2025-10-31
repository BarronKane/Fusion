use crate::rhi_error::Result;

pub trait Adapter {
    fn pick_physical_device(&mut self) -> Result<'_, ()>;
}