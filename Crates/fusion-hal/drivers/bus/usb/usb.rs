//! Universal USB driver crate layered over one hardware-facing USB substrate.

#![cfg_attr(not(feature = "std"), no_std)]

use core::marker::PhantomData;

use fusion_hal::contract::drivers::bus::usb as usb_contract;
use fusion_hal::contract::drivers::driver::{
    ActiveDriver,
    DriverActivation,
    DriverActivationContext,
    DriverBindingSource,
    DriverClass,
    DriverContract,
    DriverContractKey,
    DriverDiscoveryContext,
    DriverError,
    DriverIdentity,
    DriverMetadata,
    DriverRegistration,
    DriverUsefulness,
    RegisteredDriver,
};

pub use usb_contract::*;

#[cfg(any(target_os = "none", feature = "fdxe-module"))]
mod fdxe;
#[path = "interface/interface.rs"]
pub mod interface;
mod unsupported;

use self::interface::contract::{
    UsbHardware,
    UsbHardwarePd,
    UsbHardwareThunderbolt,
    UsbHardwareTopology,
    UsbHardwareTypec,
    UsbHardwareUsb4,
};

const USB_DRIVER_CONTRACTS: [DriverContractKey; 1] = [DriverContractKey("bus.usb")];
const USB_DRIVER_REQUIRED_CONTRACTS: [DriverContractKey; 0] = [];
const USB_DRIVER_BINDING_SOURCES: [DriverBindingSource; 6] = [
    DriverBindingSource::StaticSoc,
    DriverBindingSource::BoardManifest,
    DriverBindingSource::Acpi,
    DriverBindingSource::Devicetree,
    DriverBindingSource::Pci,
    DriverBindingSource::Manual,
];
const USB_DRIVER_METADATA: DriverMetadata = DriverMetadata {
    key: "bus.usb",
    class: DriverClass::Bus,
    identity: DriverIdentity {
        vendor: "Fusion",
        family: Some("Generic"),
        package: None,
        product: "USB driver",
        advertised_interface: "USB family",
    },
    contracts: &USB_DRIVER_CONTRACTS,
    required_contracts: &USB_DRIVER_REQUIRED_CONTRACTS,
    usefulness: DriverUsefulness::Standalone,
    singleton_class: None,
    binding_sources: &USB_DRIVER_BINDING_SOURCES,
    description: "Universal USB provider driver layered over one selected hardware substrate",
};

/// Discoverable USB provider binding surfaced by the universal USB driver family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UsbBinding {
    pub provider: u8,
}

/// Registerable universal USB driver family marker.
#[derive(Debug, Clone, Copy, Default)]
pub struct UsbDriver<H: UsbHardware = unsupported::UnsupportedUsbHardware> {
    marker: PhantomData<fn() -> H>,
}

/// One-shot driver discovery/activation context for the universal USB provider.
#[derive(Debug, Clone, Copy, Default)]
pub struct UsbDriverContext<H: UsbHardware = unsupported::UnsupportedUsbHardware> {
    marker: PhantomData<fn() -> H>,
}

impl<H> UsbDriverContext<H>
where
    H: UsbHardware,
{
    /// Creates one empty USB driver context.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            marker: PhantomData,
        }
    }
}

/// Returns the truthful static metadata for the universal USB driver family.
#[must_use]
pub const fn driver_metadata() -> &'static DriverMetadata {
    &USB_DRIVER_METADATA
}

/// Universal USB provider composed over one selected hardware-facing USB substrate.
#[derive(Debug, Clone, Copy, Default)]
pub struct Usb<H: UsbHardware = unsupported::UnsupportedUsbHardware> {
    _hardware: PhantomData<H>,
}

impl<H> Usb<H>
where
    H: UsbHardware,
{
    /// Creates one universal USB provider handle over one selected hardware substrate.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            _hardware: PhantomData,
        }
    }

    /// Opens the host-controller surface when supported.
    ///
    /// # Errors
    ///
    /// Returns one honest error when host-controller access cannot be realized.
    ///
    /// This is an associated function rather than a method because the current universal USB
    /// substrate is namespace-style and does not yet hold per-instance controller state.
    pub fn host_controller() -> Result<Option<H::HostController>, usb_contract::UsbError> {
        H::host_controller()
    }

    /// Opens the device-controller surface when supported.
    ///
    /// # Errors
    ///
    /// Returns one honest error when device-controller access cannot be realized.
    ///
    /// This is an associated function rather than a method because the current universal USB
    /// substrate is namespace-style and does not yet hold per-instance controller state.
    pub fn device_controller() -> Result<Option<H::DeviceController>, usb_contract::UsbError> {
        H::device_controller()
    }
}

