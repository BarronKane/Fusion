//! Cortex-M coprocessor backend.

use core::sync::atomic::{
    AtomicBool,
    Ordering,
};

use crate::contract::drivers::pcu::{
    PcuBaseContract,
    PcuCaps,
    PcuCommandOpCaps,
    PcuCommandSubmission,
    PcuCommandSupport,
    PcuControlContract,
    PcuDirectDispatchBackend,
    PcuDispatchOpCaps,
    PcuDispatchPolicyCaps,
    PcuDispatchSubmission,
    PcuDispatchSupport,
    PcuError,
    PcuExecutorClaim,
    PcuExecutorClass,
    PcuExecutorDescriptor,
    PcuExecutorId,
    PcuExecutorOrigin,
    PcuExecutorSupport,
    PcuFeatureSupport,
    PcuFiniteHandle,
    PcuFiniteState,
    PcuImplementationKind,
    PcuInvocationBindings,
    PcuInvocationParameters,
    PcuPersistentHandle,
    PcuPersistentState,
    PcuPrimitiveCaps,
    PcuPrimitiveSupport,
    PcuSignalInstallation,
    PcuSignalOpCaps,
    PcuSignalSupport,
    PcuStreamInstallation,
    PcuStreamCapabilities,
    PcuStreamKernelIr,
    PcuStreamPattern,
    PcuStreamValueType,
    PcuStreamSupport,
    PcuSupport,
    PcuTransactionSubmission,
    PcuTransactionFeatureCaps,
    PcuTransactionSupport,
};
use crate::pal::soc::cortex_m::hal::soc::pio::{
    PioControl,
    PioEngineClaim,
    PioImplementationKind,
    PioIrInstruction,
    PioLaneClaim,
    PioLaneMask,
    PioProgramId,
    PioProgramLease,
    PioBase,
    PcuIrProgram,
    bit_invert_stream_transform,
    bit_reverse_stream_transform,
    byte_swap32_stream_transform,
    decrement_stream_transform,
    extract_bits_stream_transform,
    increment_stream_transform,
    lower_rp2350_program,
    mask_lower_stream_transform,
    rp2350_build_execution_registers,
    shift_left_stream_transform,
    shift_right_stream_transform,
    system_pio,
};
use crate::pal::soc::cortex_m::hal::soc::board;

const CORTEX_M_CPU_EXECUTOR_ID: PcuExecutorId = PcuExecutorId(0);
const MAX_CORTEX_M_PIO_EXECUTORS: usize = 8;

const CORTEX_M_PIO_STREAM_DIRECT_SUPPORT: PcuStreamCapabilities = PcuStreamCapabilities::FIFO_INPUT
    .union(PcuStreamCapabilities::FIFO_OUTPUT)
    .union(PcuStreamCapabilities::BIT_REVERSE)
    .union(PcuStreamCapabilities::BIT_INVERT)
    .union(PcuStreamCapabilities::INCREMENT)
    .union(PcuStreamCapabilities::DECREMENT)
    .union(PcuStreamCapabilities::SHIFT_LEFT)
    .union(PcuStreamCapabilities::SHIFT_RIGHT)
    .union(PcuStreamCapabilities::EXTRACT_BITS)
    .union(PcuStreamCapabilities::MASK_LOWER)
    .union(PcuStreamCapabilities::BYTE_SWAP32);

const CORTEX_M_CPU_EXECUTOR_SUPPORT: PcuExecutorSupport = PcuExecutorSupport::unsupported();

const CORTEX_M_PIO_EXECUTOR_SUPPORT: PcuExecutorSupport = PcuExecutorSupport {
    primitives: PcuPrimitiveCaps::STREAM,
    dispatch_policy: PcuDispatchPolicyCaps::PERSISTENT_INSTALL
        .union(PcuDispatchPolicyCaps::ORDERED_SUBMISSION),
    dispatch_instructions: PcuDispatchOpCaps::empty(),
    stream_instructions: CORTEX_M_PIO_STREAM_DIRECT_SUPPORT,
    command_instructions: PcuCommandOpCaps::empty(),
    transaction_features: PcuTransactionFeatureCaps::empty(),
    signal_instructions: PcuSignalOpCaps::empty(),
};

