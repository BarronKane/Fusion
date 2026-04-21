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
    DriverDiscoveryContext,
    DriverError,
    DriverIdentity,
    DriverMetadata,
    DriverRegistration,
    RegisteredDriver,
};
pub(crate) use fusion_hal::contract::drivers::driver::{
    DriverContractKey,
    DriverDogma,
    DriverUsefulness,
};

pub use fusion_hal::contract::drivers::bus::gpio::*;

mod dogma;
#[cfg(any(target_os = "none", feature = "fdxe-module"))]
mod fdxe;
#[path = "interface/interface.rs"]
pub mod interface;
mod unsupported;

use self::interface::contract::{
    GpioHardware,
    GpioHardwarePin,
};

const GPIO_DRIVER_BINDING_SOURCES: [DriverBindingSource; 5] = [
    DriverBindingSource::StaticSoc,
    DriverBindingSource::BoardManifest,
    DriverBindingSource::Acpi,
    DriverBindingSource::Devicetree,
    DriverBindingSource::Manual,
];
const GPIO_DRIVER_METADATA: DriverMetadata = DriverMetadata {
    key: dogma::GPIO_DRIVER_DOGMA.key,
    class: DriverClass::Bus,
    identity: DriverIdentity {
        vendor: "Fusion",
        family: Some("Generic"),
        package: None,
        product: "GPIO driver",
        advertised_interface: "GPIO",
    },
    contracts: dogma::GPIO_DRIVER_DOGMA.contracts,
    required_contracts: dogma::GPIO_DRIVER_DOGMA.required_contracts,
    usefulness: dogma::GPIO_DRIVER_DOGMA.usefulness,
    singleton_class: dogma::GPIO_DRIVER_DOGMA.singleton_class,
    binding_sources: &GPIO_DRIVER_BINDING_SOURCES,
    description: "Universal GPIO provider driver layered over one selected hardware substrate",
};

/// Discoverable GPIO provider binding surfaced by the universal GPIO driver family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GpioBinding {
    pub provider: u8,
    pub controller_id: &'static str,
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
    provider: u8,
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
    /// Creates a new universal GPIO provider handle over one selected controller/provider.
    #[must_use]
    pub const fn new(provider: u8) -> Self {
        Self {
            provider,
            _hardware: PhantomData,
        }
    }

    /// Returns the truthful descriptor for this selected controller/provider.
    ///
    /// # Errors
    ///
    /// Returns one honest error when the selected provider binding is invalid.
    pub fn controller(&self) -> Result<&'static GpioControllerDescriptor, GpioError> {
        H::controller(self.provider).ok_or_else(GpioError::invalid)
    }

    /// Takes one pin exclusively from this selected provider.
    ///
    /// # Errors
    ///
    /// Returns one honest error when the pin is invalid, unsupported, or already claimed.
    pub fn take_pin(&self, pin: u8) -> Result<GpioPin<H::Pin>, GpioError> {
        Ok(GpioPin {
            inner: H::claim_pin(self.provider, pin)?,
        })
    }

    /// Returns one truthful capability snapshot for one pin number on this selected provider.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when the pin does not exist.
    pub fn capabilities(&self, pin: u8) -> Result<GpioCapabilities, GpioError> {
        GpioBaseContract::capabilities(self, pin)
    }

    /// Returns the statically or dynamically surfaced GPIO pin catalog for this provider.
    #[must_use]
    pub fn pins(&self) -> &'static [GpioPinDescriptor] {
        H::pins(self.provider)
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
    fn controller(&self) -> &'static GpioControllerDescriptor {
        Gpio::controller(self).unwrap_or_else(|_| panic!("invalid gpio provider {}", self.provider))
    }

    fn support(&self) -> GpioSupport {
        H::support(self.provider)
    }

    fn pins(&self) -> &'static [GpioPinDescriptor] {
        H::pins(self.provider)
    }
}

impl<H> GpioControlContract for Gpio<H>
where
    H: GpioHardware,
{
    type Pin = GpioPin<H::Pin>;

    fn take_pin(&self, pin: u8) -> Result<Self::Pin, GpioError> {
        Gpio::take_pin(self, pin)
    }
}

