//! fusion-sys GPIO ownership and capability surfaces.
//!
//! This module stays intentionally small: it exposes owned pin handles and truthful capability
//! reporting without pretending every backend has the same GPIO story.

use core::fmt;

/// Kind of GPIO failure surfaced by `fusion-sys`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GpioErrorKind {
    /// GPIO is unsupported on the selected backend.
    Unsupported,
    /// One invalid pin number or configuration was requested.
    Invalid,
    /// The requested GPIO is already owned or in one conflicting state.
    StateConflict,
}

/// Error surfaced by GPIO ownership/configuration operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GpioError {
    kind: GpioErrorKind,
}

impl GpioError {
    /// Creates one unsupported GPIO error.
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            kind: GpioErrorKind::Unsupported,
        }
    }

    /// Creates one invalid GPIO error.
    #[must_use]
    pub const fn invalid() -> Self {
        Self {
            kind: GpioErrorKind::Invalid,
        }
    }

    /// Creates one GPIO state-conflict error.
    #[must_use]
    pub const fn state_conflict() -> Self {
        Self {
            kind: GpioErrorKind::StateConflict,
        }
    }

    /// Returns the concrete GPIO error kind.
    #[must_use]
    pub const fn kind(self) -> GpioErrorKind {
        self.kind
    }
}

impl fmt::Display for GpioErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Unsupported => f.write_str("gpio unsupported"),
            Self::Invalid => f.write_str("invalid gpio request"),
            Self::StateConflict => f.write_str("gpio state conflict"),
        }
    }
}

impl fmt::Display for GpioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)
    }
}

bitflags::bitflags! {
    /// Honest capability set for one owned GPIO.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct GpioCapabilities: u32 {
        /// The pin can be configured as one input.
        const INPUT               = 1 << 0;
        /// The pin can be configured as one push-pull output.
        const OUTPUT              = 1 << 1;
        /// The pin exposes alternate-function muxing.
        const ALTERNATE_FUNCTIONS = 1 << 2;
        /// The pin exposes pull resistors.
        const PULLS               = 1 << 3;
        /// The pin exposes selectable drive strength.
        const DRIVE_STRENGTH      = 1 << 4;
        /// The pin can act as one interrupt/event source.
        const INTERRUPTS          = 1 << 5;
    }
}

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
    /// Software-controlled GPIO through the SIO block.
    Sio,
    /// One backend-specific raw mux selector.
    Raw(u8),
}

/// Shared contract for one owned GPIO handle.
pub trait GpioOwnedPin {
    /// Returns the concrete backend pin number.
    fn pin(&self) -> u8;

    /// Returns one truthful capability snapshot for this pin.
    fn capabilities(&self) -> GpioCapabilities;
}

/// Output-capable GPIO contract consumed by simple components such as LEDs.
pub trait GpioOutputPin: GpioOwnedPin {
    /// Configures this pin for software-controlled output.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when output configuration cannot be realized.
    fn configure_output(&mut self, initial_high: bool) -> Result<(), GpioError>;

    /// Sets the logical output level.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when the pin cannot be driven.
    fn set_level(&mut self, high: bool) -> Result<(), GpioError>;
}

/// Input-capable GPIO contract consumed by simple components such as buttons.
pub trait GpioInputPin: GpioOwnedPin {
    /// Configures this pin for software-controlled input sampling.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when input configuration cannot be realized.
    fn configure_input(&mut self) -> Result<(), GpioError>;

    /// Reads the current sampled input level.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when the pin cannot be read.
    fn read_level(&self) -> Result<bool, GpioError>;
}

/// Namespace for taking owned GPIO handles from the selected backend.
#[derive(Debug, Clone, Copy)]
pub struct Gpio;

/// Owned GPIO handle for the selected backend.
#[derive(Debug)]
pub struct GpioPin {
    pin: u8,
}

#[allow(clippy::missing_const_for_fn)]
impl Gpio {
    /// Takes exclusive ownership of one GPIO pin.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when GPIO is unsupported, the pin is invalid, or the pin
    /// is already owned.
    pub fn take(pin: u8) -> Result<GpioPin, GpioError> {
        platform::claim(pin)?;
        Ok(GpioPin { pin })
    }

    /// Returns one truthful capability snapshot for one backend pin number.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when the pin does not exist.
    pub fn capabilities(pin: u8) -> Result<GpioCapabilities, GpioError> {
        platform::capabilities(pin)
    }
}

