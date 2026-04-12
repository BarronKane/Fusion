//! Canonical public ACPI lid driver family.

use core::marker::PhantomData;

use crate::contract::drivers::acpi::{
    AcpiError,
    AcpiLidContract,
    AcpiLidDescriptor,
    AcpiLidState,
    AcpiLidSupport,
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

use crate::drivers::acpi::public::interface::contract::AcpiLidHardware;
use crate::drivers::acpi::public::unsupported;

const LID_DRIVER_CONTRACTS: [DriverContractKey; 1] = [DriverContractKey("acpi.lid")];
const LID_DRIVER_BINDING_SOURCES: [DriverBindingSource; 2] =
    [DriverBindingSource::Acpi, DriverBindingSource::Manual];
const LID_DRIVER_METADATA: DriverMetadata = DriverMetadata {
    key: "acpi.lid",
    class: DriverClass::Other("acpi"),
    identity: DriverIdentity {
        vendor: "Fusion",
        family: Some("ACPI"),
        package: None,
        product: "lid driver",
        advertised_interface: "ACPI lid",
    },
    contracts: &LID_DRIVER_CONTRACTS,
    binding_sources: &LID_DRIVER_BINDING_SOURCES,
    description: "Canonical ACPI lid driver layered over one selected ACPI backend",
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AcpiLidBinding {
    pub provider: u8,
    pub provider_id: &'static str,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AcpiLidDriver<H: AcpiLidHardware = unsupported::UnsupportedAcpiHardware> {
    marker: PhantomData<fn() -> H>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AcpiLidDriverContext<H: AcpiLidHardware = unsupported::UnsupportedAcpiHardware> {
    marker: PhantomData<fn() -> H>,
}

impl<H> AcpiLidDriverContext<H>
where
    H: AcpiLidHardware,
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
    &LID_DRIVER_METADATA
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AcpiLid<H: AcpiLidHardware = unsupported::UnsupportedAcpiHardware> {
    provider: u8,
    _hardware: PhantomData<H>,
}

impl<H> AcpiLid<H>
where
    H: AcpiLidHardware,
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

impl<H> AcpiLidContract for AcpiLid<H>
where
    H: AcpiLidHardware,
{
    fn lids(&self) -> &'static [AcpiLidDescriptor] {
        H::lids(self.provider)
    }

    fn lid_support(&self, index: u8) -> Result<AcpiLidSupport, AcpiError> {
        H::lid_support(self.provider, index)
    }

    fn lid_state(&self, index: u8) -> Result<AcpiLidState, AcpiError> {
        H::lid_state(self.provider, index)
    }
}

fn enumerate_lid_bindings<H>(
    _registered: &RegisteredDriver<AcpiLidDriver<H>>,
    context: &mut DriverDiscoveryContext<'_>,
    out: &mut [AcpiLidBinding],
) -> Result<usize, DriverError>
where
    H: AcpiLidHardware + 'static,
{
    let _ = context.downcast_mut::<AcpiLidDriverContext<H>>()?;
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
        if H::lids(provider).is_empty() {
            continue;
        }
        out[written] = AcpiLidBinding {
            provider,
            provider_id: provider_descriptor.id,
        };
        written += 1;
    }

    Ok(written)
}

fn activate_lid_binding<H>(
    _registered: &RegisteredDriver<AcpiLidDriver<H>>,
    context: &mut DriverActivationContext<'_>,
    binding: AcpiLidBinding,
) -> Result<ActiveDriver<AcpiLidDriver<H>>, DriverError>
where
    H: AcpiLidHardware + 'static,
{
    let _ = context.downcast_mut::<AcpiLidDriverContext<H>>()?;
    let Some(provider_descriptor) = H::provider(binding.provider) else {
        return Err(DriverError::invalid());
    };
    if provider_descriptor.id != binding.provider_id {
        return Err(DriverError::invalid());
    }

    Ok(ActiveDriver::new(
        binding,
        AcpiLid::<H>::new(binding.provider),
    ))
}

impl<H> DriverContract for AcpiLidDriver<H>
where
    H: AcpiLidHardware + 'static,
{
    type Binding = AcpiLidBinding;
    type Instance = AcpiLid<H>;

    fn registration() -> DriverRegistration<Self> {
        DriverRegistration::new(
            driver_metadata,
            DriverActivation::new(enumerate_lid_bindings::<H>, activate_lid_binding::<H>),
        )
    }
}
