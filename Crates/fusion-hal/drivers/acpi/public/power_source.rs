//! Canonical public ACPI power-source driver family.

use core::marker::PhantomData;

use crate::contract::drivers::acpi::{
    AcpiError,
    AcpiPowerSourceContract,
    AcpiPowerSourceDescriptor,
    AcpiPowerSourceState,
    AcpiPowerSourceSupport,
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
    RegisteredDriver,
};

use crate::drivers::acpi::public::interface::contract::AcpiPowerSourceHardware;
use crate::drivers::acpi::public::unsupported;

const POWER_SOURCE_DRIVER_CONTRACTS: [DriverContractKey; 1] =
    [DriverContractKey("acpi.power_source")];
const POWER_SOURCE_DRIVER_BINDING_SOURCES: [DriverBindingSource; 2] =
    [DriverBindingSource::Acpi, DriverBindingSource::Manual];
const POWER_SOURCE_DRIVER_METADATA: DriverMetadata = DriverMetadata {
    key: "acpi.power_source",
    class: DriverClass::Other("acpi"),
    identity: DriverIdentity {
        vendor: "Fusion",
        family: Some("ACPI"),
        package: None,
        product: "power-source driver",
        advertised_interface: "ACPI power source",
    },
    contracts: &POWER_SOURCE_DRIVER_CONTRACTS,
    binding_sources: &POWER_SOURCE_DRIVER_BINDING_SOURCES,
    description: "Canonical ACPI power-source driver layered over one selected ACPI backend",
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AcpiPowerSourceBinding {
    pub provider: u8,
    pub provider_id: &'static str,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AcpiPowerSourceDriver<H: AcpiPowerSourceHardware = unsupported::UnsupportedAcpiHardware>
{
    marker: PhantomData<fn() -> H>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AcpiPowerSourceDriverContext<
    H: AcpiPowerSourceHardware = unsupported::UnsupportedAcpiHardware,
> {
    marker: PhantomData<fn() -> H>,
}

impl<H> AcpiPowerSourceDriverContext<H>
where
    H: AcpiPowerSourceHardware,
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
    &POWER_SOURCE_DRIVER_METADATA
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AcpiPowerSource<H: AcpiPowerSourceHardware = unsupported::UnsupportedAcpiHardware> {
    provider: u8,
    _hardware: PhantomData<H>,
}

impl<H> AcpiPowerSource<H>
where
    H: AcpiPowerSourceHardware,
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

impl<H> AcpiPowerSourceContract for AcpiPowerSource<H>
where
    H: AcpiPowerSourceHardware,
{
    fn power_sources(&self) -> &'static [AcpiPowerSourceDescriptor] {
        H::power_sources(self.provider)
    }

    fn power_source_support(&self, index: u8) -> Result<AcpiPowerSourceSupport, AcpiError> {
        H::power_source_support(self.provider, index)
    }

    fn power_source_state(&self, index: u8) -> Result<AcpiPowerSourceState, AcpiError> {
        H::power_source_state(self.provider, index)
    }
}

fn enumerate_power_source_bindings<H>(
    _registered: &RegisteredDriver<AcpiPowerSourceDriver<H>>,
    context: &mut DriverDiscoveryContext<'_>,
    out: &mut [AcpiPowerSourceBinding],
) -> Result<usize, DriverError>
where
    H: AcpiPowerSourceHardware + 'static,
{
    let _ = context.downcast_mut::<AcpiPowerSourceDriverContext<H>>()?;
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
        if H::power_sources(provider).is_empty() {
            continue;
        }
        out[written] = AcpiPowerSourceBinding {
            provider,
            provider_id: provider_descriptor.id,
        };
        written += 1;
    }

    Ok(written)
}

fn activate_power_source_binding<H>(
    _registered: &RegisteredDriver<AcpiPowerSourceDriver<H>>,
    context: &mut DriverActivationContext<'_>,
    binding: AcpiPowerSourceBinding,
) -> Result<ActiveDriver<AcpiPowerSourceDriver<H>>, DriverError>
where
    H: AcpiPowerSourceHardware + 'static,
{
    let _ = context.downcast_mut::<AcpiPowerSourceDriverContext<H>>()?;
    let Some(provider_descriptor) = H::provider(binding.provider) else {
        return Err(DriverError::invalid());
    };
    if provider_descriptor.id != binding.provider_id {
        return Err(DriverError::invalid());
    }

    Ok(ActiveDriver::new(
        binding,
        AcpiPowerSource::<H>::new(binding.provider),
    ))
}

impl<H> DriverContract for AcpiPowerSourceDriver<H>
where
    H: AcpiPowerSourceHardware + 'static,
{
    type Binding = AcpiPowerSourceBinding;
    type Instance = AcpiPowerSource<H>;

    fn registration() -> DriverRegistration<Self> {
        DriverRegistration::new(
            driver_metadata,
            DriverActivation::new(
                enumerate_power_source_bindings::<H>,
                activate_power_source_binding::<H>,
            ),
        )
    }
}
