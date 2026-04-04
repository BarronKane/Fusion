//! GPIO-composed CYW43439 backend.

use crate::contract::drivers::net::bluetooth::{
    BluetoothAdapterDescriptor,
    BluetoothAdapterId,
    BluetoothAdapterSupport,
    BluetoothError,
    BluetoothImplementationKind,
    BluetoothProviderCaps,
    BluetoothSupport,
    BluetoothVersion,
    BluetoothVersionRange,
};
use crate::drivers::bus::gpio::{
    GpioFunction,
    GpioPin,
};
use crate::drivers::bus::gpio::interface::contract::GpioHardwarePin;
use crate::drivers::net::bluetooth::infineon::cyw43439::interface::contract::{
    Cyw43439ControllerCaps,
    Cyw43439Hardware,
};

const CYW43439_ADAPTER_ID: BluetoothAdapterId = BluetoothAdapterId(0);
const CYW43439_ADAPTERS: [BluetoothAdapterDescriptor; 1] = [BluetoothAdapterDescriptor {
    id: CYW43439_ADAPTER_ID,
    name: "CYW43439",
    address: None,
    version: BluetoothVersionRange {
        minimum: BluetoothVersion::new(5, 2),
        maximum: BluetoothVersion::new(5, 2),
    },
    support: BluetoothAdapterSupport {
        transports: crate::contract::drivers::net::bluetooth::BluetoothTransportCaps::empty(),
        roles: crate::contract::drivers::net::bluetooth::BluetoothRoleCaps::empty(),
        le_phys: crate::contract::drivers::net::bluetooth::BluetoothLePhyCaps::empty(),
        advertising: crate::contract::drivers::net::bluetooth::BluetoothAdvertisingCaps::empty(),
        scanning: crate::contract::drivers::net::bluetooth::BluetoothScanningCaps::empty(),
        connection: crate::contract::drivers::net::bluetooth::BluetoothConnectionCaps::empty(),
        security: crate::contract::drivers::net::bluetooth::BluetoothSecurityCaps::empty(),
        l2cap: crate::contract::drivers::net::bluetooth::BluetoothL2capCaps::empty(),
        att: crate::contract::drivers::net::bluetooth::BluetoothAttCaps::empty(),
        gatt: crate::contract::drivers::net::bluetooth::BluetoothGattCaps::empty(),
        iso: crate::contract::drivers::net::bluetooth::BluetoothIsoCaps::empty(),
        max_connections: 0,
        max_advertising_sets: 0,
        max_periodic_advertising_sets: 0,
        max_att_mtu: 0,
        max_attribute_value_len: 0,
        max_l2cap_channels: 0,
        max_l2cap_sdu_len: 0,
    },
}];

/// CYW43439 backend composed over owned GPIO pins.
#[derive(Debug)]
pub struct GpioBackend<
    ClockPin: GpioHardwarePin,
    ChipSelectPin: GpioHardwarePin,
    DataIrqPin: GpioHardwarePin,
    PowerPin: GpioHardwarePin,
    ResetPin: GpioHardwarePin,
    WakePin: GpioHardwarePin,
> {
    clock: GpioPin<ClockPin>,
    chip_select: GpioPin<ChipSelectPin>,
    data_irq: GpioPin<DataIrqPin>,
    power: Option<GpioPin<PowerPin>>,
    reset: Option<GpioPin<ResetPin>>,
    wake: Option<GpioPin<WakePin>>,
    delay: fn(u32),
    firmware: Option<&'static [u8]>,
    nvram: Option<&'static [u8]>,
    claimed: bool,
    powered: bool,
    power_configured: bool,
    reset_configured: bool,
    wake_configured: bool,
}

impl<
    ClockPin: GpioHardwarePin,
    ChipSelectPin: GpioHardwarePin,
    DataIrqPin: GpioHardwarePin,
    PowerPin: GpioHardwarePin,
    ResetPin: GpioHardwarePin,
    WakePin: GpioHardwarePin,
