//! Portable deterministic programmable-IO kernel and execution-state vocabulary.

use super::PcuProgramId;

/// Direction of one shift engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PcuIrShiftDirection {
    /// Shift left.
    Left,
    /// Shift right.
    #[default]
    Right,
}

/// Runtime shift-engine configuration for one programmable-IO kernel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PcuIrShiftConfig {
    /// Input shift direction when explicitly configured.
    pub in_direction: Option<PcuIrShiftDirection>,
    /// Output shift direction when explicitly configured.
    pub out_direction: Option<PcuIrShiftDirection>,
    /// Auto-push threshold when explicitly configured.
    pub autopush_threshold: Option<u8>,
    /// Auto-pull threshold when explicitly configured.
    pub autopull_threshold: Option<u8>,
}

/// Runtime pin-window configuration for one programmable-IO kernel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PcuIrPinConfig {
    /// Input pin base when explicitly configured.
    pub input_base: Option<u8>,
    /// Number of visible input pins when explicitly configured.
    pub input_count: Option<u8>,
    /// Output pin base when explicitly configured.
    pub output_base: Option<u8>,
    /// Number of output pins asserted by OUT/MOV PINS when explicitly configured.
    pub output_count: Option<u8>,
    /// SET pin base when explicitly configured.
    pub set_base: Option<u8>,
    /// SET pin count when explicitly configured.
    pub set_count: Option<u8>,
    /// Side-set pin base when explicitly configured.
    pub sideset_base: Option<u8>,
    /// Side-set bit count when explicitly configured.
    pub sideset_count: Option<u8>,
    /// Whether side-set is optional when configured.
    pub sideset_optional: bool,
    /// Jump-pin selection when explicitly configured.
    pub jmp_pin: Option<u8>,
}

/// Runtime clock divider configuration for one programmable-IO kernel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PcuIrClockConfig {
    /// Integer divider component when explicitly configured.
    pub divider_integer: Option<u16>,
    /// Fractional divider component when explicitly configured.
    pub divider_fractional: Option<u8>,
}

/// Runtime execution configuration for one programmable-IO kernel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PcuIrExecutionConfig {
    /// Clock-divider configuration.
    pub clocking: PcuIrClockConfig,
    /// Pin-window and side-set configuration.
    pub pins: PcuIrPinConfig,
    /// Shift/autopull/autopush configuration.
    pub shift: PcuIrShiftConfig,
    /// Wrap target when explicitly configured.
    pub wrap_target: Option<u8>,
    /// Wrap source when explicitly configured.
    pub wrap_source: Option<u8>,
}

/// WAIT-condition vocabulary portable across the current programmable-IO model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuIrWaitCondition {
    /// Wait until one pin reads low.
    PinLow { pin: u8 },
    /// Wait until one pin reads high.
    PinHigh { pin: u8 },
    /// Wait until one IRQ or event flag matches the requested polarity.
    Irq {
        /// Wait for set when `true`, cleared when `false`.
        polarity: bool,
        /// Whether the IRQ index is relative to the current lane.
        relative: bool,
        /// IRQ or event index.
        index: u8,
    },
}

/// Conditional branch vocabulary for one programmable-IO kernel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuIrJumpCondition {
    /// Unconditional jump.
    Always,
    /// Jump when scratch X is zero.
    XZero,
    /// Jump when scratch X is non-zero and post-decrement it.
    XDecNonZero,
    /// Jump when scratch Y is zero.
    YZero,
    /// Jump when scratch Y is non-zero and post-decrement it.
    YDecNonZero,
    /// Jump when scratch X does not equal scratch Y.
    XNotEqualY,
    /// Jump when the selected jump pin is high.
    PinHigh,
    /// Jump when the output shift register is not empty.
    OsrNotEmpty,
}

/// Valid sources for the IN instruction family.
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

/// Valid destinations for the OUT instruction family.
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

/// Valid destinations for the SET instruction family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuIrSetDestination {
    Pins,
    X,
    Y,
    PinDirs,
}

/// Valid destinations for the MOV instruction family.
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

/// Valid sources for the MOV instruction family.
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

/// MOV instruction modifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PcuIrMovOperation {
    /// Plain move.
    #[default]
    None,
    /// Invert source bits before the move.
    Invert,
    /// Reverse source bits before the move.
    Reverse,
}

/// IRQ instruction family actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuIrIrqAction {
    /// Set one IRQ flag.
    Set,
    /// Wait for one IRQ flag operation to complete.
    Wait,
    /// Clear one IRQ flag.
    Clear,
}

/// Portable programmable-IO instruction vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuIrInstruction {
    /// Consume one cycle without performing a state change.
    Nop,
    /// Delay execution for the supplied number of engine cycles.
    Delay { cycles: u8 },
    /// Wait for one deterministic condition.
    Wait(PcuIrWaitCondition),
    /// Conditional or unconditional branch.
    Jump {
        condition: PcuIrJumpCondition,
        target: u8,
    },
    /// Shift data into lane-local state.
    In {
        source: PcuIrInSource,
        bit_count: u8,
    },
    /// Shift data out of lane-local state.
    Out {
        destination: PcuIrOutDestination,
        bit_count: u8,
    },
    /// Push the input shift register into the RX path.
    Push { if_full: bool, blocking: bool },
    /// Pull one word from the TX path into the output shift register.
    Pull { if_empty: bool, blocking: bool },
    /// Move one value between lane-local sources and destinations.
    Mov {
        destination: PcuIrMovDestination,
        operation: PcuIrMovOperation,
        source: PcuIrMovSource,
    },
    /// Execute one IRQ-family operation.
    Irq {
        action: PcuIrIrqAction,
        relative: bool,
        index: u8,
    },
    /// Apply one small immediate value to a destination.
    Set {
        destination: PcuIrSetDestination,
        value: u8,
    },
}

/// One portable programmable-IO kernel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuIrProgram<'a> {
    /// Stable caller-supplied program identifier.
    pub id: PcuProgramId,
    /// Portable instruction stream.
    pub instructions: &'a [PcuIrInstruction],
    /// Execution-state bundle for this kernel.
    pub execution: PcuIrExecutionConfig,
}

impl<'a> PcuIrProgram<'a> {
    /// Creates one portable programmable-IO kernel with default execution config.
    #[must_use]
    pub const fn new(id: PcuProgramId, instructions: &'a [PcuIrInstruction]) -> Self {
        Self {
            id,
            instructions,
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

    /// Returns one copy of this kernel with explicit execution-state configuration.
    #[must_use]
    pub const fn with_execution(mut self, execution: PcuIrExecutionConfig) -> Self {
        self.execution = execution;
        self
    }

    /// Returns one copy of this kernel with explicit wrap bounds.
    #[must_use]
    pub const fn with_wrap(mut self, wrap_target: u8, wrap_source: u8) -> Self {
        self.execution.wrap_target = Some(wrap_target);
        self.execution.wrap_source = Some(wrap_source);
        self
    }
}
