//! Canonical public display-layout driver family.

#![cfg_attr(not(feature = "std"), no_std)]

use core::marker::PhantomData;

use fusion_hal::contract::drivers::display::{
    DisplayControlContract,
    DisplayLayoutConfig,
    DisplayLayoutContract,
    DisplayLayoutPresentReport,
    DisplayLayoutPresentRequest,
    DisplayLayoutState,
    DisplayLayoutValidationError,
    DisplayOutputDescriptor,
    DisplayOutputId,
    DisplayResult,
    DisplaySurfaceId,
    DisplaySurfacePlacement,
};
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

#[cfg(any(target_os = "none", feature = "fdxe-module"))]
mod fdxe;
#[path = "interface/interface.rs"]
pub mod interface;
mod unsupported;

const DISPLAY_LAYOUT_DRIVER_CONTRACTS: [DriverContractKey; 1] =
    [DriverContractKey("display.layout")];
const DISPLAY_LAYOUT_DRIVER_REQUIRED_CONTRACTS: [DriverContractKey; 0] = [];
const DISPLAY_LAYOUT_DRIVER_BINDING_SOURCES: [DriverBindingSource; 5] = [
    DriverBindingSource::StaticSoc,
    DriverBindingSource::BoardManifest,
    DriverBindingSource::Acpi,
    DriverBindingSource::Devicetree,
    DriverBindingSource::Manual,
];
const DISPLAY_LAYOUT_DRIVER_METADATA: DriverMetadata = DriverMetadata {
    key: "display.layout",
    class: DriverClass::Display,
    identity: DriverIdentity {
        vendor: "Fusion",
        family: Some("Display"),
        package: None,
        product: "layout driver",
        advertised_interface: "machine display composition",
    },
    contracts: &DISPLAY_LAYOUT_DRIVER_CONTRACTS,
    required_contracts: &DISPLAY_LAYOUT_DRIVER_REQUIRED_CONTRACTS,
    usefulness: DriverUsefulness::MustBeConsumed,
    singleton_class: Some("display.layout.machine"),
    binding_sources: &DISPLAY_LAYOUT_DRIVER_BINDING_SOURCES,
    description: "Canonical display-layout driver layered over one selected machine display substrate",
};

/// Discoverable machine-display layout binding surfaced by the canonical layout driver family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DisplayLayoutBinding {
    pub layout: u8,
    pub layout_id: &'static str,
}

/// Composition-facing machine-display backend seam consumed by the canonical layout driver family.
pub trait DisplayLayoutBackend {
    type Control<'a>: DisplayControlContract
    where
        Self: 'a;

    fn layout_count() -> u8;
    fn layout_id(layout: u8) -> Option<&'static str>;
    fn enumerate_outputs(layout: u8, out: &mut [DisplayOutputId]) -> DisplayResult<usize>;
    fn output_descriptor(
        layout: u8,
        id: DisplayOutputId,
    ) -> DisplayResult<Option<DisplayOutputDescriptor>>;
    fn layout_state(layout: u8) -> DisplayResult<DisplayLayoutState>;
    fn validate_layout(
        layout: u8,
        config: &DisplayLayoutConfig<'_>,
    ) -> Result<(), DisplayLayoutValidationError>;
    fn apply_layout(layout: u8, config: &DisplayLayoutConfig<'_>) -> DisplayResult<()>;
    fn primary_output(layout: u8) -> DisplayResult<Option<DisplayOutputId>>;
    fn set_primary_output(layout: u8, output: Option<DisplayOutputId>) -> DisplayResult<()>;
    fn control<'a>(layout: u8, id: DisplayOutputId) -> DisplayResult<Option<Self::Control<'a>>>;
    fn control_mut<'a>(layout: u8, id: DisplayOutputId)
    -> DisplayResult<Option<Self::Control<'a>>>;
    fn place_surface(
        layout: u8,
        surface: DisplaySurfaceId,
        placement: &DisplaySurfacePlacement,
    ) -> DisplayResult<()>;
    fn present_layout(
        layout: u8,
        request: &DisplayLayoutPresentRequest<'_>,
    ) -> DisplayResult<DisplayLayoutPresentReport>;
}

/// Registerable canonical display-layout driver family marker.
#[derive(Debug, Clone, Copy, Default)]
pub struct DisplayLayoutDriver<
    H: DisplayLayoutBackend = unsupported::UnsupportedDisplayLayoutHardware,
> {
    marker: PhantomData<fn() -> H>,
}

/// One-shot discovery/activation context for the canonical display-layout driver family.
#[derive(Debug, Clone, Copy, Default)]
pub struct DisplayLayoutDriverContext<
    H: DisplayLayoutBackend = unsupported::UnsupportedDisplayLayoutHardware,
> {
    marker: PhantomData<fn() -> H>,
}

impl<H> DisplayLayoutDriverContext<H>
where
    H: DisplayLayoutBackend,
{
    #[must_use]
    pub const fn new() -> Self {
        Self {
            marker: PhantomData,
        }
    }
}

/// Returns truthful static metadata for the canonical display-layout driver family.
#[must_use]
pub const fn driver_metadata() -> &'static DriverMetadata {
    &DISPLAY_LAYOUT_DRIVER_METADATA
}