const fn cpu_executor() -> PcuExecutorDescriptor {
    PcuExecutorDescriptor {
        id: CORTEX_M_CPU_EXECUTOR_ID,
        name: "cortex-m-cpu",
        class: PcuExecutorClass::Cpu,
        origin: PcuExecutorOrigin::Synthetic,
        support: CORTEX_M_CPU_EXECUTOR_SUPPORT,
    }
}

const fn pio_executor(id: u8, name: &'static str) -> PcuExecutorDescriptor {
    PcuExecutorDescriptor {
        id: PcuExecutorId(id),
        name,
        class: PcuExecutorClass::Io,
        origin: PcuExecutorOrigin::TopologyBound,
        support: CORTEX_M_PIO_EXECUTOR_SUPPORT,
    }
}

static CORTEX_M_EXECUTORS_0: [PcuExecutorDescriptor; 1] = [cpu_executor()];
static CORTEX_M_EXECUTORS_1: [PcuExecutorDescriptor; 2] =
    [cpu_executor(), pio_executor(1, "cortex-m-pio0")];
static CORTEX_M_EXECUTORS_2: [PcuExecutorDescriptor; 3] = [
    cpu_executor(),
    pio_executor(1, "cortex-m-pio0"),
    pio_executor(2, "cortex-m-pio1"),
];
static CORTEX_M_EXECUTORS_3: [PcuExecutorDescriptor; 4] = [
    cpu_executor(),
    pio_executor(1, "cortex-m-pio0"),
    pio_executor(2, "cortex-m-pio1"),
    pio_executor(3, "cortex-m-pio2"),
];
static CORTEX_M_EXECUTORS_4: [PcuExecutorDescriptor; 5] = [
    cpu_executor(),
    pio_executor(1, "cortex-m-pio0"),
    pio_executor(2, "cortex-m-pio1"),
    pio_executor(3, "cortex-m-pio2"),
    pio_executor(4, "cortex-m-pio3"),
];
static CORTEX_M_EXECUTORS_5: [PcuExecutorDescriptor; 6] = [
    cpu_executor(),
    pio_executor(1, "cortex-m-pio0"),
    pio_executor(2, "cortex-m-pio1"),
    pio_executor(3, "cortex-m-pio2"),
    pio_executor(4, "cortex-m-pio3"),
    pio_executor(5, "cortex-m-pio4"),
];
static CORTEX_M_EXECUTORS_6: [PcuExecutorDescriptor; 7] = [
    cpu_executor(),
    pio_executor(1, "cortex-m-pio0"),
    pio_executor(2, "cortex-m-pio1"),
    pio_executor(3, "cortex-m-pio2"),
    pio_executor(4, "cortex-m-pio3"),
    pio_executor(5, "cortex-m-pio4"),
    pio_executor(6, "cortex-m-pio5"),
];
static CORTEX_M_EXECUTORS_7: [PcuExecutorDescriptor; 8] = [
    cpu_executor(),
    pio_executor(1, "cortex-m-pio0"),
    pio_executor(2, "cortex-m-pio1"),
    pio_executor(3, "cortex-m-pio2"),
    pio_executor(4, "cortex-m-pio3"),
    pio_executor(5, "cortex-m-pio4"),
    pio_executor(6, "cortex-m-pio5"),
    pio_executor(7, "cortex-m-pio6"),
];
static CORTEX_M_EXECUTORS_8: [PcuExecutorDescriptor; 9] = [
    cpu_executor(),
    pio_executor(1, "cortex-m-pio0"),
    pio_executor(2, "cortex-m-pio1"),
    pio_executor(3, "cortex-m-pio2"),
    pio_executor(4, "cortex-m-pio3"),
    pio_executor(5, "cortex-m-pio4"),
    pio_executor(6, "cortex-m-pio5"),
    pio_executor(7, "cortex-m-pio6"),
    pio_executor(8, "cortex-m-pio7"),
];
static CORTEX_M_CPU_EXECUTOR_CLAIMED: AtomicBool = AtomicBool::new(false);
static CORTEX_M_PIO_EXECUTOR_CLAIMED: [AtomicBool; MAX_CORTEX_M_PIO_EXECUTORS] = [
    AtomicBool::new(false),
    AtomicBool::new(false),
    AtomicBool::new(false),
    AtomicBool::new(false),
    AtomicBool::new(false),
    AtomicBool::new(false),
    AtomicBool::new(false),
    AtomicBool::new(false),
];

