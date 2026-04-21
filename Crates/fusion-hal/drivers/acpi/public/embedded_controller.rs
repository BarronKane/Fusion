//! Canonical public ACPI embedded-controller driver family.

use core::marker::PhantomData;

use crate::contract::drivers::acpi::{
    AcpiEmbeddedControllerContract,
    AcpiEmbeddedControllerDescriptor,
    AcpiEmbeddedControllerSupport,
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

use crate::drivers::acpi::public::interface::contract::AcpiEmbeddedControllerHardware;
use crate::drivers::acpi::public::unsupported;

const EC_DRIVER_BINDING_SOURCES: [DriverBindingSource; 2] =
    [DriverBindingSource::Acpi, DriverBindingSource::Manual];
const EC_DRIVER_METADATA: DriverMetadata = DriverMetadata {
    key: crate::drivers::acpi::public::dogma::EC_DRIVER_DOGMA.key,
    class: DriverClass::Other("acpi"),
    identity: DriverIdentity {
        vendor: "Fusion",
        family: Some("ACPI"),
        package: None,
        product: "embedded-controller driver",
        advertised_interface: "ACPI embedded controller",
    },
    contracts: crate::drivers::acpi::public::dogma::EC_DRIVER_DOGMA.contracts,
    required_contracts: crate::drivers::acpi::public::dogma::EC_DRIVER_DOGMA.required_contracts,
    usefulness: crate::drivers::acpi::public::dogma::EC_DRIVER_DOGMA.usefulness,
    singleton_class: crate::drivers::acpi::public::dogma::EC_DRIVER_DOGMA.singleton_class,
    binding_sources: &EC_DRIVER_BINDING_SOURCES,
    description: "Canonical ACPI embedded-controller driver layered over one selected ACPI backend",
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AcpiEmbeddedControllerBinding {
    pub provider: u8,
    pub provider_id: &'static str,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AcpiEmbeddedControllerDriver<
    H: AcpiEmbeddedControllerHardware = unsupported::UnsupportedAcpiHardware,
> {
    marker: PhantomData<fn() -> H>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AcpiEmbeddedControllerDriverContext<
    H: AcpiEmbeddedControllerHardware = unsupported::UnsupportedAcpiHardware,
> {
    marker: PhantomData<fn() -> H>,
}

impl<H> AcpiEmbeddedControllerDriverContext<H>
where
    H: AcpiEmbeddedControllerHardware,
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
    &EC_DRIVER_METADATA
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AcpiEmbeddedController<
    H: AcpiEmbeddedControllerHardware = unsupported::UnsupportedAcpiHardware,
> {
    provider: u8,
    _hardware: PhantomData<H>,
}

impl<H> AcpiEmbeddedController<H>
where
    H: AcpiEmbeddedControllerHardware,
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

impl<H> AcpiEmbeddedControllerContract for AcpiEmbeddedController<H>
where
    H: AcpiEmbeddedControllerHardware,
{
    fn embedded_controllers(&self) -> &'static [AcpiEmbeddedControllerDescriptor] {
        H::embedded_controllers(self.provider)
    }

    fn embedded_controller_support(
        &self,
        index: u8,
    ) -> Result<AcpiEmbeddedControllerSupport, AcpiError> {
        H::embedded_controller_support(self.provider, index)
    }

    fn embedded_controller_read(&self, index: u8, register: u8) -> Result<u8, AcpiError> {
        H::embedded_controller_read(self.provider, index, register)
    }

    fn embedded_controller_write(
        &mut self,
        index: u8,
        register: u8,
        value: u8,
    ) -> Result<(), AcpiError> {
        H::embedded_controller_write(self.provider, index, register, value)
    }
}

fn enumerate_ec_bindings<H>(
    _registered: &RegisteredDriver<AcpiEmbeddedControllerDriver<H>>,
    context: &mut DriverDiscoveryContext<'_>,
    out: &mut [AcpiEmbeddedControllerBinding],
) -> Result<usize, DriverError>
where
    H: AcpiEmbeddedControllerHardware + 'static,
{
    let _ = context.downcast_mut::<AcpiEmbeddedControllerDriverContext<H>>()?;
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
        if H::embedded_controllers(provider).is_empty() {
            continue;
        }
        out[written] = AcpiEmbeddedControllerBinding {
            provider,
            provider_id: provider_descriptor.id,
        };
        written += 1;
    }

    Ok(written)
}

fn activate_ec_binding<H>(
    _registered: &RegisteredDriver<AcpiEmbeddedControllerDriver<H>>,
    context: &mut DriverActivationContext<'_>,
    binding: AcpiEmbeddedControllerBinding,
) -> Result<ActiveDriver<AcpiEmbeddedControllerDriver<H>>, DriverError>
where
    H: AcpiEmbeddedControllerHardware + 'static,
{
    let _ = context.downcast_mut::<AcpiEmbeddedControllerDriverContext<H>>()?;
    let Some(provider_descriptor) = H::provider(binding.provider) else {
        return Err(DriverError::invalid());
    };
    if provider_descriptor.id != binding.provider_id {
        return Err(DriverError::invalid());
    }

    Ok(ActiveDriver::new(
        binding,
        AcpiEmbeddedController::<H>::new(binding.provider),
    ))
}

impl<H> DriverContract for AcpiEmbeddedControllerDriver<H>
where
    H: AcpiEmbeddedControllerHardware + 'static,
{
    type Binding = AcpiEmbeddedControllerBinding;
    type Instance = AcpiEmbeddedController<H>;

    fn registration() -> DriverRegistration<Self> {
        DriverRegistration::new(
            driver_metadata,
            DriverActivation::new(enumerate_ec_bindings::<H>, activate_ec_binding::<H>),
        )
    }
}
