//! Backend-neutral unsupported power implementation.

use super::{PowerBase, PowerControl, PowerError, PowerModeDescriptor, PowerSupport};

/// Unsupported power provider placeholder.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnsupportedPower;

impl UnsupportedPower {
    /// Creates a new unsupported power provider placeholder.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl PowerBase for UnsupportedPower {
    fn support(&self) -> PowerSupport {
        PowerSupport::unsupported()
    }
}

impl PowerControl for UnsupportedPower {
    fn modes(&self) -> &'static [PowerModeDescriptor] {
        &[]
    }

    fn enter_mode(&self, _name: &str) -> Result<(), PowerError> {
        Err(PowerError::unsupported())
    }
}