fn pio_executor_count() -> usize {
    core::cmp::min(system_pio().engines().len(), MAX_CORTEX_M_PIO_EXECUTORS)
}

const fn cortex_m_primitive_support(has_pio: bool) -> PcuPrimitiveSupport {
    PcuPrimitiveSupport {
        primitives: PcuFeatureSupport::new(
            if has_pio {
                PcuPrimitiveCaps::STREAM
            } else {
                PcuPrimitiveCaps::empty()
            },
            PcuPrimitiveCaps::empty(),
        ),
    }
}

const fn cortex_m_dispatch_support(has_pio: bool) -> PcuDispatchSupport {
    PcuDispatchSupport {
        flags: if has_pio {
            PcuDispatchPolicyCaps::PERSISTENT_INSTALL
                .union(PcuDispatchPolicyCaps::ORDERED_SUBMISSION)
        } else {
            PcuDispatchPolicyCaps::empty()
        },
        instructions: PcuFeatureSupport::new(
            PcuDispatchOpCaps::empty(),
            PcuDispatchOpCaps::empty(),
        ),
    }
}

const fn cortex_m_stream_support(has_pio: bool) -> PcuStreamSupport {
    PcuStreamSupport {
        instructions: PcuFeatureSupport::new(
            if has_pio {
                CORTEX_M_PIO_STREAM_DIRECT_SUPPORT
            } else {
                PcuStreamCapabilities::empty()
            },
            PcuStreamCapabilities::empty(),
        ),
    }
}

const fn cortex_m_command_support() -> PcuCommandSupport {
    PcuCommandSupport {
        instructions: PcuFeatureSupport::new(PcuCommandOpCaps::empty(), PcuCommandOpCaps::empty()),
    }
}

const fn cortex_m_transaction_support() -> PcuTransactionSupport {
    PcuTransactionSupport {
        features: PcuFeatureSupport::new(
            PcuTransactionFeatureCaps::empty(),
            PcuTransactionFeatureCaps::empty(),
        ),
    }
}

const fn cortex_m_signal_support() -> PcuSignalSupport {
    PcuSignalSupport {
        instructions: PcuFeatureSupport::new(PcuSignalOpCaps::empty(), PcuSignalOpCaps::empty()),
    }
}

fn cortex_m_executors() -> &'static [PcuExecutorDescriptor] {
    match pio_executor_count() {
        0 => &CORTEX_M_EXECUTORS_0,
        1 => &CORTEX_M_EXECUTORS_1,
        2 => &CORTEX_M_EXECUTORS_2,
        3 => &CORTEX_M_EXECUTORS_3,
        4 => &CORTEX_M_EXECUTORS_4,
        5 => &CORTEX_M_EXECUTORS_5,
        6 => &CORTEX_M_EXECUTORS_6,
        7 => &CORTEX_M_EXECUTORS_7,
        _ => &CORTEX_M_EXECUTORS_8,
    }
}

/// Cortex-M coprocessor provider type.
#[derive(Debug, Clone, Copy, Default)]
pub struct CortexMPcu;

/// Selected Cortex-M programmable-IO provider type.
pub type PlatformPcu = CortexMPcu;

/// Returns the selected Cortex-M coprocessor provider.
#[must_use]
pub const fn system_pcu() -> PlatformPcu {
    PlatformPcu::new()
}

