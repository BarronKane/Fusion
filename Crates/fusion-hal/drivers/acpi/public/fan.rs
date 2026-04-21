//! Canonical public ACPI fan driver family.

use core::marker::PhantomData;

use crate::contract::drivers::acpi::{
    AcpiError,
    AcpiFanContract,
    AcpiFanDescriptor,
    AcpiFanState,
    AcpiFanSupport,
    AcpiProviderDescriptor,
};
use crate::contract::drivers::driver::{
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

use crate::drivers::acpi::public::interface::contract::AcpiFanHardware;
use crate::drivers::acpi::public::unsupported;

const FAN_DRIVER_BINDING_SOURCES: [DriverBindingSource; 2] =
    [DriverBindingSource::Acpi, DriverBindingSource::Manual];
const FAN_DRIVER_METADATA: DriverMetadata = DriverMetadata {
    key: crate::drivers::acpi::public::dogma::FAN_DRIVER_DOGMA.key,
    class: DriverClass::Other("acpi"),
    identity: DriverIdentity {
        vendor: "Fusion",
        family: Some("ACPI"),
        package: None,
        product: "fan driver",
        advertised_interface: "ACPI fan",
    },
    contracts: crate::drivers::acpi::public::dogma::FAN_DRIVER_DOGMA.contracts,
    required_contracts: crate::drivers::acpi::public::dogma::FAN_DRIVER_DOGMA.required_contracts,
    usefulness: crate::drivers::acpi::public::dogma::FAN_DRIVER_DOGMA.usefulness,
    singleton_class: crate::drivers::acpi::public::dogma::FAN_DRIVER_DOGMA.singleton_class,
    binding_sources: &FAN_DRIVER_BINDING_SOURCES,
    description: "Canonical ACPI fan driver layered over one selected ACPI backend",
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AcpiFanBinding {
    pub provider: u8,
    pub provider_id: &'static str,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AcpiFanDriver<H: AcpiFanHardware = unsupported::UnsupportedAcpiHardware> {
    marker: PhantomData<fn() -> H>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AcpiFanDriverContext<H: AcpiFanHardware = unsupported::UnsupportedAcpiHardware> {
    marker: PhantomData<fn() -> H>,
}

impl<H> AcpiFanDriverContext<H>
where
    H: AcpiFanHardware,
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
    &FAN_DRIVER_METADATA
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AcpiFan<H: AcpiFanHardware = unsupported::UnsupportedAcpiHardware> {
    provider: u8,
    _hardware: PhantomData<H>,
}

impl<H> AcpiFan<H>
where
    H: AcpiFanHardware,
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

impl<H> AcpiFanContract for AcpiFan<H>
where
    H: AcpiFanHardware,
{
    fn fans(&self) -> &'static [AcpiFanDescriptor] {
        H::fans(self.provider)
    }

    fn fan_support(&self, index: u8) -> Result<AcpiFanSupport, AcpiError> {
        H::fan_support(self.provider, index)
    }

    fn fan_state(&self, index: u8) -> Result<AcpiFanState, AcpiError> {
        H::fan_state(self.provider, index)
    }
}

fn enumerate_fan_bindings<H>(
    _registered: &RegisteredDriver<AcpiFanDriver<H>>,
    context: &mut DriverDiscoveryContext<'_>,
    out: &mut [AcpiFanBinding],
) -> Result<usize, DriverError>
where
    H: AcpiFanHardware + 'static,
{
    let _ = context.downcast_mut::<AcpiFanDriverContext<H>>()?;
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
        if H::fans(provider).is_empty() {
            continue;
        }
        out[written] = AcpiFanBinding {
            provider,
            provider_id: provider_descriptor.id,
        };
        written += 1;
    }

    Ok(written)
}

fn activate_fan_binding<H>(
    _registered: &RegisteredDriver<AcpiFanDriver<H>>,
    context: &mut DriverActivationContext<'_>,
    binding: AcpiFanBinding,
) -> Result<ActiveDriver<AcpiFanDriver<H>>, DriverError>
where
    H: AcpiFanHardware + 'static,
{
    let _ = context.downcast_mut::<AcpiFanDriverContext<H>>()?;
    let Some(provider_descriptor) = H::provider(binding.provider) else {
        return Err(DriverError::invalid());
    };
    if provider_descriptor.id != binding.provider_id {
        return Err(DriverError::invalid());
    }

    Ok(ActiveDriver::new(
        binding,
        AcpiFan::<H>::new(binding.provider),
    ))
}

impl<H> DriverContract for AcpiFanDriver<H>
where
    H: AcpiFanHardware + 'static,
{
    type Binding = AcpiFanBinding;
    type Instance = AcpiFan<H>;

    fn registration() -> DriverRegistration<Self> {
        DriverRegistration::new(
            driver_metadata,
            DriverActivation::new(enumerate_fan_bindings::<H>, activate_fan_binding::<H>),
        )
    }
}