impl<P> GpioOwnedPinContract for GpioPin<P>
where
    P: GpioHardwarePin,
{
    fn controller(&self) -> &'static GpioControllerDescriptor {
        self.inner.controller()
    }

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
    let mut written = 0;
    for provider in 0..H::provider_count() {
        if written == out.len() {
            return Err(DriverError::resource_exhausted());
        }
        let support = H::support(provider);
        let Some(controller) = H::controller(provider) else {
            continue;
        };
        if support.implementation == GpioImplementationKind::Unsupported
            || support.caps.is_empty()
            || support.pin_count == 0
        {
            continue;
        }
        out[written] = GpioBinding {
            provider,
            controller_id: controller.id,
        };
        written += 1;
    }
    Ok(written)
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
    let Some(controller) = H::controller(binding.provider) else {
        return Err(DriverError::invalid());
    };
    if controller.id != binding.controller_id {
        return Err(DriverError::invalid());
    }

    Ok(ActiveDriver::new(binding, Gpio::<H>::new(binding.provider)))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interface::contract::{
        GpioHardware as GpioHardwareContract,
        GpioHardwarePin as GpioHardwarePinContract,
    };

    const TEST_CONTROLLER_A: GpioControllerDescriptor = GpioControllerDescriptor {
        id: "test-gpio-a",
        name: "Test GPIO A",
    };
    const TEST_CONTROLLER_B: GpioControllerDescriptor = GpioControllerDescriptor {
        id: "test-gpio-b",
        name: "Test GPIO B",
    };
    const TEST_PINS_A: [GpioPinDescriptor; 1] = [GpioPinDescriptor {
        pin: 1,
        name: "a1",
        capabilities: GpioCapabilities::INPUT,
    }];
    const TEST_PINS_B: [GpioPinDescriptor; 1] = [GpioPinDescriptor {
        pin: 2,
        name: "b2",
        capabilities: GpioCapabilities::OUTPUT,
    }];
    const TEST_SUPPORT: GpioSupport = GpioSupport {
        caps: GpioProviderCaps::ENUMERATE | GpioProviderCaps::CLAIM,
        implementation: GpioImplementationKind::Native,
        pin_count: 1,
    };

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct TestPin {
        provider: u8,
        pin: u8,
    }

    #[derive(Debug, Clone, Copy, Default)]
    struct TestHardware;

    impl GpioHardwareContract for TestHardware {
        type Pin = TestPin;

        fn provider_count() -> u8 {
            2
        }

        fn controller(provider: u8) -> Option<&'static GpioControllerDescriptor> {
            match provider {
                0 => Some(&TEST_CONTROLLER_A),
                1 => Some(&TEST_CONTROLLER_B),
                _ => None,
            }
        }

        fn support(provider: u8) -> GpioSupport {
            match provider {
                0 | 1 => TEST_SUPPORT,
                _ => GpioSupport::unsupported(),
            }
        }

        fn pins(provider: u8) -> &'static [GpioPinDescriptor] {
            match provider {
                0 => &TEST_PINS_A,
                1 => &TEST_PINS_B,
                _ => &[],
            }
        }

        fn claim_pin(provider: u8, pin: u8) -> Result<Self::Pin, GpioError> {
            if Self::pins(provider)
                .iter()
                .any(|descriptor| descriptor.pin == pin)
            {
                Ok(TestPin { provider, pin })
            } else {
                Err(GpioError::invalid())
            }
        }
    }

    impl GpioHardwarePinContract for TestPin {
        fn controller(&self) -> &'static GpioControllerDescriptor {
            match self.provider {
                0 => &TEST_CONTROLLER_A,
                1 => &TEST_CONTROLLER_B,
                _ => panic!("invalid test provider {}", self.provider),
            }
        }

        fn pin(&self) -> u8 {
            self.pin
        }

        fn capabilities(&self) -> GpioCapabilities {
            match self.provider {
                0 => GpioCapabilities::INPUT,
                1 => GpioCapabilities::OUTPUT,
                _ => GpioCapabilities::empty(),
            }
        }

        fn set_function(&mut self, _function: GpioFunction) -> Result<(), GpioError> {
            Err(GpioError::unsupported())
        }

        fn configure_input(&mut self) -> Result<(), GpioError> {
            Ok(())
        }

        fn read_level(&self) -> Result<bool, GpioError> {
            Ok(false)
        }

        fn configure_output(&mut self, _initial_high: bool) -> Result<(), GpioError> {
            Ok(())
        }

        fn set_level(&mut self, _high: bool) -> Result<(), GpioError> {
            Ok(())
        }

        fn set_pull(&mut self, _pull: GpioPull) -> Result<(), GpioError> {
            Err(GpioError::unsupported())
        }

        fn set_drive_strength(&mut self, _strength: GpioDriveStrength) -> Result<(), GpioError> {
            Err(GpioError::unsupported())
        }
    }

    #[test]
    fn gpio_driver_enumerates_multiple_controllers_honestly() {
        let mut registry = DriverRegistry::<1>::new();
        let registered = registry
            .register::<GpioDriver<TestHardware>>()
            .expect("test gpio driver should register");
        let mut context = GpioDriverContext::<TestHardware>::new();
        let mut bindings = [GpioBinding {
            provider: 0,
            controller_id: "",
        }; 2];
        let count = registered
            .enumerate_bindings(
                &mut DriverDiscoveryContext::new(&mut context),
                &mut bindings,
            )
            .expect("enumeration should succeed");
        assert_eq!(count, 2);
        assert_eq!(bindings[0].controller_id, TEST_CONTROLLER_A.id);
        assert_eq!(bindings[1].controller_id, TEST_CONTROLLER_B.id);
    }

    #[test]
    fn gpio_driver_activation_binds_instance_to_selected_controller() {
        let mut registry = DriverRegistry::<1>::new();
        let registered = registry
            .register::<GpioDriver<TestHardware>>()
            .expect("test gpio driver should register");
        let mut context = GpioDriverContext::<TestHardware>::new();
        let binding = GpioBinding {
            provider: 1,
            controller_id: TEST_CONTROLLER_B.id,
        };
        let active = registered
            .activate(&mut DriverActivationContext::new(&mut context), binding)
            .expect("activation should succeed");
        let gpio = active.into_instance();
        assert_eq!(
            gpio.controller().expect("controller should resolve").id,
            TEST_CONTROLLER_B.id
        );
        assert_eq!(gpio.pins()[0].pin, 2);
        let pin = gpio.take_pin(2).expect("pin should claim");
        assert_eq!(pin.controller().id, TEST_CONTROLLER_B.id);
    }
}
