//! Canonical public ACPI battery driver family.

use core::marker::PhantomData;

use crate::contract::drivers::acpi::{
    AcpiBatteryContract,
    AcpiBatteryDescriptor,
    AcpiBatteryInformation,
    AcpiBatteryStatus,
    AcpiBatterySupport,
    AcpiError,
    AcpiProviderDescriptor,
};
use crate::contract::drivers::driver::{
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

use crate::drivers::acpi::public::interface::contract::AcpiBatteryHardware;
use crate::drivers::acpi::public::unsupported;

const BATTERY_DRIVER_CONTRACTS: [DriverContractKey; 1] = [DriverContractKey("acpi.battery")];
const BATTERY_DRIVER_REQUIRED_CONTRACTS: [DriverContractKey; 0] = [];
const BATTERY_DRIVER_BINDING_SOURCES: [DriverBindingSource; 2] =
    [DriverBindingSource::Acpi, DriverBindingSource::Manual];
const BATTERY_DRIVER_METADATA: DriverMetadata = DriverMetadata {
    key: "acpi.battery",
    class: DriverClass::Other("acpi"),
    identity: DriverIdentity {
        vendor: "Fusion",
        family: Some("ACPI"),
        package: None,
        product: "battery driver",
        advertised_interface: "ACPI battery",
    },
    contracts: &BATTERY_DRIVER_CONTRACTS,
    required_contracts: &BATTERY_DRIVER_REQUIRED_CONTRACTS,
    usefulness: DriverUsefulness::Standalone,
    singleton_class: None,
    binding_sources: &BATTERY_DRIVER_BINDING_SOURCES,
    description: "Canonical ACPI battery driver layered over one selected ACPI backend",
};

/// Discoverable ACPI battery-provider binding surfaced by the canonical public battery driver.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AcpiBatteryBinding {
    pub provider: u8,
    pub provider_id: &'static str,
}

/// Registerable public ACPI battery driver family marker.
#[derive(Debug, Clone, Copy, Default)]
pub struct AcpiBatteryDriver<H: AcpiBatteryHardware = unsupported::UnsupportedAcpiHardware> {
    marker: PhantomData<fn() -> H>,
}

/// One-shot discovery/activation context for the public ACPI battery driver family.
#[derive(Debug, Clone, Copy, Default)]
pub struct AcpiBatteryDriverContext<H: AcpiBatteryHardware = unsupported::UnsupportedAcpiHardware> {
    marker: PhantomData<fn() -> H>,
}

impl<H> AcpiBatteryDriverContext<H>
where
    H: AcpiBatteryHardware,
{
    #[must_use]
    pub const fn new() -> Self {
        Self {
            marker: PhantomData,
        }
    }
}

#[must_use]
pub const fn driver_metadata() -> &'static DriverMetadata {
    &BATTERY_DRIVER_METADATA
}

/// Public ACPI battery provider composed over one selected backend.
#[derive(Debug, Clone, Copy, Default)]
pub struct AcpiBattery<H: AcpiBatteryHardware = unsupported::UnsupportedAcpiHardware> {
    provider: u8,
    _hardware: PhantomData<H>,
}

impl<H> AcpiBattery<H>
where
    H: AcpiBatteryHardware,
{
    #[must_use]
    pub const fn new(provider: u8) -> Self {
        Self {
            provider,
            _hardware: PhantomData,
        }
    }

    pub fn provider(&self) -> Result<&'static AcpiProviderDescriptor, AcpiError> {
        H::provider(self.provider).ok_or_else(AcpiError::invalid)
    }
}

impl<H> AcpiBatteryContract for AcpiBattery<H>
where
    H: AcpiBatteryHardware,
{
    fn batteries(&self) -> &'static [AcpiBatteryDescriptor] {
        H::batteries(self.provider)
    }

    fn battery_support(&self, index: u8) -> Result<AcpiBatterySupport, AcpiError> {
        H::battery_support(self.provider, index)
    }

    fn battery_information(&self, index: u8) -> Result<AcpiBatteryInformation, AcpiError> {
        H::battery_information(self.provider, index)
    }

    fn battery_status(&self, index: u8) -> Result<AcpiBatteryStatus, AcpiError> {
        H::battery_status(self.provider, index)
    }
}

fn enumerate_battery_bindings<H>(
    _registered: &RegisteredDriver<AcpiBatteryDriver<H>>,
    context: &mut DriverDiscoveryContext<'_>,
    out: &mut [AcpiBatteryBinding],
) -> Result<usize, DriverError>
where
    H: AcpiBatteryHardware + 'static,
{
    let _ = context.downcast_mut::<AcpiBatteryDriverContext<H>>()?;
    if out.is_empty() {
        return Err(DriverError::resource_exhausted());
    }

    let mut written = 0;
    for provider in 0..H::provider_count() {
        if written == out.len() {
            return Err(DriverError::resource_exhausted());
        }
        let Some(provider_descriptor) = H::provider(provider) else {
            continue;
        };
        if H::batteries(provider).is_empty() {
            continue;
        }
        out[written] = AcpiBatteryBinding {
            provider,
            provider_id: provider_descriptor.id,
        };
        written += 1;
    }

    Ok(written)
}

fn activate_battery_binding<H>(
    _registered: &RegisteredDriver<AcpiBatteryDriver<H>>,
    context: &mut DriverActivationContext<'_>,
    binding: AcpiBatteryBinding,
) -> Result<ActiveDriver<AcpiBatteryDriver<H>>, DriverError>
where
    H: AcpiBatteryHardware + 'static,
{
    let _ = context.downcast_mut::<AcpiBatteryDriverContext<H>>()?;
    let Some(provider_descriptor) = H::provider(binding.provider) else {
        return Err(DriverError::invalid());
    };
    if provider_descriptor.id != binding.provider_id {
        return Err(DriverError::invalid());
    }

    Ok(ActiveDriver::new(
        binding,
        AcpiBattery::<H>::new(binding.provider),
    ))
}

impl<H> DriverContract for AcpiBatteryDriver<H>
where
    H: AcpiBatteryHardware + 'static,
{
    type Binding = AcpiBatteryBinding;
    type Instance = AcpiBattery<H>;

    fn registration() -> DriverRegistration<Self> {
        DriverRegistration::new(
            driver_metadata,
            DriverActivation::new(
                enumerate_battery_bindings::<H>,
                activate_battery_binding::<H>,
            ),
        )
    }
}
