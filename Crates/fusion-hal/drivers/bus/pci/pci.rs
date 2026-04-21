//! Universal PCI driver crate layered over one hardware-facing PCI substrate.

#![cfg_attr(not(feature = "std"), no_std)]

use core::marker::PhantomData;

use fusion_hal::contract::drivers::bus::pci as pci_contract;
use fusion_hal::contract::drivers::driver::{
    ActiveDriver,
    DriverActivation,
    DriverActivationContext,
    DriverBindingSource,
    DriverClass,
    DriverContract,
    DriverDiscoveryContext,
    DriverError,
    DriverIdentity,
    DriverMetadata,
    DriverRegistration,
    RegisteredDriver,
};
pub(crate) use fusion_hal::contract::drivers::driver::{
    DriverContractKey,
    DriverDogma,
    DriverUsefulness,
};

pub use pci_contract::*;

mod dogma;
#[cfg(any(target_os = "none", feature = "fdxe-module"))]
mod fdxe;
#[path = "interface/interface.rs"]
pub mod interface;
mod unsupported;

use self::interface::contract::{
    PciHardware,
    PciHardwareFunction,
};

// The universal PCI family declares the full binding-source taxonomy up front even though only
// manual attachment is practically usable today. That keeps future ACPI/DT/platform attachers from
// forcing a contract metadata rewrite just because the discovery side finally grew up.
const PCI_DRIVER_BINDING_SOURCES: [DriverBindingSource; 5] = [
    DriverBindingSource::StaticSoc,
    DriverBindingSource::BoardManifest,
    DriverBindingSource::Acpi,
    DriverBindingSource::Devicetree,
    DriverBindingSource::Manual,
];
const PCI_DRIVER_METADATA: DriverMetadata = DriverMetadata {
    key: dogma::PCI_DRIVER_DOGMA.key,
    class: DriverClass::Bus,
    identity: DriverIdentity {
        vendor: "Fusion",
        family: Some("Generic"),
        package: None,
        product: "PCI driver",
        advertised_interface: "PCI family",
    },
    contracts: dogma::PCI_DRIVER_DOGMA.contracts,
    required_contracts: dogma::PCI_DRIVER_DOGMA.required_contracts,
    usefulness: dogma::PCI_DRIVER_DOGMA.usefulness,
    singleton_class: dogma::PCI_DRIVER_DOGMA.singleton_class,
    binding_sources: &PCI_DRIVER_BINDING_SOURCES,
    description: "Universal PCI provider driver layered over one selected hardware substrate",
};

/// Discoverable PCI provider binding surfaced by the universal PCI driver family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PciBinding {
    pub provider: u8,
    pub controller_id: &'static str,
}

/// Registerable universal PCI driver family marker.
#[derive(Debug, Clone, Copy, Default)]
pub struct PciDriver<H: PciHardware = unsupported::UnsupportedPciHardware> {
    marker: PhantomData<fn() -> H>,
}

/// One-shot driver discovery/activation context for the universal PCI provider.
#[derive(Debug, Clone, Copy, Default)]
pub struct PciDriverContext<H: PciHardware = unsupported::UnsupportedPciHardware> {
    marker: PhantomData<fn() -> H>,
}

impl<H> PciDriverContext<H>
where
    H: PciHardware,
{
    /// Creates one empty PCI driver context.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            marker: PhantomData,
        }
    }
}

/// Returns the truthful static metadata for the universal PCI driver family.
#[must_use]
pub const fn driver_metadata() -> &'static DriverMetadata {
    &PCI_DRIVER_METADATA
}

/// Universal PCI provider composed over one selected hardware-facing PCI substrate.
#[derive(Debug, Clone, Copy, Default)]
pub struct Pci<H: PciHardware = unsupported::UnsupportedPciHardware> {
    provider: u8,
    _hardware: PhantomData<H>,
}

/// Universal PCI function handle composed over one selected hardware-facing PCI substrate.
#[derive(Debug)]
pub struct PciFunction<F: PciHardwareFunction = unsupported::UnsupportedPciHardwareFunction> {
    inner: F,
}

