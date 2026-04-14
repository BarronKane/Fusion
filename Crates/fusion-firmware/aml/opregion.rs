//! AML operation-region descriptors and host access boundary.

use crate::aml::{
    AmlResult,
    AmlNamespaceNodeId,
};

/// AML operation-region address-space identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AmlAddressSpaceId {
    SystemMemory,
    SystemIo,
    PciConfig,
    EmbeddedControl,
    SmBus,
    Cmos,
    PciBarTarget,
    Ipmi,
    Gpio,
    GenericSerialBus,
    PlatformCommChannel,
    FunctionalFixedHardware,
    Oem(u8),
}

/// Width for one AML region access.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AmlAccessWidth {
    Bits8,
    Bits16,
    Bits32,
    Bits64,
}

/// Stable descriptor for one loaded AML operation region.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AmlOpRegionDescriptor {
    pub node: AmlNamespaceNodeId,
    pub space: AmlAddressSpaceId,
    pub offset: Option<u64>,
    pub length: Option<u64>,
}

/// Host handler for one AML address-space class.
pub trait AmlOpRegionHandler {
    fn read(
        &self,
        region: &AmlOpRegionDescriptor,
        offset: u64,
        width: AmlAccessWidth,
    ) -> AmlResult<u64>;

    fn write(
        &self,
        region: &AmlOpRegionDescriptor,
        offset: u64,
        width: AmlAccessWidth,
        value: u64,
    ) -> AmlResult<()>;
}
