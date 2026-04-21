//! Canonical public ACPI button/switch driver family.

use core::marker::PhantomData;

use crate::contract::drivers::acpi::{
    AcpiButtonContract,
    AcpiButtonDescriptor,
    AcpiButtonState,
    AcpiButtonSupport,
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
    DriverDiscoveryContext,
    DriverError,
    DriverIdentity,
    DriverMetadata,
    DriverRegistration,
    RegisteredDriver,
};

use crate::drivers::acpi::public::interface::contract::AcpiButtonHardware;
use crate::drivers::acpi::public::unsupported;

const BUTTON_DRIVER_BINDING_SOURCES: [DriverBindingSource; 2] =
    [DriverBindingSource::Acpi, DriverBindingSource::Manual];
const BUTTON_DRIVER_METADATA: DriverMetadata = DriverMetadata {
    key: crate::drivers::acpi::public::dogma::BUTTON_DRIVER_DOGMA.key,
    class: DriverClass::Other("acpi"),
    identity: DriverIdentity {
        vendor: "Fusion",
        family: Some("ACPI"),
        package: None,
        product: "button driver",
        advertised_interface: "ACPI button",
    },
    contracts: crate::drivers::acpi::public::dogma::BUTTON_DRIVER_DOGMA.contracts,
    required_contracts: crate::drivers::acpi::public::dogma::BUTTON_DRIVER_DOGMA.required_contracts,
    usefulness: crate::drivers::acpi::public::dogma::BUTTON_DRIVER_DOGMA.usefulness,
    singleton_class: crate::drivers::acpi::public::dogma::BUTTON_DRIVER_DOGMA.singleton_class,
    binding_sources: &BUTTON_DRIVER_BINDING_SOURCES,
    description: "Canonical ACPI button/switch driver layered over one selected ACPI backend",
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AcpiButtonBinding {
    pub provider: u8,
    pub provider_id: &'static str,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AcpiButtonDriver<H: AcpiButtonHardware = unsupported::UnsupportedAcpiHardware> {
    marker: PhantomData<fn() -> H>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AcpiButtonDriverContext<H: AcpiButtonHardware = unsupported::UnsupportedAcpiHardware> {
    marker: PhantomData<fn() -> H>,
}

impl<H> AcpiButtonDriverContext<H>
where
    H: AcpiButtonHardware,
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
    &BUTTON_DRIVER_METADATA
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AcpiButton<H: AcpiButtonHardware = unsupported::UnsupportedAcpiHardware> {
    provider: u8,
    _hardware: PhantomData<H>,
}

impl<H> AcpiButton<H>
where
    H: AcpiButtonHardware,
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

impl<H> AcpiButtonContract for AcpiButton<H>
where
    H: AcpiButtonHardware,
{
    fn buttons(&self) -> &'static [AcpiButtonDescriptor] {
        H::buttons(self.provider)
    }

    fn button_support(&self, index: u8) -> Result<AcpiButtonSupport, AcpiError> {
        H::button_support(self.provider, index)
    }

    fn button_state(&self, index: u8) -> Result<AcpiButtonState, AcpiError> {
        H::button_state(self.provider, index)
    }
}

fn enumerate_button_bindings<H>(
    _registered: &RegisteredDriver<AcpiButtonDriver<H>>,
    context: &mut DriverDiscoveryContext<'_>,
    out: &mut [AcpiButtonBinding],
) -> Result<usize, DriverError>
where
    H: AcpiButtonHardware + 'static,
{
    let _ = context.downcast_mut::<AcpiButtonDriverContext<H>>()?;
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
        if H::buttons(provider).is_empty() {
            continue;
        }
        out[written] = AcpiButtonBinding {
            provider,
            provider_id: provider_descriptor.id,
        };
        written += 1;
    }

    Ok(written)
}

fn activate_button_binding<H>(
    _registered: &RegisteredDriver<AcpiButtonDriver<H>>,
    context: &mut DriverActivationContext<'_>,
    binding: AcpiButtonBinding,
) -> Result<ActiveDriver<AcpiButtonDriver<H>>, DriverError>
where
    H: AcpiButtonHardware + 'static,
{
    let _ = context.downcast_mut::<AcpiButtonDriverContext<H>>()?;
    let Some(provider_descriptor) = H::provider(binding.provider) else {
        return Err(DriverError::invalid());
    };
    if provider_descriptor.id != binding.provider_id {
        return Err(DriverError::invalid());
    }

    Ok(ActiveDriver::new(
        binding,
        AcpiButton::<H>::new(binding.provider),
    ))
}

impl<H> DriverContract for AcpiButtonDriver<H>
where
    H: AcpiButtonHardware + 'static,
{
    type Binding = AcpiButtonBinding;
    type Instance = AcpiButton<H>;

    fn registration() -> DriverRegistration<Self> {
        DriverRegistration::new(
            driver_metadata,
            DriverActivation::new(enumerate_button_bindings::<H>, activate_button_binding::<H>),
        )
    }
}