impl<H> Pci<H>
where
    H: PciHardware,
{
    /// Creates one universal PCI provider handle over one selected controller/provider.
    #[must_use]
    pub const fn new(provider: u8) -> Self {
        Self {
            provider,
            _hardware: PhantomData,
        }
    }

    /// Returns the descriptor for this selected controller/provider.
    ///
    /// # Errors
    ///
    /// Returns one honest error when the selected provider binding is invalid.
    pub fn controller(&self) -> Result<&'static PciControllerDescriptor, PciError> {
        H::controller(self.provider).ok_or_else(PciError::invalid)
    }

    /// Returns the truthful coarse support summary for this provider.
    #[must_use]
    pub fn support(&self) -> PciSupport {
        H::support(self.provider)
    }

    /// Returns the surfaced segment ranges for this provider.
    #[must_use]
    pub fn segments(&self) -> &'static [PciSegmentDescriptor] {
        H::segments(self.provider)
    }

    /// Enumerates visible functions through this provider.
    ///
    /// # Errors
    ///
    /// Returns one honest error when enumeration fails.
    pub fn enumerate_functions(&self, out: &mut [PciFunctionAddress]) -> Result<usize, PciError> {
        H::enumerate_functions(self.provider, out)
    }

    /// Opens one function handle at the requested address when visible.
    ///
    /// # Errors
    ///
    /// Returns one honest error when the function cannot be reached.
    pub fn function(
        &self,
        address: PciFunctionAddress,
    ) -> Result<Option<PciFunction<H::Function>>, PciError> {
        H::function(self.provider, address).map(|function| function.map(PciFunction::from_inner))
    }
}

impl<F> PciFunction<F>
where
    F: PciHardwareFunction,
{
    /// Wraps one already-owned hardware-facing PCI function handle.
    #[must_use]
    pub fn from_inner(inner: F) -> Self {
        Self { inner }
    }

    /// Releases the hardware-facing function handle back to the caller.
    #[must_use]
    pub fn into_inner(self) -> F {
        self.inner
    }
}

impl<H> PciControllerContract for Pci<H>
where
    H: PciHardware,
{
    type Function = PciFunction<H::Function>;

    fn controller(&self) -> &'static PciControllerDescriptor {
        Pci::controller(self).unwrap_or_else(|_| panic!("invalid pci provider {}", self.provider))
    }

    fn support(&self) -> PciSupport {
        self.support()
    }

    fn segments(&self) -> &'static [PciSegmentDescriptor] {
        self.segments()
    }

    fn enumerate_functions(&self, out: &mut [PciFunctionAddress]) -> Result<usize, PciError> {
        self.enumerate_functions(out)
    }

    fn function(&self, address: PciFunctionAddress) -> Result<Option<Self::Function>, PciError> {
        self.function(address)
    }
}

// The forwarding impls below are intentionally boring. They make the wrapper boundary explicit and
// keep failures readable in backtraces and compile errors; a macro would save lines and cost
// clarity, which is a bad trade for contract glue that should almost never change shape.
impl<F> PciFunctionContract for PciFunction<F>
where
    F: PciHardwareFunction,
{
    fn address(&self) -> PciFunctionAddress {
        self.inner.address()
    }

    fn identity(&self) -> PciFunctionIdentity {
        self.inner.identity()
    }

    fn profile(&self) -> PciFunctionProfile {
        self.inner.profile()
    }

    fn bars(&self) -> &[PciBarDescriptor] {
        self.inner.bars()
    }

    fn bridge_windows(&self) -> &[PciBridgeWindow] {
        self.inner.bridge_windows()
    }

    fn option_rom(&self) -> Option<PciRomDescriptor> {
        self.inner.option_rom()
    }

    fn capabilities(&self) -> &[PciCapabilityRecord] {
        self.inner.capabilities()
    }

    fn read_config_u8(&self, offset: PciConfigOffset) -> Result<u8, PciError> {
        self.inner.read_config_u8(offset)
    }

    fn read_config_u16(&self, offset: PciConfigOffset) -> Result<u16, PciError> {
        self.inner.read_config_u16(offset)
    }

    fn read_config_u32(&self, offset: PciConfigOffset) -> Result<u32, PciError> {
        self.inner.read_config_u32(offset)
    }

    fn write_config_u8(&mut self, offset: PciConfigOffset, value: u8) -> Result<(), PciError> {
        self.inner.write_config_u8(offset, value)
    }

    fn write_config_u16(&mut self, offset: PciConfigOffset, value: u16) -> Result<(), PciError> {
        self.inner.write_config_u16(offset, value)
    }

    fn write_config_u32(&mut self, offset: PciConfigOffset, value: u32) -> Result<(), PciError> {
        self.inner.write_config_u32(offset, value)
    }
}