> GpioBackend<ClockPin, ChipSelectPin, DataIrqPin, PowerPin, ResetPin, WakePin>
{
    /// Creates one GPIO-composed CYW43439 backend.
    #[must_use]
    pub fn new(
        clock: GpioPin<ClockPin>,
        chip_select: GpioPin<ChipSelectPin>,
        data_irq: GpioPin<DataIrqPin>,
        power: Option<GpioPin<PowerPin>>,
        reset: Option<GpioPin<ResetPin>>,
        wake: Option<GpioPin<WakePin>>,
        delay: fn(u32),
        firmware: Option<&'static [u8]>,
        nvram: Option<&'static [u8]>,
    ) -> Self {
        Self {
            clock,
            chip_select,
            data_irq,
            power,
            reset,
            wake,
            delay,
            firmware,
            nvram,
            claimed: false,
            powered: false,
            power_configured: false,
            reset_configured: false,
            wake_configured: false,
        }
    }

    fn validate_adapter(&self, adapter: BluetoothAdapterId) -> Result<(), BluetoothError> {
        if adapter == CYW43439_ADAPTER_ID {
            Ok(())
        } else {
            Err(BluetoothError::invalid())
        }
    }

    fn provider_caps(&self) -> BluetoothProviderCaps {
        let mut caps = BluetoothProviderCaps::ENUMERATE_ADAPTERS
            | BluetoothProviderCaps::OPEN_ADAPTER
            | BluetoothProviderCaps::STATIC_TOPOLOGY;

        if self.power.is_some() {
            caps |= BluetoothProviderCaps::POWER_CONTROL;
        }

        caps
    }

    fn configure_output_pin<P: GpioHardwarePin>(
        pin: &mut GpioPin<P>,
        initial_high: bool,
        already_configured: &mut bool,
    ) -> Result<(), BluetoothError> {
        pin.set_function(GpioFunction::Sio)
            .map_err(map_gpio_error)?;

        if *already_configured {
            pin.set_level(initial_high).map_err(map_gpio_error)
        } else {
            pin.configure_output(initial_high).map_err(map_gpio_error)?;
            *already_configured = true;
            Ok(())
        }
    }
}

impl<
    ClockPin: GpioHardwarePin,
    ChipSelectPin: GpioHardwarePin,
    DataIrqPin: GpioHardwarePin,
    PowerPin: GpioHardwarePin,
    ResetPin: GpioHardwarePin,
    WakePin: GpioHardwarePin,