impl CortexMPcu {
    /// Creates a new Cortex-M coprocessor provider handle.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

const fn map_pio_implementation_kind(
    implementation: PioImplementationKind,
) -> PcuImplementationKind {
    match implementation {
        PioImplementationKind::Native => PcuImplementationKind::Native,
        PioImplementationKind::Emulated => PcuImplementationKind::Emulated,
        PioImplementationKind::Unsupported => PcuImplementationKind::Unsupported,
    }
}

impl PcuBaseContract for CortexMPcu {
    fn support(&self) -> PcuSupport {
        let support = system_pio().support();
        let has_pio = support.engine_count != 0;
        PcuSupport {
            caps: PcuCaps::ENUMERATE_EXECUTORS
                | PcuCaps::CLAIM_EXECUTOR
                | PcuCaps::DISPATCH
                | PcuCaps::COMPLETION_STATUS
                | PcuCaps::EXTERNAL_RESOURCES,
            // Overall generic-PCU support is still native on CPU-only Cortex-M targets even when
            // no topology-bound PIO executor is surfaced.
            implementation: if support.engine_count == 0 {
                PcuImplementationKind::Native
            } else {
                map_pio_implementation_kind(support.implementation)
            },
            executor_count: cortex_m_executors().len() as u8,
            primitive_support: cortex_m_primitive_support(has_pio),
            dispatch_support: cortex_m_dispatch_support(has_pio),
            stream_support: cortex_m_stream_support(has_pio),
            command_support: cortex_m_command_support(),
            transaction_support: cortex_m_transaction_support(),
            signal_support: cortex_m_signal_support(),
        }
    }

