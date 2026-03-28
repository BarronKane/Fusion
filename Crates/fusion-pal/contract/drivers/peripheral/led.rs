//! LED peripheral contracts.

/// Binary LED contract.
pub trait LedContract {
    /// Concrete backend or composition error.
    type Error;

    /// Returns whether the LED is currently commanded on.
    fn is_on(&self) -> bool;

    /// Sets the LED on/off state.
    ///
    /// # Errors
    ///
    /// Returns an honest backend error when the LED cannot be driven.
    fn set(&mut self, on: bool) -> Result<(), Self::Error>;

    /// Turns the LED on.
    ///
    /// # Errors
    ///
    /// Returns an honest backend error when the LED cannot be driven.
    fn on(&mut self) -> Result<(), Self::Error> {
        self.set(true)
    }

    /// Turns the LED off.
    ///
    /// # Errors
    ///
    /// Returns an honest backend error when the LED cannot be driven.
    fn off(&mut self) -> Result<(), Self::Error> {
        self.set(false)
    }

    /// Toggles the LED state.
    ///
    /// # Errors
    ///
    /// Returns an honest backend error when the LED cannot be driven.
    fn toggle(&mut self) -> Result<(), Self::Error>;
}

/// Level-capable LED contract.
pub trait LedLevelContract: LedContract {
    /// Returns the maximum supported drive level for this LED.
    fn max_level(&self) -> u16;

    /// Sets the LED drive level.
    ///
    /// `0` means off and `max_level()` means fully driven.
    ///
    /// # Errors
    ///
    /// Returns an honest backend error when the LED cannot be driven at the requested level.
    fn set_level(&mut self, level: u16) -> Result<(), Self::Error>;
}