> Cyw43439Hardware
    for GpioBackend<ClockPin, ChipSelectPin, DataIrqPin, PowerPin, ResetPin, WakePin>
{
    fn support(&self) -> BluetoothSupport {
        BluetoothSupport {
            caps: self.provider_caps(),
            implementation: BluetoothImplementationKind::Native,
            adapter_count: 1,
        }
    }

    fn adapters(&self) -> &'static [BluetoothAdapterDescriptor] {
        &CYW43439_ADAPTERS
    }

    fn controller_caps(&self, adapter: BluetoothAdapterId) -> Cyw43439ControllerCaps {
        if self.validate_adapter(adapter).is_err() {
            return Cyw43439ControllerCaps::empty();
        }

        let mut caps = Cyw43439ControllerCaps::CLAIM_CONTROLLER;

        if self.power.is_some() {
            caps |= Cyw43439ControllerCaps::POWER_CONTROL;
        }
        if self.reset.is_some() {
            caps |= Cyw43439ControllerCaps::RESET_CONTROL;
        }
        if self.wake.is_some() {
            caps |= Cyw43439ControllerCaps::WAKE_CONTROL;
        }
        caps |= Cyw43439ControllerCaps::TIMING_DELAY;

        caps
    }

    fn claim_controller(&mut self, adapter: BluetoothAdapterId) -> Result<(), BluetoothError> {
        self.validate_adapter(adapter)?;
        let _ = self.clock.pin();
        let _ = self.chip_select.pin();
        let _ = self.data_irq.pin();
        if self.claimed {
            return Err(BluetoothError::state_conflict());
        }
        self.claimed = true;
        Ok(())
    }

    fn release_controller(&mut self, adapter: BluetoothAdapterId) {
        if self.validate_adapter(adapter).is_ok() {
            self.claimed = false;
        }
    }

    fn controller_powered(&self, adapter: BluetoothAdapterId) -> Result<bool, BluetoothError> {
        self.validate_adapter(adapter)?;
        if self.power.is_none() {
            return Err(BluetoothError::unsupported());
        }
        Ok(self.powered)
    }

    fn set_controller_powered(
        &mut self,
        adapter: BluetoothAdapterId,
        powered: bool,
    ) -> Result<(), BluetoothError> {
        self.validate_adapter(adapter)?;
        let power = self
            .power
            .as_mut()
            .ok_or_else(BluetoothError::unsupported)?;
        Self::configure_output_pin(power, powered, &mut self.power_configured)?;
        self.powered = powered;
        Ok(())
    }

    fn set_controller_reset(
        &mut self,
        adapter: BluetoothAdapterId,
        asserted: bool,
    ) -> Result<(), BluetoothError> {
        self.validate_adapter(adapter)?;
        let reset = self
            .reset
            .as_mut()
            .ok_or_else(BluetoothError::unsupported)?;
        Self::configure_output_pin(reset, asserted, &mut self.reset_configured)
    }

    fn set_controller_wake(
        &mut self,
        adapter: BluetoothAdapterId,
        awake: bool,
    ) -> Result<(), BluetoothError> {
        self.validate_adapter(adapter)?;
        let wake = self.wake.as_mut().ok_or_else(BluetoothError::unsupported)?;
        Self::configure_output_pin(wake, awake, &mut self.wake_configured)
    }

    fn wait_for_controller_irq(
        &mut self,
        adapter: BluetoothAdapterId,
        _timeout_ms: Option<u32>,
    ) -> Result<bool, BluetoothError> {
        self.validate_adapter(adapter)?;
        Err(BluetoothError::unsupported())
    }

    fn acknowledge_controller_irq(
        &mut self,
        adapter: BluetoothAdapterId,
    ) -> Result<(), BluetoothError> {
        self.validate_adapter(adapter)?;
        Err(BluetoothError::unsupported())
    }

    fn write_controller_transport(
        &mut self,
        adapter: BluetoothAdapterId,
        _payload: &[u8],
    ) -> Result<(), BluetoothError> {
        self.validate_adapter(adapter)?;
        Err(BluetoothError::unsupported())
    }

    fn read_controller_transport(
        &mut self,
        adapter: BluetoothAdapterId,
        _out: &mut [u8],
    ) -> Result<usize, BluetoothError> {
        self.validate_adapter(adapter)?;
        Err(BluetoothError::unsupported())
    }

    fn firmware_image(
        &self,
        adapter: BluetoothAdapterId,
    ) -> Result<Option<&'static [u8]>, BluetoothError> {
        self.validate_adapter(adapter)?;
        Ok(self.firmware)
    }

    fn nvram_image(
        &self,
        adapter: BluetoothAdapterId,
    ) -> Result<Option<&'static [u8]>, BluetoothError> {
        self.validate_adapter(adapter)?;
        Ok(self.nvram)
    }

    fn delay_ms(&self, milliseconds: u32) {
        (self.delay)(milliseconds);
    }
}

fn map_gpio_error(error: crate::contract::drivers::bus::gpio::GpioError) -> BluetoothError {
    match error.kind() {
        crate::contract::drivers::bus::gpio::GpioErrorKind::Unsupported => {
            BluetoothError::unsupported()
        }
        crate::contract::drivers::bus::gpio::GpioErrorKind::Invalid => BluetoothError::invalid(),
        crate::contract::drivers::bus::gpio::GpioErrorKind::Busy => BluetoothError::busy(),
        crate::contract::drivers::bus::gpio::GpioErrorKind::ResourceExhausted => {
            BluetoothError::resource_exhausted()
        }
        crate::contract::drivers::bus::gpio::GpioErrorKind::StateConflict => {
            BluetoothError::state_conflict()
        }
        crate::contract::drivers::bus::gpio::GpioErrorKind::Platform(code) => {
            BluetoothError::platform(code)
        }
    }
}
