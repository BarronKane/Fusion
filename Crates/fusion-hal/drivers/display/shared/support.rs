//! Shared display-port driver support helpers.

use crate::contract::drivers::display::{
    DisplayConfigError,
    DisplayError,
    DisplayErrorKind,
};
use crate::contract::drivers::driver::DriverError;

#[must_use]
pub const fn map_config_error(error: DisplayConfigError) -> DisplayError {
    match error {
        DisplayConfigError::NotReady => DisplayError::state_conflict(),
        _ => DisplayError::invalid(),
    }
}

#[must_use]
pub const fn map_display_error(error: DisplayError) -> DriverError {
    match error.kind() {
        DisplayErrorKind::Unsupported => DriverError::unsupported(),
        DisplayErrorKind::Invalid | DisplayErrorKind::NegotiationFailed => DriverError::invalid(),
        DisplayErrorKind::Busy => DriverError::busy(),
        DisplayErrorKind::ResourceExhausted => DriverError::resource_exhausted(),
        DisplayErrorKind::StateConflict => DriverError::state_conflict(),
        DisplayErrorKind::Timeout => DriverError::platform(-1),
        DisplayErrorKind::Disconnected => DriverError::platform(-2),
        DisplayErrorKind::Platform(code) => DriverError::platform(code),
    }
}