impl<F> PciExpressContract for PciFunction<F>
where
    F: PciHardwareFunction,
{
    fn pcie_profile(&self) -> Option<PciExpressProfile> {
        self.inner.pcie_profile()
    }

    fn extended_capabilities(&self) -> &[PciExtendedCapabilityRecord] {
        self.inner.extended_capabilities()
    }
}

impl<F> PciTopologyContract for PciFunction<F>
where
    F: PciHardwareFunction,
{
    fn topology_profile(&self) -> PciTopologyProfile {
        self.inner.topology_profile()
    }
}

impl<F> PciInterruptContract for PciFunction<F>
where
    F: PciHardwareFunction,
{
    fn interrupt_profile(&self) -> PciInterruptProfile {
        self.inner.interrupt_profile()
    }
}

impl<F> PciDmaContract for PciFunction<F>
where
    F: PciHardwareFunction,
{
    fn dma_profile(&self) -> PciDmaProfile {
        self.inner.dma_profile()
    }
}

impl<F> PciPowerContract for PciFunction<F>
where
    F: PciHardwareFunction,
{
    fn power_profile(&self) -> PciPowerProfile {
        self.inner.power_profile()
    }
}

impl<F> PciErrorReportingContract for PciFunction<F>
where
    F: PciHardwareFunction,
{
    fn error_reporting_profile(&self) -> PciErrorReportingProfile {
        self.inner.error_reporting_profile()
    }
}

impl<F> PciVirtualizationContract for PciFunction<F>
where
    F: PciHardwareFunction,
{
    fn virtualization_profile(&self) -> PciVirtualizationProfile {
        self.inner.virtualization_profile()
    }
}

impl<F> PciHotplugContract for PciFunction<F>
where
    F: PciHardwareFunction,
{
    fn hotplug_profile(&self) -> Option<PciHotplugProfile> {
        self.inner.hotplug_profile()
    }
}

fn enumerate_pci_bindings<H>(
    _registered: &RegisteredDriver<PciDriver<H>>,
    context: &mut DriverDiscoveryContext<'_>,
    out: &mut [PciBinding],
) -> Result<usize, DriverError>
where
    H: PciHardware + 'static,
{
    let _ = context.downcast_mut::<PciDriverContext<H>>()?;
    if out.is_empty() {
        return Err(DriverError::resource_exhausted());
    }

    let mut written = 0;
    for provider in 0..H::provider_count() {
        if written == out.len() {
            return Err(DriverError::resource_exhausted());
        }
        let support = H::support(provider);
        let Some(controller) = H::controller(provider) else {
            continue;
        };
        if support.is_unsupported() {
            continue;
        }
        out[written] = PciBinding {
            provider,
            controller_id: controller.id,
        };
        written += 1;
    }
    Ok(written)
}

