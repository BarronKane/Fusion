//! Dataflow endpoint vocabulary for the PCU IR core.

use super::PcuValueType;

/// Direction of one PCU port.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuPortDirection {
    Input,
    Output,
    InOut,
}

/// Traffic cadence for one PCU port.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuPortRate {
    Single,
    Stream,
    Signal,
    Latch,
}

/// Blocking behavior for one PCU port.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuPortBlocking {
    Blocking,
    NonBlocking,
}

/// Delivery/reliability behavior for one PCU port.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuPortReliability {
    Lossless,
    Lossy,
}

/// Backpressure behavior for one PCU port.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuPortBackpressure {
    Backpressured,
    FreeRunning,
}

/// One typed directional I/O endpoint for one kernel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuPort<'a> {
    pub name: Option<&'a str>,
    pub direction: PcuPortDirection,
    pub value_type: PcuValueType,
    pub rate: PcuPortRate,
    pub blocking: PcuPortBlocking,
    pub reliability: PcuPortReliability,
    pub backpressure: PcuPortBackpressure,
}

impl<'a> PcuPort<'a> {
    /// Creates one explicit PCU port.
    #[must_use]
    pub const fn new(
        name: Option<&'a str>,
        direction: PcuPortDirection,
        value_type: PcuValueType,
        rate: PcuPortRate,
        blocking: PcuPortBlocking,
        reliability: PcuPortReliability,
        backpressure: PcuPortBackpressure,
    ) -> Self {
        Self {
            name,
            direction,
            value_type,
            rate,
            blocking,
            reliability,
            backpressure,
        }
    }

    /// Creates one lossless stream input port with ordinary non-blocking profile defaults.
    #[must_use]
    pub const fn stream_input(name: Option<&'a str>, value_type: PcuValueType) -> Self {
        Self::new(
            name,
            PcuPortDirection::Input,
            value_type,
            PcuPortRate::Stream,
            PcuPortBlocking::NonBlocking,
            PcuPortReliability::Lossless,
            PcuPortBackpressure::Backpressured,
        )
    }

    /// Creates one lossless stream output port with ordinary non-blocking profile defaults.
    #[must_use]
    pub const fn stream_output(name: Option<&'a str>, value_type: PcuValueType) -> Self {
        Self::new(
            name,
            PcuPortDirection::Output,
            value_type,
            PcuPortRate::Stream,
            PcuPortBlocking::NonBlocking,
            PcuPortReliability::Lossless,
            PcuPortBackpressure::Backpressured,
        )
    }
}
