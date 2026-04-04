//! GPIO-facing composition interfaces for peripheral drivers.

pub use crate::contract::drivers::bus::gpio::GpioError as GpioPeripheralError;
pub use crate::contract::drivers::bus::gpio::GpioInputPin as GpioPeripheralInputPin;
pub use crate::contract::drivers::bus::gpio::GpioOutputPin as GpioPeripheralOutputPin;
pub use crate::contract::drivers::bus::gpio::GpioPinControl as GpioPeripheralControlPin;

/// Marker for peripheral drivers composed over one GPIO-facing interface.
pub trait GpioPeripheral {
    /// Error surfaced by the underlying GPIO-facing composition.
    type Error;
}