#[allow(clippy::missing_const_for_fn)]
impl GpioPin {
    /// Returns the concrete pin number.
    #[must_use]
    pub const fn pin(&self) -> u8 {
        self.pin
    }

    /// Returns one truthful capability snapshot for this owned pin.
    #[must_use]
    pub fn capabilities(&self) -> GpioCapabilities {
        platform::capabilities(self.pin).unwrap_or_else(|_| GpioCapabilities::empty())
    }

    /// Selects one alternate-function mux setting for this pin.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when the function cannot be selected.
    pub fn set_function(&mut self, function: GpioFunction) -> Result<(), GpioError> {
        platform::set_function(self.pin, function)
    }

    /// Configures this pin for input sampling.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when input mode cannot be realized.
    pub fn configure_input(&mut self) -> Result<(), GpioError> {
        platform::configure_input(self.pin)
    }

    /// Reads the current sampled input level.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when the pin cannot be read.
    pub fn read(&self) -> Result<bool, GpioError> {
        platform::read(self.pin)
    }

    /// Configures this pin for software-controlled output.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when output mode cannot be realized.
    pub fn configure_output(&mut self, initial_high: bool) -> Result<(), GpioError> {
        platform::configure_output(self.pin, initial_high)
    }

    /// Sets the logical output level.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when the pin cannot be driven.
    pub fn set_level(&mut self, high: bool) -> Result<(), GpioError> {
        platform::write(self.pin, high)
    }

    /// Selects the pad pull-resistor mode.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when pull configuration is unsupported.
    pub fn set_pull(&mut self, pull: GpioPull) -> Result<(), GpioError> {
        platform::set_pull(self.pin, pull)
    }

    /// Selects the pad drive strength.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when drive-strength control is unsupported.
    pub fn set_drive_strength(&mut self, strength: GpioDriveStrength) -> Result<(), GpioError> {
        platform::set_drive_strength(self.pin, strength)
    }
}

impl GpioOwnedPin for GpioPin {
    fn pin(&self) -> u8 {
        self.pin
    }

    fn capabilities(&self) -> GpioCapabilities {
        self.capabilities()
    }
}

impl GpioOutputPin for GpioPin {
    fn configure_output(&mut self, initial_high: bool) -> Result<(), GpioError> {
        self.configure_output(initial_high)
    }

    fn set_level(&mut self, high: bool) -> Result<(), GpioError> {
        self.set_level(high)
    }
}

impl GpioInputPin for GpioPin {
    fn configure_input(&mut self) -> Result<(), GpioError> {
        self.configure_input()
    }

    fn read_level(&self) -> Result<bool, GpioError> {
        self.read()
    }
}

impl Drop for GpioPin {
    fn drop(&mut self) {
        platform::release(self.pin);
    }
}

#[cfg(all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350"))]
mod platform {
    use super::{GpioCapabilities, GpioDriveStrength, GpioError, GpioFunction, GpioPull};
    use core::ptr;
    use core::sync::atomic::{AtomicU32, Ordering};

    const RP2350_GPIO_COUNT: u8 = 30;
    const RP2350_RESETS_BASE: usize = 0x4002_0000;
    const RP2350_RESETS_CLR: *mut u32 = (RP2350_RESETS_BASE + 0x3000) as *mut u32;
    const RP2350_RESET_DONE: *const u32 = (RP2350_RESETS_BASE + 0x08) as *const u32;
    const RP2350_RESET_IO_BANK0: u32 = 1 << 6;
    const RP2350_RESET_PADS_BANK0: u32 = 1 << 9;

    const RP2350_IO_BANK0_BASE: usize = 0x4002_8000;
    const RP2350_PADS_BANK0_BASE: usize = 0x4003_8000;
    const RP2350_SIO_BASE: usize = 0xD000_0000;

    const RP2350_PAD_SLEWFAST_BIT: u32 = 1 << 0;
    const RP2350_PAD_PDE_BIT: u32 = 1 << 2;
    const RP2350_PAD_PUE_BIT: u32 = 1 << 3;
    const RP2350_PAD_DRIVE_LSB: u32 = 4;
    const RP2350_PAD_IE_BIT: u32 = 1 << 6;
    const RP2350_PAD_OD_BIT: u32 = 1 << 7;
    const RP2350_PAD_ISO_BIT: u32 = 1 << 8;

