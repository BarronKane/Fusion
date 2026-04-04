//! RP2350-selected Bluetooth driver exports.
//!
//! The selected RP2350 board contract currently follows Pico 2 W wiring truth. This module does
//! not implement Bluetooth itself; it only claims the reserved board GPIO pins and attaches them
//! to the generic CYW43439 GPIO backend in `fusion-hal`.

use core::hint::spin_loop;

use fusion_hal::contract::drivers::bus::gpio::{
    GpioError,
    GpioErrorKind,
};
use fusion_hal::contract::drivers::net::bluetooth::BluetoothError;
use fusion_hal::drivers::bus::gpio::GpioPin as HalGpioPin;
use fusion_hal::drivers::net::bluetooth::infineon::{
    CYW43439 as UniversalCYW43439,
    cyw43439::interface::backend::gpio::GpioBackend as Cyw43439GpioBackend,
};

use crate::pal::soc::cortex_m::rp2350::{
    CortexMBluetoothControllerBinding,
    CortexMBluetoothTransportBinding,
    bluetooth_controllers,
    monotonic_raw_now,
    monotonic_tick_hz,
};
use super::super::bus::gpio::{
    GpioPinHardware,
    claim_board_owned_pin,
};

type Cyw43439Backend = Cyw43439GpioBackend<
    GpioPinHardware,
    GpioPinHardware,
    GpioPinHardware,
    GpioPinHardware,
    GpioPinHardware,
    GpioPinHardware,
>;

/// Selected universal Bluetooth driver composed over the RP2350 Pico 2 W GPIO wiring.
pub type Bluetooth = UniversalCYW43439<Cyw43439Backend>;

/// Returns the selected universal Bluetooth provider over the RP2350 Pico 2 W GPIO wiring.
///
/// # Errors
///
/// Returns an error if the selected board does not expose the CYW43439 binding honestly or the
/// reserved GPIO pins are already claimed.
pub fn system_bluetooth() -> Result<Bluetooth, BluetoothError> {
    let binding = cyw43439_binding().ok_or_else(BluetoothError::unsupported)?;

    let (clock_gpio, chip_select_gpio, data_irq_gpio) = match binding.transport {
        CortexMBluetoothTransportBinding::Spi3WireSharedDataIrq {
            clock_gpio,
            chip_select_gpio,
            data_irq_gpio,
        } => (clock_gpio, chip_select_gpio, data_irq_gpio),
        _ => return Err(BluetoothError::unsupported()),
    };

    let clock = HalGpioPin::from_inner(claim_board_owned_pin(clock_gpio).map_err(map_gpio_error)?);
    let chip_select =
        HalGpioPin::from_inner(claim_board_owned_pin(chip_select_gpio).map_err(map_gpio_error)?);
    let data_irq =
        HalGpioPin::from_inner(claim_board_owned_pin(data_irq_gpio).map_err(map_gpio_error)?);
    let power = claim_optional_pin(binding.power_gpio)?;
    let reset = claim_optional_pin(binding.reset_gpio)?;
    let wake = claim_optional_pin(binding.wake_gpio)?;

    Ok(Bluetooth::new(Cyw43439Backend::new(
        clock,
        chip_select,
        data_irq,
        power,
        reset,
        wake,
        rp2350_delay_ms,
        None,
        None,
    )))
}

fn claim_optional_pin(
    pin: Option<u8>,
) -> Result<Option<HalGpioPin<GpioPinHardware>>, BluetoothError> {
    pin.map(|pin| claim_board_owned_pin(pin).map(HalGpioPin::from_inner))
        .transpose()
        .map_err(map_gpio_error)
}

fn cyw43439_binding() -> Option<CortexMBluetoothControllerBinding> {
    bluetooth_controllers()
        .iter()
        .copied()
        .find(|binding| binding.chip == "CYW43439")
}

fn rp2350_delay_ms(milliseconds: u32) {
    if milliseconds == 0 {
        return;
    }

    let Ok(start) = monotonic_raw_now() else {
        return;
    };
    let Some(ticks_per_second) = monotonic_tick_hz() else {
        return;
    };
    let delay_ticks = (u64::from(milliseconds).saturating_mul(ticks_per_second)) / 1_000;
    let deadline = start.saturating_add(delay_ticks.max(1));

    loop {
        let Ok(now) = monotonic_raw_now() else {
            break;
        };
        if now >= deadline {
            break;
        }
        spin_loop();
    }
}

fn map_gpio_error(error: GpioError) -> BluetoothError {
    match error.kind() {
        GpioErrorKind::Unsupported => BluetoothError::unsupported(),
        GpioErrorKind::Invalid => BluetoothError::invalid(),
        GpioErrorKind::Busy => BluetoothError::busy(),
        GpioErrorKind::ResourceExhausted => BluetoothError::resource_exhausted(),
        GpioErrorKind::StateConflict => BluetoothError::state_conflict(),
        GpioErrorKind::Platform(code) => BluetoothError::platform(code),
    }
}
