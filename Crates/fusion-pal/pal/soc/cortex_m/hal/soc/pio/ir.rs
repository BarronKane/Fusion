//! Portable deterministic programmable-IO kernel and execution-state vocabulary.

use super::PioProgramId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PioIrShiftDirection {
    Left,
    #[default]
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PioIrShiftConfig {
    pub in_direction: Option<PioIrShiftDirection>,
    pub out_direction: Option<PioIrShiftDirection>,
    pub autopush_threshold: Option<u8>,
    pub autopull_threshold: Option<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PioIrPinConfig {
    pub input_base: Option<u8>,
    pub input_count: Option<u8>,
    pub output_base: Option<u8>,
    pub output_count: Option<u8>,
    pub set_base: Option<u8>,
    pub set_count: Option<u8>,
    pub sideset_base: Option<u8>,
    pub sideset_count: Option<u8>,
    pub sideset_optional: bool,
    pub jmp_pin: Option<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PioIrClockConfig {
    pub divider_integer: Option<u16>,
    pub divider_fractional: Option<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PioIrExecutionConfig {
    pub clocking: PioIrClockConfig,
    pub pins: PioIrPinConfig,
    pub shift: PioIrShiftConfig,
    pub wrap_target: Option<u8>,
    pub wrap_source: Option<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PioIrInstructionTiming {
    pub stall_cycles: u8,
    pub sideset_bits: Option<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PioIrWaitCondition {
    GpioLow {
        pin: u8,
    },
    GpioHigh {
        pin: u8,
    },
    PinLow {
        pin: u8,
    },
    PinHigh {
        pin: u8,
    },
    JmpPinLow,
    JmpPinHigh,
    Irq {
        polarity: bool,
        relative: bool,
        index: u8,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PioIrJumpCondition {
    Always,
    XZero,
    XDecNonZero,
    YZero,
    YDecNonZero,
    XNotEqualY,
    PinHigh,
    OutputShiftCountBelowPullThreshold,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PioIrInSource {
    Pins,
    X,
    Y,
    Null,
    Status,
    Isr,
    Osr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PioIrOutDestination {
    Pins,
    X,
    Y,
    Null,
    PinDirs,
    Pc,
    Isr,
    Exec,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PioIrSetDestination {
    Pins,
    X,
    Y,
    PinDirs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PioIrMovDestination {
    Pins,
    X,
    Y,
    Exec,
    Pc,
    Isr,
    Osr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PioIrMovSource {
    Pins,
    X,
    Y,
    Null,
    Status,
    Isr,
    Osr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PioIrMovOperation {
    #[default]
    None,
    Invert,
    Reverse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PioIrIrqAction {
    Set,
    Wait,
    Clear,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PioIrInstruction {
    Nop,
    Delay {
        cycles: u8,
    },
    Wait(PioIrWaitCondition),
    Jump {
        condition: PioIrJumpCondition,
        target: u8,
    },
    In {
        source: PioIrInSource,
        bit_count: u8,
    },
    Out {
        destination: PioIrOutDestination,
        bit_count: u8,
    },
    Push {
        if_full: bool,
        blocking: bool,
    },
    Pull {
        if_empty: bool,
        blocking: bool,
    },
    Mov {
        destination: PioIrMovDestination,
        operation: PioIrMovOperation,
        source: PioIrMovSource,
    },
    Irq {
        action: PioIrIrqAction,
        relative: bool,
        index: u8,
    },
    Set {
        destination: PioIrSetDestination,
        value: u8,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PioIrProgram<'a> {
    pub id: PioProgramId,
    pub instructions: &'a [PioIrInstruction],
    pub timing: Option<&'a [PioIrInstructionTiming]>,
    pub execution: PioIrExecutionConfig,
}

impl<'a> PioIrProgram<'a> {
    #[must_use]
    pub const fn new(id: PioProgramId, instructions: &'a [PioIrInstruction]) -> Self {
        Self {
            id,
            instructions,
            timing: None,
            execution: PioIrExecutionConfig {
                clocking: PioIrClockConfig {
                    divider_integer: None,
                    divider_fractional: None,
                },
                pins: PioIrPinConfig {
                    input_base: None,
                    input_count: None,
                    output_base: None,
                    output_count: None,
                    set_base: None,
                    set_count: None,
                    sideset_base: None,
                    sideset_count: None,
                    sideset_optional: false,
                    jmp_pin: None,
                },
                shift: PioIrShiftConfig {
                    in_direction: None,
                    out_direction: None,
                    autopush_threshold: None,
                    autopull_threshold: None,
                },
                wrap_target: None,
                wrap_source: None,
            },
        }
    }

    #[must_use]
    pub const fn with_execution(mut self, execution: PioIrExecutionConfig) -> Self {
        self.execution = execution;
        self
    }

    #[must_use]
    pub const fn with_timing(mut self, timing: &'a [PioIrInstructionTiming]) -> Self {
        self.timing = Some(timing);
        self
    }

    #[must_use]
    pub const fn with_wrap(mut self, wrap_target: u8, wrap_source: u8) -> Self {
        self.execution.wrap_target = Some(wrap_target);
        self.execution.wrap_source = Some(wrap_source);
        self
    }
}
