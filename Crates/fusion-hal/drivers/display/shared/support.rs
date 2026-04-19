//! Shared display-port driver support helpers.

use crate::contract::drivers::display::{
    DisplayConfigError,
    DisplayError,
    DisplayErrorKind,
};
use crate::contract::drivers::driver::DriverError;

pub fn map_config_error(error: DisplayConfigError) -> DisplayError {
    match error {
        DisplayConfigError::NotReady => DisplayError::state_conflict(),
        _ => DisplayError::invalid(),
    }
}

pub fn map_display_error(error: DisplayError) -> DriverError {
    match error.kind() {
        DisplayErrorKind::Unsupported => DriverError::unsupported(),
        DisplayErrorKind::Invalid => DriverError::invalid(),
        DisplayErrorKind::Busy => DriverError::busy(),
        DisplayErrorKind::ResourceExhausted => DriverError::resource_exhausted(),
        DisplayErrorKind::StateConflict => DriverError::state_conflict(),
        DisplayErrorKind::Timeout => DriverError::platform(-1),
        DisplayErrorKind::Disconnected => DriverError::platform(-2),
        DisplayErrorKind::NegotiationFailed => DriverError::invalid(),
        DisplayErrorKind::Platform(code) => DriverError::platform(code),
    }
}