impl<H> usb_contract::UsbCoreContract for Usb<H>
where
    H: UsbHardware,
{
    fn usb_support(&self) -> usb_contract::UsbSupport {
        H::support()
    }

    fn usb_core_metadata(&self) -> usb_contract::UsbCoreMetadata {
        H::core_metadata()
    }
}

impl<H> usb_contract::UsbTopologyContract for Usb<H>
where
    H: UsbHardwareTopology,
{
    fn port_count(&self) -> usize {
        H::topology_port_count()
    }

    fn port_status(
        &self,
        port: usb_contract::UsbPortId,
    ) -> Result<usb_contract::UsbPortStatus, usb_contract::UsbError> {
        H::topology_port_status(port)
    }
}

impl<H> usb_contract::UsbTypecPortContract for Usb<H>
where
    H: UsbHardwareTypec,
{
    fn typec_status(
        &self,
    ) -> Result<usb_contract::UsbTypecPortStatus<'static>, usb_contract::UsbError> {
        H::typec_status()
    }
}

impl<H> usb_contract::UsbPdContract for Usb<H>
where
    H: UsbHardwarePd,
{
    fn pd_contract_state(
        &self,
    ) -> Result<usb_contract::UsbPdContractState<'static>, usb_contract::UsbError> {
        H::pd_contract_state()
    }
}

impl<H> usb_contract::Usb4Contract for Usb<H>
where
    H: UsbHardwareUsb4,
{
    fn usb4_metadata(&self) -> usb_contract::Usb4Metadata {
        H::usb4_metadata()
    }

    fn usb4_state(&self) -> Result<usb_contract::Usb4RouterState, usb_contract::UsbError> {
        H::usb4_state()
    }
}

impl<H> usb_contract::ThunderboltContract for Usb<H>
where
    H: UsbHardwareThunderbolt,
{
    fn thunderbolt_metadata(&self) -> usb_contract::ThunderboltMetadata {
        H::thunderbolt_metadata()
    }

    fn thunderbolt_active(&self) -> Result<bool, usb_contract::UsbError> {
        H::thunderbolt_active()
    }
}

fn enumerate_usb_bindings<H>(
    _registered: &RegisteredDriver<UsbDriver<H>>,
    context: &mut DriverDiscoveryContext<'_>,
    out: &mut [UsbBinding],
) -> Result<usize, DriverError>
where
    H: UsbHardware + 'static,
{
    let _ = context.downcast_mut::<UsbDriverContext<H>>()?;
    if out.is_empty() {
        return Err(DriverError::resource_exhausted());
    }

    let support = H::support();
    if support.is_unsupported() || !support.has_any_surface() {
        return Ok(0);
    }

    out[0] = UsbBinding { provider: 0 };
    Ok(1)
}

fn activate_usb_binding<H>(
    _registered: &RegisteredDriver<UsbDriver<H>>,
    context: &mut DriverActivationContext<'_>,
    binding: UsbBinding,
) -> Result<ActiveDriver<UsbDriver<H>>, DriverError>
where
    H: UsbHardware + 'static,
{
    let _ = context.downcast_mut::<UsbDriverContext<H>>()?;
    if binding.provider != 0 {
        return Err(DriverError::invalid());
    }

    Ok(ActiveDriver::new(binding, Usb::<H>::new()))
}

impl<H> DriverContract for UsbDriver<H>
where
    H: UsbHardware + 'static,
{
    type Binding = UsbBinding;
    type Instance = Usb<H>;

    fn registration() -> DriverRegistration<Self> {
        DriverRegistration::new(
            driver_metadata,
            DriverActivation::new(enumerate_usb_bindings::<H>, activate_usb_binding::<H>),
        )
    }
}