fn activate_pci_binding<H>(
    _registered: &RegisteredDriver<PciDriver<H>>,
    context: &mut DriverActivationContext<'_>,
    binding: PciBinding,
) -> Result<ActiveDriver<PciDriver<H>>, DriverError>
where
    H: PciHardware + 'static,
{
    let _ = context.downcast_mut::<PciDriverContext<H>>()?;
    let Some(controller) = H::controller(binding.provider) else {
        return Err(DriverError::invalid());
    };
    if controller.id != binding.controller_id {
        return Err(DriverError::invalid());
    }

    Ok(ActiveDriver::new(binding, Pci::<H>::new(binding.provider)))
}

impl<H> DriverContract for PciDriver<H>
where
    H: PciHardware + 'static,
{
    type Binding = PciBinding;
    type Instance = Pci<H>;

    fn registration() -> DriverRegistration<Self> {
        DriverRegistration::new(
            driver_metadata,
            DriverActivation::new(enumerate_pci_bindings::<H>, activate_pci_binding::<H>),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // These tests live inline for now because the crate is still tiny. If the backend matrix or
    // capability coverage grows substantially, split them into sibling modules instead of turning
    // this file into a standards-body landfill.

    const fn test_pci_device(value: u8) -> fusion_hal::contract::drivers::bus::pci::PciDevice {
        match fusion_hal::contract::drivers::bus::pci::PciDevice::from_u8(value) {
            Some(device) => device,
            None => panic!("invalid test pci device"),
        }
    }

    const fn test_pci_function(value: u8) -> fusion_hal::contract::drivers::bus::pci::PciFunction {
        match fusion_hal::contract::drivers::bus::pci::PciFunction::from_u8(value) {
            Some(function) => function,
            None => panic!("invalid test pci function"),
        }
    }

    const TEST_CONTROLLER_A: PciControllerDescriptor = PciControllerDescriptor {
        id: "test-pci-a",
        name: "Test PCI A",
    };
    const TEST_CONTROLLER_B: PciControllerDescriptor = PciControllerDescriptor {
        id: "test-pci-b",
        name: "Test PCI B",
    };
    const TEST_SEGMENTS_A: [PciSegmentDescriptor; 1] = [PciSegmentDescriptor {
        segment: PciSegment(0),
        start_bus: PciBus(0),
        end_bus: PciBus(31),
    }];
    const TEST_SEGMENTS_B: [PciSegmentDescriptor; 1] = [PciSegmentDescriptor {
        segment: PciSegment(4),
        start_bus: PciBus(64),
        end_bus: PciBus(95),
    }];
    const TEST_CAPS: [PciCapabilityRecord; 1] = [PciCapabilityRecord {
        id: PciCapabilityId::Msi,
        offset: PciConfigOffset(0x50),
        next: None,
    }];
    const TEST_EXT_CAPS: [PciExtendedCapabilityRecord; 1] = [PciExtendedCapabilityRecord {
        id: PciExtendedCapabilityId::AdvancedErrorReporting,
        version: 2,
        offset: PciConfigOffset(0x100),
        next: None,
    }];
    const TEST_BARS: [PciBarDescriptor; 1] = [PciBarDescriptor {
        index: 0,
        kind: PciBarKind::Memory64,
        base: 0x8000_0000,
        size: 0x1000,
        prefetchable: false,
        implemented: true,
    }];
    const TEST_SUPPORT: PciSupport = PciSupport {
        implementation: PciImplementationKind::Hardware,
        pcie: true,
        interrupts: true,
        dma: true,
        power_management: true,
        error_reporting: true,
        virtualization: false,
        hotplug: false,
    };
    const TEST_ADDR_A: PciFunctionAddress = PciFunctionAddress {
        segment: PciSegment(0),
        bus: PciBus(0),
        device: test_pci_device(1),
        function: test_pci_function(0),
    };
    const TEST_ADDR_B: PciFunctionAddress = PciFunctionAddress {
        segment: PciSegment(4),
        bus: PciBus(64),
        device: test_pci_device(2),
        function: test_pci_function(0),
    };

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct TestFunction {
        provider: u8,
        address: PciFunctionAddress,
    }

    #[derive(Debug, Clone, Copy, Default)]
    struct TestHardware;

    impl PciHardware for TestHardware {
        type Function = TestFunction;

        fn provider_count() -> u8 {
            2
        }

        fn controller(provider: u8) -> Option<&'static PciControllerDescriptor> {
            match provider {
                0 => Some(&TEST_CONTROLLER_A),
                1 => Some(&TEST_CONTROLLER_B),
                _ => None,
            }
        }

        fn support(provider: u8) -> PciSupport {
            match provider {
                0 | 1 => TEST_SUPPORT,
                _ => PciSupport::unsupported(),
            }
        }

        fn segments(provider: u8) -> &'static [PciSegmentDescriptor] {
            match provider {
                0 => &TEST_SEGMENTS_A,
                1 => &TEST_SEGMENTS_B,
                _ => &[],
            }
        }

        fn enumerate_functions(
            provider: u8,
            out: &mut [PciFunctionAddress],
        ) -> Result<usize, PciError> {
            if out.is_empty() {
                return Err(PciError::resource_exhausted());
            }
            match provider {
                0 => {
                    out[0] = TEST_ADDR_A;
                    Ok(1)
                }
                1 => {
                    out[0] = TEST_ADDR_B;
                    Ok(1)
                }
                _ => Ok(0),
            }
        }

        fn function(
            provider: u8,
            address: PciFunctionAddress,
        ) -> Result<Option<Self::Function>, PciError> {
            let expected = match provider {
                0 => TEST_ADDR_A,
                1 => TEST_ADDR_B,
                _ => return Ok(None),
            };
            if address == expected {
                Ok(Some(TestFunction { provider, address }))
            } else {
                Ok(None)
            }
        }
    }

    impl PciHardwareFunction for TestFunction {
        fn address(&self) -> PciFunctionAddress {
            self.address
        }

        fn identity(&self) -> PciFunctionIdentity {
            PciFunctionIdentity {
                vendor_id: PciVendorId(0x1234),
                device_id: PciDeviceId(0x5678),
                subsystem_vendor_id: Some(PciSubsystemVendorId(0xabcd)),
                subsystem_id: Some(PciSubsystemId(0xef01)),
                class_code: PciClassCode {
                    base: 0x02,
                    sub: 0x00,
                    interface: 0x00,
                },
                revision_id: 1,
            }
        }

        fn profile(&self) -> PciFunctionProfile {
            PciFunctionProfile {
                transport_family: PciTransportFamily::PciExpress,
                configuration_model: PciConfigurationModel::Enhanced4KiB,
                header_type: PciHeaderType::Type0,
                multifunction: false,
                kind: PciFunctionKind::Endpoint,
            }
        }

        fn bars(&self) -> &[PciBarDescriptor] {
            &TEST_BARS
        }

        fn bridge_windows(&self) -> &[PciBridgeWindow] {
            &[]
        }

        fn option_rom(&self) -> Option<PciRomDescriptor> {
            None
        }

        fn capabilities(&self) -> &[PciCapabilityRecord] {
            &TEST_CAPS
        }

        fn extended_capabilities(&self) -> &[PciExtendedCapabilityRecord] {
            &TEST_EXT_CAPS
        }

        fn topology_profile(&self) -> PciTopologyProfile {
            PciTopologyProfile::default()
        }

        fn interrupt_profile(&self) -> PciInterruptProfile {
            PciInterruptProfile {
                legacy_pin: Some(PciInterruptPin::IntA),
                msi: Some(PciMsiProfile {
                    vector_count: 8,
                    is_64_bit: true,
                    per_vector_masking: true,
                }),
                msix: None,
            }
        }

        fn dma_profile(&self) -> PciDmaProfile {
            PciDmaProfile {
                bus_master_capable: true,
                dma_address_width: Some(PciDmaAddressWidth::Bits64),
                cache_coherent: Some(true),
                ats: false,
                pri: false,
                pasid: false,
                acs: false,
            }
        }

        fn power_profile(&self) -> PciPowerProfile {
            PciPowerProfile {
                capability_version: Some(3),
                current_state: Some(PciPowerState::D0),
                pme_supported: true,
                pme_enabled: false,
                aux_current_ma: Some(0),
                aspm_supported: true,
                aspm_enabled: true,
            }
        }

        fn error_reporting_profile(&self) -> PciErrorReportingProfile {
            PciErrorReportingProfile {
                advanced_error_reporting: true,
                downstream_port_containment: false,
                ecrc_checking_capable: true,
                ecrc_generation_capable: true,
            }
        }

        fn virtualization_profile(&self) -> PciVirtualizationProfile {
            PciVirtualizationProfile::default()
        }

        fn hotplug_profile(&self) -> Option<PciHotplugProfile> {
            None
        }

        fn pcie_profile(&self) -> Option<PciExpressProfile> {
            Some(PciExpressProfile {
                capability_version: Some(PciExpressVersion(2)),
                device_port_type: Some(PciExpressDevicePortType::Endpoint),
                max_link_speed: Some(PciLinkSpeed::Gen4),
                current_link_speed: Some(PciLinkSpeed::Gen4),
                max_link_width: Some(PciLinkWidth(4)),
                current_link_width: Some(PciLinkWidth(4)),
                slot_implemented: false,
                hotplug_capable: false,
                surprise_hotplug_capable: false,
                dll_link_active_reporting_capable: true,
                dll_link_active: Some(true),
                link_training: Some(false),
            })
        }

        fn read_config_u8(&self, offset: PciConfigOffset) -> Result<u8, PciError> {
            Ok((offset.0 & 0xff) as u8)
        }

        fn read_config_u16(&self, offset: PciConfigOffset) -> Result<u16, PciError> {
            Ok(offset.0)
        }

        fn read_config_u32(&self, offset: PciConfigOffset) -> Result<u32, PciError> {
            Ok(offset.0 as u32)
        }

        fn write_config_u8(
            &mut self,
            _offset: PciConfigOffset,
            _value: u8,
        ) -> Result<(), PciError> {
            Ok(())
        }

        fn write_config_u16(
            &mut self,
            _offset: PciConfigOffset,
            _value: u16,
        ) -> Result<(), PciError> {
            Ok(())
        }

        fn write_config_u32(
            &mut self,
            _offset: PciConfigOffset,
            _value: u32,
        ) -> Result<(), PciError> {
            Ok(())
        }
    }

    #[test]
    fn enumerates_multiple_pci_providers() {
        let mut registry = fusion_hal::contract::drivers::driver::DriverRegistry::<1>::new();
        let registered = registry
            .register::<PciDriver<TestHardware>>()
            .expect("register pci driver");
        let mut payload = PciDriverContext::<TestHardware>::new();
        let mut context = DriverDiscoveryContext::new(&mut payload);
        let mut bindings = [PciBinding {
            provider: 0,
            controller_id: "",
        }; 4];

        let count = registered
            .enumerate_bindings(&mut context, &mut bindings)
            .expect("discover bindings");

        assert_eq!(count, 2);
        assert_eq!(bindings[0].controller_id, "test-pci-a");
        assert_eq!(bindings[1].controller_id, "test-pci-b");
    }

    #[test]
    fn opens_function_and_exposes_profiles() {
        let pci = Pci::<TestHardware>::new(0);
        let function = pci
            .function(TEST_ADDR_A)
            .expect("function access should succeed")
            .expect("function should exist");

        assert_eq!(function.address(), TEST_ADDR_A);
        assert_eq!(function.identity().vendor_id, PciVendorId(0x1234));
        assert_eq!(
            function.profile().transport_family,
            PciTransportFamily::PciExpress
        );
        assert_eq!(function.capabilities()[0].id, PciCapabilityId::Msi);
        assert_eq!(
            function
                .pcie_profile()
                .expect("pcie profile")
                .current_link_speed,
            Some(PciLinkSpeed::Gen4)
        );
    }
}