    const RP2350_SIO_GPIO_IN: *const u32 = (RP2350_SIO_BASE + 0x04) as *const u32;
    const RP2350_SIO_GPIO_OUT_SET: *mut u32 = (RP2350_SIO_BASE + 0x18) as *mut u32;
    const RP2350_SIO_GPIO_OUT_CLR: *mut u32 = (RP2350_SIO_BASE + 0x20) as *mut u32;
    const RP2350_SIO_GPIO_OE_SET: *mut u32 = (RP2350_SIO_BASE + 0x38) as *mut u32;
    const RP2350_SIO_GPIO_OE_CLR: *mut u32 = (RP2350_SIO_BASE + 0x40) as *mut u32;

    const RP2350_SIO_FUNCSEL: u32 = 5;
    static CLAIMED_GPIO: AtomicU32 = AtomicU32::new(0);

    fn ensure_bank0_ready() {
        // SAFETY: these are RP2350 reset-controller MMIO registers.
        unsafe {
            ptr::write_volatile(
                RP2350_RESETS_CLR,
                RP2350_RESET_IO_BANK0 | RP2350_RESET_PADS_BANK0,
            );
        }
        while unsafe { ptr::read_volatile(RP2350_RESET_DONE) }
            & (RP2350_RESET_IO_BANK0 | RP2350_RESET_PADS_BANK0)
            != (RP2350_RESET_IO_BANK0 | RP2350_RESET_PADS_BANK0)
        {}
    }

    fn validate_pin(pin: u8) -> Result<(), GpioError> {
        if pin >= RP2350_GPIO_COUNT {
            return Err(GpioError::invalid());
        }
        Ok(())
    }

    fn pad_register(pin: u8) -> *mut u32 {
        (RP2350_PADS_BANK0_BASE + 0x04 + usize::from(pin) * 4) as *mut u32
    }

    fn ctrl_register(pin: u8) -> *mut u32 {
        (RP2350_IO_BANK0_BASE + usize::from(pin) * 8 + 4) as *mut u32
    }

    pub(super) fn claim(pin: u8) -> Result<(), GpioError> {
        validate_pin(pin)?;
        let mask = 1_u32 << pin;
        let mut claimed = CLAIMED_GPIO.load(Ordering::Acquire);
        loop {
            if claimed & mask != 0 {
                return Err(GpioError::state_conflict());
            }
            match CLAIMED_GPIO.compare_exchange_weak(
                claimed,
                claimed | mask,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return Ok(()),
                Err(observed) => claimed = observed,
            }
        }
    }

    pub(super) fn release(pin: u8) {
        if pin >= RP2350_GPIO_COUNT {
            return;
        }
        CLAIMED_GPIO.fetch_and(!(1_u32 << pin), Ordering::AcqRel);
    }

    pub(super) fn capabilities(pin: u8) -> Result<GpioCapabilities, GpioError> {
        validate_pin(pin)?;
        Ok(GpioCapabilities::INPUT
            | GpioCapabilities::OUTPUT
            | GpioCapabilities::ALTERNATE_FUNCTIONS
            | GpioCapabilities::PULLS
            | GpioCapabilities::DRIVE_STRENGTH
            | GpioCapabilities::INTERRUPTS)
    }

    pub(super) fn set_function(pin: u8, function: GpioFunction) -> Result<(), GpioError> {
        validate_pin(pin)?;
        ensure_bank0_ready();
        let value = match function {
            GpioFunction::Sio => RP2350_SIO_FUNCSEL,
            GpioFunction::Raw(value) => u32::from(value),
        };
        // SAFETY: one owned RP2350 GPIO control register.
        unsafe { ptr::write_volatile(ctrl_register(pin), value) };
        Ok(())
    }

    pub(super) fn configure_input(pin: u8) -> Result<(), GpioError> {
        validate_pin(pin)?;
        ensure_bank0_ready();
        let pad = pad_register(pin);
        let pad_value = unsafe { ptr::read_volatile(pad) };
        unsafe {
            ptr::write_volatile(
                pad,
                (pad_value | RP2350_PAD_IE_BIT) & !(RP2350_PAD_OD_BIT | RP2350_PAD_ISO_BIT),
            );
            ptr::write_volatile(RP2350_SIO_GPIO_OE_CLR, 1_u32 << pin);
        }
        Ok(())
    }

    pub(super) fn read(pin: u8) -> Result<bool, GpioError> {
        validate_pin(pin)?;
        Ok(unsafe { ptr::read_volatile(RP2350_SIO_GPIO_IN) } & (1_u32 << pin) != 0)
    }

