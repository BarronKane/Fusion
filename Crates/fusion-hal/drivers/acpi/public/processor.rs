//! Canonical public ACPI processor driver family.

use core::marker::PhantomData;

use crate::contract::drivers::acpi::{
    AcpiError,
    AcpiProcessorContract,
    AcpiProcessorDescriptor,
    AcpiProcessorState,
    AcpiProcessorSupport,
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

use crate::drivers::acpi::public::interface::contract::AcpiProcessorHardware;
use crate::drivers::acpi::public::unsupported;

const PROCESSOR_DRIVER_CONTRACTS: [DriverContractKey; 1] = [DriverContractKey("acpi.processor")];
const PROCESSOR_DRIVER_BINDING_SOURCES: [DriverBindingSource; 2] =
    [DriverBindingSource::Acpi, DriverBindingSource::Manual];
const PROCESSOR_DRIVER_METADATA: DriverMetadata = DriverMetadata {
    key: "acpi.processor",
    class: DriverClass::Other("acpi"),
    identity: DriverIdentity {
        vendor: "Fusion",
        family: Some("ACPI"),
        package: None,
        product: "processor driver",
        advertised_interface: "ACPI processor",
    },
    contracts: &PROCESSOR_DRIVER_CONTRACTS,
    binding_sources: &PROCESSOR_DRIVER_BINDING_SOURCES,
    description: "Canonical ACPI processor driver layered over one selected ACPI backend",
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AcpiProcessorBinding {
    pub provider: u8,
    pub provider_id: &'static str,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AcpiProcessorDriver<H: AcpiProcessorHardware = unsupported::UnsupportedAcpiHardware> {
    marker: PhantomData<fn() -> H>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AcpiProcessorDriverContext<
    H: AcpiProcessorHardware = unsupported::UnsupportedAcpiHardware,
> {
    marker: PhantomData<fn() -> H>,
}

impl<H> AcpiProcessorDriverContext<H>
where
    H: AcpiProcessorHardware,
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
    &PROCESSOR_DRIVER_METADATA
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AcpiProcessor<H: AcpiProcessorHardware = unsupported::UnsupportedAcpiHardware> {
    provider: u8,
    _hardware: PhantomData<H>,
}

impl<H> AcpiProcessor<H>
where
    H: AcpiProcessorHardware,
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

impl<H> AcpiProcessorContract for AcpiProcessor<H>
where
    H: AcpiProcessorHardware,
{
    fn processors(&self) -> &'static [AcpiProcessorDescriptor] {
        H::processors(self.provider)
    }

    fn processor_support(&self, index: u8) -> Result<AcpiProcessorSupport, AcpiError> {
        H::processor_support(self.provider, index)
    }

    fn processor_state(&self, index: u8) -> Result<AcpiProcessorState, AcpiError> {
        H::processor_state(self.provider, index)
    }
}

fn enumerate_processor_bindings<H>(
    _registered: &RegisteredDriver<AcpiProcessorDriver<H>>,
    context: &mut DriverDiscoveryContext<'_>,
    out: &mut [AcpiProcessorBinding],
) -> Result<usize, DriverError>
where
    H: AcpiProcessorHardware + 'static,
{
    let _ = context.downcast_mut::<AcpiProcessorDriverContext<H>>()?;
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
        if H::processors(provider).is_empty() {
            continue;
        }
        out[written] = AcpiProcessorBinding {
            provider,
            provider_id: provider_descriptor.id,
        };
        written += 1;
    }

    Ok(written)
}

fn activate_processor_binding<H>(
    _registered: &RegisteredDriver<AcpiProcessorDriver<H>>,
    context: &mut DriverActivationContext<'_>,
    binding: AcpiProcessorBinding,
) -> Result<ActiveDriver<AcpiProcessorDriver<H>>, DriverError>
where
    H: AcpiProcessorHardware + 'static,
{
    let _ = context.downcast_mut::<AcpiProcessorDriverContext<H>>()?;
    let Some(provider_descriptor) = H::provider(binding.provider) else {
        return Err(DriverError::invalid());
    };
    if provider_descriptor.id != binding.provider_id {
        return Err(DriverError::invalid());
    }

    Ok(ActiveDriver::new(
        binding,
        AcpiProcessor::<H>::new(binding.provider),
    ))
}

impl<H> DriverContract for AcpiProcessorDriver<H>
where
    H: AcpiProcessorHardware + 'static,
{
    type Binding = AcpiProcessorBinding;
    type Instance = AcpiProcessor<H>;

    fn registration() -> DriverRegistration<Self> {
        DriverRegistration::new(
            driver_metadata,
            DriverActivation::new(
                enumerate_processor_bindings::<H>,
                activate_processor_binding::<H>,
            ),
        )
    }
}
