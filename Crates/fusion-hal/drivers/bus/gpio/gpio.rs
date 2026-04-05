//! Universal GPIO driver crate layered over one hardware-facing GPIO substrate.

#![cfg_attr(not(feature = "std"), no_std)]

use core::marker::PhantomData;

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
    RegisteredDriver,
};

pub use fusion_hal::contract::drivers::bus::gpio::*;

#[cfg(any(target_os = "none", feature = "fdxe-module"))]
mod fdxe;
#[path = "interface/interface.rs"]
pub mod interface;
mod unsupported;

use self::interface::contract::{
    GpioHardware,
    GpioHardwarePin,
};

const GPIO_DRIVER_CONTRACTS: [DriverContractKey; 1] = [DriverContractKey("bus.gpio")];
const GPIO_DRIVER_BINDING_SOURCES: [DriverBindingSource; 5] = [
    DriverBindingSource::StaticSoc,
    DriverBindingSource::BoardManifest,
    DriverBindingSource::Acpi,
    DriverBindingSource::Devicetree,
    DriverBindingSource::Manual,
];
const GPIO_DRIVER_METADATA: DriverMetadata = DriverMetadata {
    key: "bus.gpio",
    class: DriverClass::Bus,
    identity: DriverIdentity {
        vendor: "Fusion",
        family: Some("Generic"),
        package: None,
        product: "GPIO driver",
        advertised_interface: "GPIO",
    },
    contracts: &GPIO_DRIVER_CONTRACTS,
    binding_sources: &GPIO_DRIVER_BINDING_SOURCES,
    description: "Universal GPIO provider driver layered over one selected hardware substrate",
};

/// Discoverable GPIO provider binding surfaced by the universal GPIO driver family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GpioBinding {
    pub provider: u8,
}

/// Registerable universal GPIO driver family marker.
#[derive(Debug, Clone, Copy, Default)]
pub struct GpioDriver<H: GpioHardware = unsupported::UnsupportedGpioHardware> {
    marker: PhantomData<fn() -> H>,
}

/// One-shot driver discovery/activation context for the universal GPIO provider.
#[derive(Debug, Clone, Copy, Default)]
pub struct GpioDriverContext<H: GpioHardware = unsupported::UnsupportedGpioHardware> {
    marker: PhantomData<fn() -> H>,
}

impl<H> GpioDriverContext<H>
where
    H: GpioHardware,
{
    /// Creates one empty GPIO driver context.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            marker: PhantomData,
        }
    }
}

/// Returns the truthful static metadata for the universal GPIO driver family.
#[must_use]
pub const fn driver_metadata() -> &'static DriverMetadata {
    &GPIO_DRIVER_METADATA
}

/// Universal GPIO provider composed over one selected hardware-facing GPIO substrate.
#[derive(Debug, Clone, Copy, Default)]
pub struct Gpio<H: GpioHardware = unsupported::UnsupportedGpioHardware> {
    _hardware: PhantomData<H>,
}

/// Universal owned GPIO pin composed over one selected hardware-facing GPIO substrate.
#[derive(Debug)]
pub struct GpioPin<P: GpioHardwarePin = unsupported::UnsupportedGpioPinHardware> {
    inner: P,
}

impl<H> Gpio<H>
where
    H: GpioHardware,
{
    /// Creates a new universal GPIO provider handle over one selected hardware substrate.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            _hardware: PhantomData,
        }
    }

    /// Takes one pin exclusively.
    ///
    /// # Errors
    ///
    /// Returns one honest error when the pin is invalid, unsupported, or already claimed.
    pub fn take(pin: u8) -> Result<GpioPin<H::Pin>, GpioError> {
        Ok(GpioPin {
            inner: H::claim_pin(pin)?,
        })
    }

    /// Returns one truthful capability snapshot for one pin number.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when the pin does not exist.
    pub fn capabilities(pin: u8) -> Result<GpioCapabilities, GpioError> {
        GpioBaseContract::capabilities(&Self::new(), pin)
    }

    /// Returns the statically or dynamically surfaced GPIO pin catalog.
    #[must_use]
    pub fn pins() -> &'static [GpioPinDescriptor] {
        H::pins()
    }
}

