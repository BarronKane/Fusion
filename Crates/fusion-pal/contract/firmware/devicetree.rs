#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeviceTreeSupport {
    Unsupported,
    StaticBlob,
    RuntimeEnumeration,
}

pub trait DeviceTreeFirmwareContract {
    fn devicetree_support(&self) -> DeviceTreeSupport;
}