    pub(super) fn configure_output(pin: u8, initial_high: bool) -> Result<(), GpioError> {
        validate_pin(pin)?;
        ensure_bank0_ready();
        let pad = pad_register(pin);
        let pad_value = unsafe { ptr::read_volatile(pad) };
        unsafe {
            ptr::write_volatile(
                pad,
                (pad_value | RP2350_PAD_IE_BIT) & !(RP2350_PAD_OD_BIT | RP2350_PAD_ISO_BIT),
            );
        }
        set_function(pin, GpioFunction::Sio)?;
        write(pin, initial_high)?;
        unsafe { ptr::write_volatile(RP2350_SIO_GPIO_OE_SET, 1_u32 << pin) };
        Ok(())
    }

    pub(super) fn write(pin: u8, high: bool) -> Result<(), GpioError> {
        validate_pin(pin)?;
        let register = if high {
            RP2350_SIO_GPIO_OUT_SET
        } else {
            RP2350_SIO_GPIO_OUT_CLR
        };
        unsafe { ptr::write_volatile(register, 1_u32 << pin) };
        Ok(())
    }

    pub(super) fn set_pull(pin: u8, pull: GpioPull) -> Result<(), GpioError> {
        validate_pin(pin)?;
        ensure_bank0_ready();
        let pad = pad_register(pin);
        let mut pad_value = unsafe { ptr::read_volatile(pad) };
        pad_value &= !(RP2350_PAD_PUE_BIT | RP2350_PAD_PDE_BIT);
        match pull {
            GpioPull::None => {}
            GpioPull::Up => pad_value |= RP2350_PAD_PUE_BIT,
            GpioPull::Down => pad_value |= RP2350_PAD_PDE_BIT,
        }
        unsafe { ptr::write_volatile(pad, pad_value) };
        Ok(())
    }

    pub(super) fn set_drive_strength(
        pin: u8,
        strength: GpioDriveStrength,
    ) -> Result<(), GpioError> {
        validate_pin(pin)?;
        ensure_bank0_ready();
        let pad = pad_register(pin);
        let mut pad_value = unsafe { ptr::read_volatile(pad) };
        pad_value &= !(0b11_u32 << RP2350_PAD_DRIVE_LSB);
        pad_value |= match strength {
            GpioDriveStrength::MilliAmps2 => 0b00,
            GpioDriveStrength::MilliAmps4 => 0b01,
            GpioDriveStrength::MilliAmps8 => 0b10,
            GpioDriveStrength::MilliAmps12 => 0b11,
        } << RP2350_PAD_DRIVE_LSB;
        unsafe { ptr::write_volatile(pad, pad_value & !RP2350_PAD_SLEWFAST_BIT) };
        Ok(())
    }
}

#[cfg(not(all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")))]
mod platform {
    use super::{GpioCapabilities, GpioDriveStrength, GpioError, GpioFunction, GpioPull};

    pub(super) const fn claim(_pin: u8) -> Result<(), GpioError> {
        Err(GpioError::unsupported())
    }

    pub(super) const fn release(_pin: u8) {}

    pub(super) const fn capabilities(_pin: u8) -> Result<GpioCapabilities, GpioError> {
        Err(GpioError::unsupported())
    }

    pub(super) const fn set_function(_pin: u8, _function: GpioFunction) -> Result<(), GpioError> {
        Err(GpioError::unsupported())
    }

    pub(super) const fn configure_input(_pin: u8) -> Result<(), GpioError> {
        Err(GpioError::unsupported())
    }

    pub(super) const fn read(_pin: u8) -> Result<bool, GpioError> {
        Err(GpioError::unsupported())
    }

    pub(super) const fn configure_output(_pin: u8, _initial_high: bool) -> Result<(), GpioError> {
        Err(GpioError::unsupported())
    }

    pub(super) const fn write(_pin: u8, _high: bool) -> Result<(), GpioError> {
        Err(GpioError::unsupported())
    }

    pub(super) const fn set_pull(_pin: u8, _pull: GpioPull) -> Result<(), GpioError> {
        Err(GpioError::unsupported())
    }

    pub(super) const fn set_drive_strength(
        _pin: u8,
        _strength: GpioDriveStrength,
    ) -> Result<(), GpioError> {
        Err(GpioError::unsupported())
    }
}
