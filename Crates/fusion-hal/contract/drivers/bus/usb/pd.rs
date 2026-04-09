//! USB Power Delivery policy and contract vocabulary.

use super::core::*;
use super::error::*;

/// USB Power Delivery revision truth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UsbPdRevision {
    pub major: u8,
    pub minor: u8,
}

impl UsbPdRevision {
    pub const REV_2_0: Self = Self { major: 2, minor: 0 };
    pub const REV_3_0: Self = Self { major: 3, minor: 0 };
    pub const REV_3_1: Self = Self { major: 3, minor: 1 };
    pub const REV_3_2: Self = Self { major: 3, minor: 2 };
}

/// Canonical PD power-data-object family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UsbPdPowerDataObject {
    FixedSupply {
        voltage_mv: u32,
        maximum_current_ma: u32,
    },
    VariableSupply {
        minimum_voltage_mv: u32,
        maximum_voltage_mv: u32,
        maximum_current_ma: u32,
    },
    Battery {
        minimum_voltage_mv: u32,
        maximum_voltage_mv: u32,
        maximum_power_mw: u32,
    },
    ProgrammableSupply {
        minimum_voltage_mv: u32,
        maximum_voltage_mv: u32,
        maximum_current_ma: u32,
    },
    AugmentedProgrammableSupply {
        minimum_voltage_mv: u32,
        maximum_voltage_mv: u32,
        maximum_current_ma: u32,
    },
    Other(u32),
}

/// Current PD contract state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UsbPdContractState<'a> {
    pub revision: Option<UsbPdRevision>,
    pub explicit_contract: bool,
    pub active_voltage_mv: Option<u32>,
    pub active_current_ma: Option<u32>,
    pub source_capabilities: &'a [UsbPdPowerDataObject],
    pub sink_capabilities: &'a [UsbPdPowerDataObject],
}

/// Shared USB PD policy/contract surface.
pub trait UsbPdContract: UsbCoreContract {
    /// Returns the current PD contract snapshot.
    ///
    /// # Errors
    ///
    /// Returns an error when PD state is unavailable or unsupported.
    fn pd_contract_state(&self) -> Result<UsbPdContractState<'static>, UsbError>;
}
