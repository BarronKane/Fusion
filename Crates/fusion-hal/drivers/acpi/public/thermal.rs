//! Canonical public ACPI thermal-zone driver family.

use core::marker::PhantomData;

use crate::contract::drivers::acpi::{
    AcpiError,
    AcpiProviderDescriptor,
    AcpiThermalContract,
    AcpiThermalReading,
    AcpiThermalSupport,
    AcpiThermalZoneDescriptor,
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
    RegisteredDriver,
};

use crate::drivers::acpi::public::interface::contract::AcpiThermalHardware;
use crate::drivers::acpi::public::unsupported;

const THERMAL_DRIVER_CONTRACTS: [DriverContractKey; 1] = [DriverContractKey("acpi.thermal")];
const THERMAL_DRIVER_BINDING_SOURCES: [DriverBindingSource; 2] =
    [DriverBindingSource::Acpi, DriverBindingSource::Manual];
const THERMAL_DRIVER_METADATA: DriverMetadata = DriverMetadata {
    key: "acpi.thermal",
    class: DriverClass::Other("acpi"),
    identity: DriverIdentity {
        vendor: "Fusion",
        family: Some("ACPI"),
        package: None,
        product: "thermal driver",
        advertised_interface: "ACPI thermal zone",
    },
    contracts: &THERMAL_DRIVER_CONTRACTS,
    binding_sources: &THERMAL_DRIVER_BINDING_SOURCES,
    description: "Canonical ACPI thermal-zone driver layered over one selected ACPI backend",
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AcpiThermalBinding {
    pub provider: u8,
    pub provider_id: &'static str,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AcpiThermalDriver<H: AcpiThermalHardware = unsupported::UnsupportedAcpiHardware> {
    marker: PhantomData<fn() -> H>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AcpiThermalDriverContext<H: AcpiThermalHardware = unsupported::UnsupportedAcpiHardware> {
    marker: PhantomData<fn() -> H>,
}

impl<H> AcpiThermalDriverContext<H>
where
    H: AcpiThermalHardware,
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
    &THERMAL_DRIVER_METADATA
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AcpiThermal<H: AcpiThermalHardware = unsupported::UnsupportedAcpiHardware> {
    provider: u8,
    _hardware: PhantomData<H>,
}

impl<H> AcpiThermal<H>
where
    H: AcpiThermalHardware,
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

impl<H> AcpiThermalContract for AcpiThermal<H>
where
    H: AcpiThermalHardware,
{
    fn thermal_zones(&self) -> &'static [AcpiThermalZoneDescriptor] {
        H::thermal_zones(self.provider)
    }

    fn thermal_zone_support(&self, index: u8) -> Result<AcpiThermalSupport, AcpiError> {
        H::thermal_zone_support(self.provider, index)
    }

    fn thermal_reading(&self, index: u8) -> Result<AcpiThermalReading, AcpiError> {
        H::thermal_reading(self.provider, index)
    }
}

fn enumerate_thermal_bindings<H>(
    _registered: &RegisteredDriver<AcpiThermalDriver<H>>,
    context: &mut DriverDiscoveryContext<'_>,
    out: &mut [AcpiThermalBinding],
) -> Result<usize, DriverError>
where
    H: AcpiThermalHardware + 'static,
{
    let _ = context.downcast_mut::<AcpiThermalDriverContext<H>>()?;
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
        if H::thermal_zones(provider).is_empty() {
            continue;
        }
        out[written] = AcpiThermalBinding {
            provider,
            provider_id: provider_descriptor.id,
        };
        written += 1;
    }

    Ok(written)
}

fn activate_thermal_binding<H>(
    _registered: &RegisteredDriver<AcpiThermalDriver<H>>,
    context: &mut DriverActivationContext<'_>,
    binding: AcpiThermalBinding,
) -> Result<ActiveDriver<AcpiThermalDriver<H>>, DriverError>
where
    H: AcpiThermalHardware + 'static,
{
    let _ = context.downcast_mut::<AcpiThermalDriverContext<H>>()?;
    let Some(provider_descriptor) = H::provider(binding.provider) else {
        return Err(DriverError::invalid());
    };
    if provider_descriptor.id != binding.provider_id {
        return Err(DriverError::invalid());
    }

    Ok(ActiveDriver::new(
        binding,
        AcpiThermal::<H>::new(binding.provider),
    ))
}

impl<H> DriverContract for AcpiThermalDriver<H>
where
    H: AcpiThermalHardware + 'static,
{
    type Binding = AcpiThermalBinding;
    type Instance = AcpiThermal<H>;

    fn registration() -> DriverRegistration<Self> {
        DriverRegistration::new(
            driver_metadata,
            DriverActivation::new(
                enumerate_thermal_bindings::<H>,
                activate_thermal_binding::<H>,
            ),
        )
    }
}
