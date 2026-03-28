//! Button peripheral contracts.

/// Binary button contract.
pub trait ButtonContract {
    /// Concrete backend or composition error.
    type Error;

    /// Returns whether the button is currently pressed.
    ///
    /// # Errors
    ///
    /// Returns an honest backend error when the button cannot be sampled.
    fn is_pressed(&self) -> Result<bool, Self::Error>;
}