    fn executors(&self) -> &'static [PcuExecutorDescriptor] {
        cortex_m_executors()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CortexMPersistentKernelState {
    Dormant,
    Active,
    Stopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CortexMUnsupportedFiniteHandle;

impl PcuFiniteHandle for CortexMUnsupportedFiniteHandle {
    fn state(&self) -> Result<PcuFiniteState, PcuError> {
        Err(PcuError::unsupported())
    }

    fn wait(self) -> Result<(), PcuError> {
        Err(PcuError::unsupported())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CortexMUnsupportedPersistentHandle;

impl PcuPersistentHandle for CortexMUnsupportedPersistentHandle {
    fn state(&self) -> Result<PcuPersistentState, PcuError> {
        Err(PcuError::unsupported())
    }

    fn start(&mut self) -> Result<(), PcuError> {
        Err(PcuError::unsupported())
    }

    fn stop(&mut self) -> Result<(), PcuError> {
        Err(PcuError::unsupported())
    }

    fn uninstall(self) -> Result<(), PcuError> {
        Err(PcuError::unsupported())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CortexMPioStreamHandle {
    engine_claim: PioEngineClaim,
    lane_claim: PioLaneClaim,
    lane: crate::pal::soc::cortex_m::hal::soc::pio::PioLaneId,
    lease: PioProgramLease,
    state: CortexMPersistentKernelState,
}

impl PcuPersistentHandle for CortexMPioStreamHandle {
    fn state(&self) -> Result<PcuPersistentState, PcuError> {
        Ok(match self.state {
            CortexMPersistentKernelState::Dormant => PcuPersistentState::Dormant,
            CortexMPersistentKernelState::Active => PcuPersistentState::Active,
            CortexMPersistentKernelState::Stopped => PcuPersistentState::Stopped,
        })
    }

    fn start(&mut self) -> Result<(), PcuError> {
        if matches!(self.state, CortexMPersistentKernelState::Active) {
            return Err(PcuError::state_conflict());
        }
        system_pio().start_lanes(&self.lane_claim)?;
        self.state = CortexMPersistentKernelState::Active;
        Ok(())
    }

    fn stop(&mut self) -> Result<(), PcuError> {
        if !matches!(self.state, CortexMPersistentKernelState::Active) {
            return Err(PcuError::state_conflict());
        }
        system_pio().stop_lanes(&self.lane_claim)?;
        self.state = CortexMPersistentKernelState::Stopped;
        Ok(())
    }

    fn uninstall(mut self) -> Result<(), PcuError> {
        if matches!(self.state, CortexMPersistentKernelState::Active) {
            system_pio().stop_lanes(&self.lane_claim)?;
            self.state = CortexMPersistentKernelState::Stopped;
        }
        system_pio().unload_program(&self.engine_claim, self.lease)?;
        system_pio().release_lanes(self.lane_claim)?;
        system_pio().release_engine(self.engine_claim)?;
        Ok(())
    }
}

impl CortexMPioStreamHandle {
    /// Writes one input word into the active PIO stream.
    ///
    /// # Errors
    ///
    /// Returns `StateConflict` when the installed stream is not active or any honest FIFO write
    /// failure from the PIO backend.
    pub fn write_word(&mut self, word: u32) -> Result<(), PcuError> {
        if !matches!(self.state, CortexMPersistentKernelState::Active) {
            return Err(PcuError::state_conflict());
        }
        system_pio().write_tx_fifo(&self.lane_claim, self.lane, word)
    }

    /// Reads one output word from the active PIO stream.
    ///
    /// # Errors
    ///
    /// Returns `StateConflict` when the installed stream is not active or any honest FIFO read
    /// failure from the PIO backend.
    pub fn read_word(&mut self) -> Result<u32, PcuError> {
        if !matches!(self.state, CortexMPersistentKernelState::Active) {
            return Err(PcuError::state_conflict());
        }
        system_pio().read_rx_fifo(&self.lane_claim, self.lane)
    }
}

impl PcuDirectDispatchBackend for CortexMPcu {
    type DispatchHandle = CortexMUnsupportedFiniteHandle;
    type CommandHandle = CortexMUnsupportedFiniteHandle;
    type TransactionHandle = CortexMUnsupportedFiniteHandle;
    type StreamHandle = CortexMPioStreamHandle;
    type SignalHandle = CortexMUnsupportedPersistentHandle;

    fn submit_dispatch_direct(
        &self,
        _submission: PcuDispatchSubmission<'_>,
        _bindings: PcuInvocationBindings<'_>,
        _parameters: PcuInvocationParameters<'_>,
    ) -> Result<Self::DispatchHandle, PcuError> {
        Err(PcuError::unsupported())
    }

    fn submit_command_direct(
        &self,
        _submission: PcuCommandSubmission<'_>,
        _parameters: PcuInvocationParameters<'_>,
    ) -> Result<Self::CommandHandle, PcuError> {
        Err(PcuError::unsupported())
    }

    fn submit_transaction_direct(
        &self,
        _submission: PcuTransactionSubmission<'_>,
        _bindings: PcuInvocationBindings<'_>,
        _parameters: PcuInvocationParameters<'_>,
    ) -> Result<Self::TransactionHandle, PcuError> {
        Err(PcuError::unsupported())
    }

    fn install_stream_direct(
        &self,
        installation: PcuStreamInstallation<'_>,
        bindings: PcuInvocationBindings<'_>,
        parameters: PcuInvocationParameters<'_>,
    ) -> Result<Self::StreamHandle, PcuError> {
        if !bindings.is_empty()
            || !parameters.is_empty()
            || !installation.kernel.parameters.is_empty()
        {
            return Err(PcuError::unsupported());
        }
        cortex_m_install_pio_stream(installation.kernel)
    }

    fn install_signal_direct(
        &self,
        _installation: PcuSignalInstallation<'_>,
        _parameters: PcuInvocationParameters<'_>,
    ) -> Result<Self::SignalHandle, PcuError> {
        Err(PcuError::unsupported())
    }
}

fn cortex_m_install_pio_stream(
    kernel: &PcuStreamKernelIr<'_>,
) -> Result<CortexMPioStreamHandle, PcuError> {
    if kernel.validate_simple_transform().is_err() {
        return Err(PcuError::invalid());
    }

    let [pattern] = kernel.patterns else {
        return Err(PcuError::unsupported());
    };
    if kernel.simple_transform_type() != Some(PcuStreamValueType::U32) {
        return Err(PcuError::unsupported());
    }

    let (engine_claim, lane_claim) = claim_first_available_pio_lane()?;
    let lane = crate::pal::soc::cortex_m::hal::soc::pio::PioLaneId {
        engine: engine_claim.engine(),
        index: 0,
    };
    let install_result =
        cortex_m_load_stream_pattern(engine_claim, lane_claim, kernel.id, *pattern);
    match install_result {
        Ok(lease) => Ok(CortexMPioStreamHandle {
            engine_claim,
            lane_claim,
            lane,
            lease,
            state: CortexMPersistentKernelState::Dormant,
        }),
        Err(error) => {
            let _ = system_pio().release_lanes(lane_claim);
            let _ = system_pio().release_engine(engine_claim);
            Err(error)
        }
    }
}

fn claim_first_available_pio_lane() -> Result<(PioEngineClaim, PioLaneClaim), PcuError> {
    let mut saw_busy = false;
    for engine in system_pio().engines().iter().copied() {
        let engine_claim = match system_pio().claim_engine(engine.id) {
            Ok(claim) => claim,
            Err(error) if error.kind() == PcuError::busy().kind() => {
                saw_busy = true;
                continue;
            }
            Err(error) => return Err(error),
        };

        let lane_claim = match system_pio().claim_lanes(engine.id, PioLaneMask::from_lane(0)) {
            Ok(claim) => claim,
            Err(error) => {
                let _ = system_pio().release_engine(engine_claim);
                if error.kind() == PcuError::busy().kind() {
                    saw_busy = true;
                    continue;
                }
                return Err(error);
            }
        };

        return Ok((engine_claim, lane_claim));
    }

    if saw_busy {
        Err(PcuError::busy())
    } else {
        Err(PcuError::unsupported())
    }
}

fn cortex_m_load_stream_pattern(
    engine_claim: PioEngineClaim,
    lane_claim: PioLaneClaim,
    kernel_id: crate::contract::drivers::pcu::PcuKernelId,
    pattern: PcuStreamPattern,
) -> Result<PioProgramLease, PcuError> {
    let program_id = PioProgramId(kernel_id.0);
    match pattern {
        PcuStreamPattern::BitReverse => {
            let mut instructions = [PioIrInstruction::Nop; 4];
            let program = bit_reverse_stream_transform(program_id, &mut instructions);
            cortex_m_install_pio_program(engine_claim, lane_claim, &program)
        }
        PcuStreamPattern::BitInvert => {
            let mut instructions = [PioIrInstruction::Nop; 4];
            let program = bit_invert_stream_transform(program_id, &mut instructions);
            cortex_m_install_pio_program(engine_claim, lane_claim, &program)
        }
        PcuStreamPattern::Increment => {
            let mut instructions = [PioIrInstruction::Nop; 8];
            let program = increment_stream_transform(program_id, &mut instructions);
            cortex_m_install_pio_program(engine_claim, lane_claim, &program)
        }
        PcuStreamPattern::Decrement => {
            let mut instructions = [PioIrInstruction::Nop; 6];
            let program = decrement_stream_transform(program_id, &mut instructions);
            cortex_m_install_pio_program(engine_claim, lane_claim, &program)
        }
        PcuStreamPattern::ShiftLeft { bits } => {
            let mut instructions = [PioIrInstruction::Nop; 5];
            let program = shift_left_stream_transform(program_id, bits, &mut instructions)?;
            cortex_m_install_pio_program(engine_claim, lane_claim, &program)
        }
        PcuStreamPattern::ShiftRight { bits } => {
            let mut instructions = [PioIrInstruction::Nop; 5];
            let program = shift_right_stream_transform(program_id, bits, &mut instructions)?;
            cortex_m_install_pio_program(engine_claim, lane_claim, &program)
        }
        PcuStreamPattern::ExtractBits { offset, width } => {
            let mut instructions = [PioIrInstruction::Nop; 6];
            let program =
                extract_bits_stream_transform(program_id, offset, width, &mut instructions)?;
            cortex_m_install_pio_program(engine_claim, lane_claim, &program)
        }
        PcuStreamPattern::MaskLower { bits } => {
            let mut instructions = [PioIrInstruction::Nop; 6];
            let program = mask_lower_stream_transform(program_id, bits, &mut instructions)?;
            cortex_m_install_pio_program(engine_claim, lane_claim, &program)
        }
        PcuStreamPattern::ByteSwap32 => {
            let mut instructions = [PioIrInstruction::Nop; 12];
            let program = byte_swap32_stream_transform(program_id, &mut instructions);
            cortex_m_install_pio_program(engine_claim, lane_claim, &program)
        }
        PcuStreamPattern::AddParameter { .. } | PcuStreamPattern::XorParameter { .. } => {
            Err(PcuError::unsupported())
        }
    }
}

fn cortex_m_install_pio_program(
    engine_claim: PioEngineClaim,
    lane_claim: PioLaneClaim,
    program: &PcuIrProgram<'_>,
) -> Result<PioProgramLease, PcuError> {
    let mut words = [0_u16; 32];
    let image = lower_rp2350_program(program, &mut words)?;
    let (clkdiv, execctrl, shiftctrl, pinctrl) =
        rp2350_build_execution_registers(&program.execution, Some(program.instructions))?;
    let lease = system_pio().load_program(&engine_claim, &image)?;
    if let Err(error) =
        board::apply_pio_execution_config(&lane_claim, clkdiv, execctrl, shiftctrl, pinctrl)
    {
        let _ = system_pio().unload_program(&engine_claim, lease);
        return Err(error);
    }
    if let Err(error) =
        cortex_m_initialize_pio_lanes(&lane_claim, program.execution.wrap_target.unwrap_or(0))
    {
        let _ = system_pio().unload_program(&engine_claim, lease);
        return Err(error);
    }
    Ok(lease)
}

#[cfg(feature = "soc-rp2350")]
fn cortex_m_initialize_pio_lanes(claim: &PioLaneClaim, initial_pc: u8) -> Result<(), PcuError> {
    board::initialize_pio_lanes(claim, initial_pc)
}

#[cfg(not(feature = "soc-rp2350"))]
fn cortex_m_initialize_pio_lanes(_claim: &PioLaneClaim, _initial_pc: u8) -> Result<(), PcuError> {
    Err(PcuError::unsupported())
}

impl PcuControlContract for CortexMPcu {
    fn claim_executor(&self, executor: PcuExecutorId) -> Result<PcuExecutorClaim, PcuError> {
        match executor {
            CORTEX_M_CPU_EXECUTOR_ID => {
                CORTEX_M_CPU_EXECUTOR_CLAIMED
                    .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                    .map_err(|_| PcuError::busy())?;
            }
            PcuExecutorId(index) => {
                if index == 0 || usize::from(index) > pio_executor_count() {
                    return Err(PcuError::invalid());
                }
                CORTEX_M_PIO_EXECUTOR_CLAIMED[usize::from(index - 1)]
                    .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                    .map_err(|_| PcuError::busy())?;
            }
        }
        Ok(PcuExecutorClaim::new(executor))
    }

    fn release_executor(&self, claim: PcuExecutorClaim) -> Result<(), PcuError> {
        match claim.executor() {
            CORTEX_M_CPU_EXECUTOR_ID => {
                if !CORTEX_M_CPU_EXECUTOR_CLAIMED.swap(false, Ordering::AcqRel) {
                    return Err(PcuError::state_conflict());
                }
            }
            PcuExecutorId(index) => {
                if index == 0 || usize::from(index) > pio_executor_count() {
                    return Err(PcuError::invalid());
                }
                if !CORTEX_M_PIO_EXECUTOR_CLAIMED[usize::from(index - 1)]
                    .swap(false, Ordering::AcqRel)
                {
                    return Err(PcuError::state_conflict());
                }
            }
        }
        Ok(())
    }
}
