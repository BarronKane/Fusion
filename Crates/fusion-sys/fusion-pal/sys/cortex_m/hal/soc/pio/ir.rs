//! Portable deterministic programmable-IO kernel and execution-state vocabulary.

use super::PcuProgramId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PcuIrShiftDirection {
    Left,
    #[default]
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PcuIrShiftConfig {
    pub in_direction: Option<PcuIrShiftDirection>,
    pub out_direction: Option<PcuIrShiftDirection>,
    pub autopush_threshold: Option<u8>,
    pub autopull_threshold: Option<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PcuIrPinConfig {
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
pub struct PcuIrClockConfig {
    pub divider_integer: Option<u16>,
    pub divider_fractional: Option<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PcuIrExecutionConfig {
    pub clocking: PcuIrClockConfig,
    pub pins: PcuIrPinConfig,
    pub shift: PcuIrShiftConfig,
    pub wrap_target: Option<u8>,
    pub wrap_source: Option<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PcuIrInstructionTiming {
    pub stall_cycles: u8,
    pub sideset_bits: Option<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuIrWaitCondition {
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
pub enum PcuIrJumpCondition {
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
pub enum PcuIrInSource {
    Pins,
    X,
    Y,
    Null,
    Status,
    Isr,
    Osr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuIrOutDestination {
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
pub enum PcuIrSetDestination {
    Pins,
    X,
    Y,
    PinDirs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuIrMovDestination {
    Pins,
    X,
    Y,
    Exec,
    Pc,
    Isr,
    Osr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuIrMovSource {
    Pins,
    X,
    Y,
    Null,
    Status,
    Isr,
    Osr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PcuIrMovOperation {
    #[default]
    None,
    Invert,
    Reverse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuIrIrqAction {
    Set,
    Wait,
    Clear,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuIrInstruction {
    Nop,
    Delay {
        cycles: u8,
    },
    Wait(PcuIrWaitCondition),
    Jump {
        condition: PcuIrJumpCondition,
        target: u8,
    },
    In {
        source: PcuIrInSource,
        bit_count: u8,
    },
    Out {
        destination: PcuIrOutDestination,
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
        destination: PcuIrMovDestination,
        operation: PcuIrMovOperation,
        source: PcuIrMovSource,
    },
    Irq {
        action: PcuIrIrqAction,
        relative: bool,
        index: u8,
    },
    Set {
        destination: PcuIrSetDestination,
        value: u8,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuIrProgram<'a> {
    pub id: PcuProgramId,
    pub instructions: &'a [PcuIrInstruction],
    pub timing: Option<&'a [PcuIrInstructionTiming]>,
    pub execution: PcuIrExecutionConfig,
}

impl<'a> PcuIrProgram<'a> {
    #[must_use]
    pub const fn new(id: PcuProgramId, instructions: &'a [PcuIrInstruction]) -> Self {
        Self {
            id,
            instructions,
            timing: None,
            execution: PcuIrExecutionConfig {
                clocking: PcuIrClockConfig {
                    divider_integer: None,
                    divider_fractional: None,
                },
                pins: PcuIrPinConfig {
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
                shift: PcuIrShiftConfig {
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
    pub const fn with_execution(mut self, execution: PcuIrExecutionConfig) -> Self {
        self.execution = execution;
        self
    }

    #[must_use]
    pub const fn with_timing(mut self, timing: &'a [PcuIrInstructionTiming]) -> Self {
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