/// Canonical machine-display composition surface over one selected layout backend.
#[derive(Debug, Clone, Copy, Default)]
pub struct DisplayLayout<H: DisplayLayoutBackend = unsupported::UnsupportedDisplayLayoutHardware> {
    layout: u8,
    _hardware: PhantomData<H>,
}

impl<H> DisplayLayout<H>
where
    H: DisplayLayoutBackend,
{
    #[must_use]
    pub const fn new(layout: u8) -> Self {
        Self {
            layout,
            _hardware: PhantomData,
        }
    }
}

impl<H> DisplayLayoutContract for DisplayLayout<H>
where
    H: DisplayLayoutBackend,
{
    type Control<'a>
        = H::Control<'a>
    where
        Self: 'a;

    fn enumerate_outputs(&self, out: &mut [DisplayOutputId]) -> DisplayResult<usize> {
        H::enumerate_outputs(self.layout, out)
    }

    fn output_descriptor(
        &self,
        id: DisplayOutputId,
    ) -> DisplayResult<Option<DisplayOutputDescriptor>> {
        H::output_descriptor(self.layout, id)
    }

    fn layout_state(&self) -> DisplayResult<DisplayLayoutState> {
        H::layout_state(self.layout)
    }

    fn validate_layout(
        &self,
        layout: &DisplayLayoutConfig<'_>,
    ) -> Result<(), DisplayLayoutValidationError> {
        H::validate_layout(self.layout, layout)
    }

    fn apply_layout(&mut self, layout: &DisplayLayoutConfig<'_>) -> DisplayResult<()> {
        H::apply_layout(self.layout, layout)
    }

    fn primary_output(&self) -> DisplayResult<Option<DisplayOutputId>> {
        H::primary_output(self.layout)
    }

    fn set_primary_output(&mut self, output: Option<DisplayOutputId>) -> DisplayResult<()> {
        H::set_primary_output(self.layout, output)
    }

    fn control(&self, id: DisplayOutputId) -> DisplayResult<Option<Self::Control<'_>>> {
        H::control(self.layout, id)
    }

    fn control_mut(&mut self, id: DisplayOutputId) -> DisplayResult<Option<Self::Control<'_>>> {
        H::control_mut(self.layout, id)
    }

    fn place_surface(
        &mut self,
        surface: DisplaySurfaceId,
        placement: &DisplaySurfacePlacement,
    ) -> DisplayResult<()> {
        H::place_surface(self.layout, surface, placement)
    }

    fn present_layout(
        &mut self,
        request: &DisplayLayoutPresentRequest<'_>,
    ) -> DisplayResult<DisplayLayoutPresentReport> {
        H::present_layout(self.layout, request)
    }
}

fn enumerate_layout_bindings<H>(
    _registered: &RegisteredDriver<DisplayLayoutDriver<H>>,
    context: &mut DriverDiscoveryContext<'_>,
    out: &mut [DisplayLayoutBinding],
) -> Result<usize, DriverError>
where
    H: DisplayLayoutBackend + 'static,
{
    let _ = context.downcast_mut::<DisplayLayoutDriverContext<H>>()?;
    if out.is_empty() {
        return Err(DriverError::resource_exhausted());
    }

    let mut written = 0;
    for layout in 0..H::layout_count() {
        if written == out.len() {
            return Err(DriverError::resource_exhausted());
        }
        let Some(layout_id) = H::layout_id(layout) else {
            continue;
        };
        out[written] = DisplayLayoutBinding { layout, layout_id };
        written += 1;
    }

    Ok(written)
}

fn activate_layout_binding<H>(
    _registered: &RegisteredDriver<DisplayLayoutDriver<H>>,
    context: &mut DriverActivationContext<'_>,
    binding: DisplayLayoutBinding,
) -> Result<ActiveDriver<DisplayLayoutDriver<H>>, DriverError>
where
    H: DisplayLayoutBackend + 'static,
{
    let _ = context.downcast_mut::<DisplayLayoutDriverContext<H>>()?;
    let Some(layout_id) = H::layout_id(binding.layout) else {
        return Err(DriverError::invalid());
    };
    if layout_id != binding.layout_id {
        return Err(DriverError::invalid());
    }

    Ok(ActiveDriver::new(
        binding,
        DisplayLayout::<H>::new(binding.layout),
    ))
}

impl<H> DriverContract for DisplayLayoutDriver<H>
where
    H: DisplayLayoutBackend + 'static,
{
    type Binding = DisplayLayoutBinding;
    type Instance = DisplayLayout<H>;

    fn registration() -> DriverRegistration<Self> {
        DriverRegistration::new(
            driver_metadata,
            DriverActivation::new(enumerate_layout_bindings::<H>, activate_layout_binding::<H>),
        )
    }
}