impl<P> GpioPin<P>
where
    P: GpioHardwarePin,
{
    /// Wraps one already-owned hardware-facing GPIO pin.
    #[must_use]
    pub fn from_inner(inner: P) -> Self {
        Self { inner }
    }

    /// Returns the concrete pin number.
    #[must_use]
    pub fn pin(&self) -> u8 {
        self.inner.pin()
    }

    /// Returns one truthful capability snapshot for this owned pin.
    #[must_use]
    pub fn capabilities(&self) -> GpioCapabilities {
        self.inner.capabilities()
    }

    /// Selects one alternate-function mux setting for this pin.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when the function cannot be selected.
    pub fn set_function(&mut self, function: GpioFunction) -> Result<(), GpioError> {
        self.inner.set_function(function)
    }

    /// Configures this pin for software-controlled input sampling.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when input mode cannot be realized.
    pub fn configure_input(&mut self) -> Result<(), GpioError> {
        self.inner.configure_input()
    }

    /// Reads the current sampled input level.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when the pin cannot be read.
    pub fn read(&self) -> Result<bool, GpioError> {
        self.inner.read_level()
    }

    /// Configures this pin for software-controlled output.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when output mode cannot be realized.
    pub fn configure_output(&mut self, initial_high: bool) -> Result<(), GpioError> {
        self.inner.configure_output(initial_high)
    }

    /// Sets the logical output level.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when the pin cannot be driven.
    pub fn set_level(&mut self, high: bool) -> Result<(), GpioError> {
        self.inner.set_level(high)
    }

    /// Selects the pull-resistor mode for this pin.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when pull control is unsupported or invalid.
    pub fn set_pull(&mut self, pull: GpioPull) -> Result<(), GpioError> {
        self.inner.set_pull(pull)
    }

    /// Selects one drive-strength mode for this pin.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when drive-strength control is unsupported or invalid.
    pub fn set_drive_strength(&mut self, strength: GpioDriveStrength) -> Result<(), GpioError> {
        self.inner.set_drive_strength(strength)
    }

    /// Releases the hardware-facing pin handle back to the caller.
    #[must_use]
    pub fn into_inner(self) -> P {
        self.inner
    }
}

impl<H> GpioBaseContract for Gpio<H>
where
    H: GpioHardware,
{
    fn support(&self) -> GpioSupport {
        H::support()
    }

    fn pins(&self) -> &'static [GpioPinDescriptor] {
        H::pins()
    }
}

impl<H> GpioControlContract for Gpio<H>
where
    H: GpioHardware,
{
    type Pin = GpioPin<H::Pin>;

    fn take_pin(&self, pin: u8) -> Result<Self::Pin, GpioError> {
        Self::take(pin)
    }
}

impl<P> GpioOwnedPinContract for GpioPin<P>
where
    P: GpioHardwarePin,
{
    fn pin(&self) -> u8 {
        self.pin()
    }

    fn capabilities(&self) -> GpioCapabilities {
        self.capabilities()
    }
}

impl<P> GpioFunctionPinContract for GpioPin<P>
where
    P: GpioHardwarePin,
{
    fn set_function(&mut self, function: GpioFunction) -> Result<(), GpioError> {
        self.set_function(function)
    }
}

impl<P> GpioPullPinContract for GpioPin<P>
where
    P: GpioHardwarePin,
{
    fn set_pull(&mut self, pull: GpioPull) -> Result<(), GpioError> {
        self.set_pull(pull)
    }
}

impl<P> GpioDriveStrengthPinContract for GpioPin<P>
where
    P: GpioHardwarePin,
{
    fn set_drive_strength(&mut self, strength: GpioDriveStrength) -> Result<(), GpioError> {
        self.set_drive_strength(strength)
    }
}

impl<P> GpioOutputPinContract for GpioPin<P>
where
    P: GpioHardwarePin,
{
    fn configure_output(&mut self, initial_high: bool) -> Result<(), GpioError> {
        self.configure_output(initial_high)
    }

    fn set_level(&mut self, high: bool) -> Result<(), GpioError> {
        self.set_level(high)
    }
}

impl<P> GpioInputPinContract for GpioPin<P>
where
    P: GpioHardwarePin,
{
    fn configure_input(&mut self) -> Result<(), GpioError> {
        self.configure_input()
    }

    fn read_level(&self) -> Result<bool, GpioError> {
        self.read()
    }
}

fn enumerate_gpio_bindings<H>(
    _registered: &RegisteredDriver<GpioDriver<H>>,
    context: &mut DriverDiscoveryContext<'_>,
    out: &mut [GpioBinding],
) -> Result<usize, DriverError>
where
    H: GpioHardware + 'static,
{
    let _ = context.downcast_mut::<GpioDriverContext<H>>()?;
    if out.is_empty() {
        return Err(DriverError::resource_exhausted());
    }

    let support = H::support();
    if support.implementation == GpioImplementationKind::Unsupported
        || support.caps.is_empty()
        || support.pin_count == 0
    {
        return Ok(0);
    }

    out[0] = GpioBinding { provider: 0 };
    Ok(1)
}

fn activate_gpio_binding<H>(
    _registered: &RegisteredDriver<GpioDriver<H>>,
    context: &mut DriverActivationContext<'_>,
    binding: GpioBinding,
) -> Result<ActiveDriver<GpioDriver<H>>, DriverError>
where
    H: GpioHardware + 'static,
{
    let _ = context.downcast_mut::<GpioDriverContext<H>>()?;
    if binding.provider != 0 {
        return Err(DriverError::invalid());
    }

    Ok(ActiveDriver::new(binding, Gpio::<H>::new()))
}

impl<H> DriverContract for GpioDriver<H>
where
    H: GpioHardware + 'static,
{
    type Binding = GpioBinding;
    type Instance = Gpio<H>;

    fn registration() -> DriverRegistration<Self> {
        DriverRegistration::new(
            driver_metadata,
            DriverActivation::new(enumerate_gpio_bindings::<H>, activate_gpio_binding::<H>),
        )
    }
}
