//! Shared generic GPIO identifier and descriptor vocabulary.

/// Pull-resistor mode for one GPIO pad.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GpioPull {
    /// No pull resistor enabled.
    None,
    /// Pull-up enabled.
    Up,
    /// Pull-down enabled.
    Down,
}

/// Drive-strength selection for one GPIO pad.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GpioDriveStrength {
    /// Approximately 2 mA drive.
    MilliAmps2,
    /// Approximately 4 mA drive.
    MilliAmps4,
    /// Approximately 8 mA drive.
    MilliAmps8,
    /// Approximately 12 mA drive.
    MilliAmps12,
}

/// Raw alternate-function selector for one GPIO pin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GpioFunction {
    /// Software-controlled GPIO through one backend's ordinary GPIO datapath.
    Sio,
    /// One backend-specific raw mux selector.
    Raw(u8),
}

/// Static descriptor for one surfaced GPIO pin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GpioPinDescriptor {
    /// Stable pin number within the surfaced GPIO provider.
    pub pin: u8,
    /// Human-readable pin name.
    pub name: &'static str,
    /// Truthful capability snapshot for the pin.
    pub capabilities: crate::contract::drivers::gpio::GpioCapabilities,
}
