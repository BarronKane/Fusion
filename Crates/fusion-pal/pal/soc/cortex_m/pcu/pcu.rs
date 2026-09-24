//! Cortex-M coprocessor backend.

use crate::contract::drivers::pcu::{
    PcuBaseContract,
    PcuCaps,
    PcuCommandOpCaps,
    PcuCommandSupport,
    PcuDirectStreamBackend,
    PcuExclusiveStreamBackend,
    PcuDispatchOpCaps,
    PcuDispatchPolicyCaps,
    PcuDispatchSupport,
    PcuError,
    PcuExecutorClass,
    PcuExecutorDescriptor,
    PcuExecutorId,
    PcuExecutorOrigin,
    PcuExecutorSupport,
    PcuFeatureSupport,
    PcuImplementationKind,
    PcuInvocationBindings,
    PcuInvocationParameters,
    PcuPersistentHandle,
    PcuPersistentState,
    PcuPrimitiveCaps,
    PcuPrimitiveSupport,
    PcuSignalOpCaps,
    PcuSignalSupport,
    PcuStreamInstallation,
    PcuStreamCapabilities,
    PcuStreamKernelIr,
    PcuStreamPattern,
    PcuPioU32StreamProfileError,
    validate_pio_u32_stream_invocation,
    validate_pio_u32_stream_profile,
    PcuStreamSupport,
    PcuSupport,
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

const CORTEX_M_PIO_EXECUTOR_SUPPORT: PcuExecutorSupport = PcuExecutorSupport {
    primitives: PcuPrimitiveCaps::STREAM,
    dispatch_policy: PcuDispatchPolicyCaps::PERSISTENT_INSTALL
        .union(PcuDispatchPolicyCaps::ORDERED_SUBMISSION),
    value_types: crate::contract::drivers::pcu::PcuValueTypeCaps::empty(),
    dispatch_instructions: PcuDispatchOpCaps::empty(),
    dispatch_features: crate::contract::drivers::pcu::PcuDispatchFeatureCaps::empty(),
    stream_instructions: CORTEX_M_PIO_STREAM_DIRECT_SUPPORT,
    command_instructions: PcuCommandOpCaps::empty(),
    transaction_features: PcuTransactionFeatureCaps::empty(),
    signal_instructions: PcuSignalOpCaps::empty(),
};

const fn pio_executor(id: u8, name: &'static str) -> PcuExecutorDescriptor {
    PcuExecutorDescriptor {
        id: PcuExecutorId(id),
        name,
        class: PcuExecutorClass::Io,
        origin: PcuExecutorOrigin::TopologyBound,
        support: CORTEX_M_PIO_EXECUTOR_SUPPORT,
    }
}

static CORTEX_M_EXECUTORS_0: [PcuExecutorDescriptor; 0] = [];
static CORTEX_M_EXECUTORS_1: [PcuExecutorDescriptor; 1] = [pio_executor(1, "cortex-m-pio0")];
static CORTEX_M_EXECUTORS_2: [PcuExecutorDescriptor; 2] = [
    pio_executor(1, "cortex-m-pio0"),
    pio_executor(2, "cortex-m-pio1"),
];
static CORTEX_M_EXECUTORS_3: [PcuExecutorDescriptor; 3] = [
    pio_executor(1, "cortex-m-pio0"),
    pio_executor(2, "cortex-m-pio1"),
    pio_executor(3, "cortex-m-pio2"),
];
static CORTEX_M_EXECUTORS_4: [PcuExecutorDescriptor; 4] = [
    pio_executor(1, "cortex-m-pio0"),
    pio_executor(2, "cortex-m-pio1"),
    pio_executor(3, "cortex-m-pio2"),
    pio_executor(4, "cortex-m-pio3"),
];
static CORTEX_M_EXECUTORS_5: [PcuExecutorDescriptor; 5] = [
    pio_executor(1, "cortex-m-pio0"),
    pio_executor(2, "cortex-m-pio1"),
    pio_executor(3, "cortex-m-pio2"),
    pio_executor(4, "cortex-m-pio3"),
    pio_executor(5, "cortex-m-pio4"),
];
static CORTEX_M_EXECUTORS_6: [PcuExecutorDescriptor; 6] = [
    pio_executor(1, "cortex-m-pio0"),
    pio_executor(2, "cortex-m-pio1"),
    pio_executor(3, "cortex-m-pio2"),
    pio_executor(4, "cortex-m-pio3"),
    pio_executor(5, "cortex-m-pio4"),
    pio_executor(6, "cortex-m-pio5"),
];
static CORTEX_M_EXECUTORS_7: [PcuExecutorDescriptor; 7] = [
    pio_executor(1, "cortex-m-pio0"),
    pio_executor(2, "cortex-m-pio1"),
    pio_executor(3, "cortex-m-pio2"),
    pio_executor(4, "cortex-m-pio3"),
    pio_executor(5, "cortex-m-pio4"),
    pio_executor(6, "cortex-m-pio5"),
    pio_executor(7, "cortex-m-pio6"),
];
static CORTEX_M_EXECUTORS_8: [PcuExecutorDescriptor; 8] = [
    pio_executor(1, "cortex-m-pio0"),
    pio_executor(2, "cortex-m-pio1"),
    pio_executor(3, "cortex-m-pio2"),
    pio_executor(4, "cortex-m-pio3"),
    pio_executor(5, "cortex-m-pio4"),
    pio_executor(6, "cortex-m-pio5"),
    pio_executor(7, "cortex-m-pio6"),
    pio_executor(8, "cortex-m-pio7"),
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
        features: PcuFeatureSupport::new(
            crate::contract::drivers::pcu::PcuDispatchFeatureCaps::empty(),
            crate::contract::drivers::pcu::PcuDispatchFeatureCaps::empty(),
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
            caps: if has_pio {
                PcuCaps::ENUMERATE_EXECUTORS
                    | PcuCaps::CLAIM_EXECUTOR
                    | PcuCaps::DISPATCH
                    | PcuCaps::COMPLETION_STATUS
                    | PcuCaps::EXTERNAL_RESOURCES
            } else {
                PcuCaps::ENUMERATE_EXECUTORS
            },
            implementation: if has_pio {
                map_pio_implementation_kind(support.implementation)
            } else {
                PcuImplementationKind::Unsupported
            },
            executor_count: u8::try_from(cortex_m_executors().len())
                .expect("executor count fits u8"),
            primitive_support: cortex_m_primitive_support(has_pio),
            value_type_support: PcuFeatureSupport::new(
                crate::contract::drivers::pcu::PcuValueTypeCaps::empty(),
                crate::contract::drivers::pcu::PcuValueTypeCaps::empty(),
            ),
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

#[derive(Debug, PartialEq, Eq)]
pub struct CortexMPioStreamHandle {
    engine_claim: PioEngineClaim,
    lane_claim: PioLaneClaim,
    lane: crate::pal::soc::cortex_m::hal::soc::pio::PioLaneId,
    lease: PioProgramLease,
    state: CortexMPersistentKernelState,
}

/// Opaque non-copyable claim for one selected Cortex-M PIO executor and lane.
///
/// Dropping an unconsumed lease releases its lane and engine claims. Successful installation
/// moves those claims into the stream handle, leaving this lease consumed.
#[derive(Debug, PartialEq, Eq)]
pub struct CortexMPioExecutorLease {
    executor: PcuExecutorId,
    engine_claim: Option<PioEngineClaim>,
    lane_claim: Option<PioLaneClaim>,
}

impl Drop for CortexMPioExecutorLease {
    fn drop(&mut self) {
        if let Some(lane_claim) = self.lane_claim.take() {
            let _ = system_pio().release_lanes(lane_claim);
        }
        if let Some(engine_claim) = self.engine_claim.take() {
            let _ = system_pio().release_engine(engine_claim);
        }
    }
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

impl PcuDirectStreamBackend for CortexMPcu {
    type StreamHandle = CortexMPioStreamHandle;

    fn install_stream_direct(
        &self,
        installation: PcuStreamInstallation<'_>,
        bindings: PcuInvocationBindings<'_>,
        parameters: PcuInvocationParameters<'_>,
    ) -> Result<Self::StreamHandle, PcuError> {
        validate_pio_u32_stream_invocation(installation.kernel, bindings, parameters)
            .map_err(pio_stream_profile_error)?;
        cortex_m_install_pio_stream(installation, bindings, parameters)
    }
}

impl PcuExclusiveStreamBackend for CortexMPcu {
    type Lease = CortexMPioExecutorLease;
    type StreamHandle = CortexMPioStreamHandle;

    fn claim_stream_executor(&self, executor: PcuExecutorId) -> Result<Self::Lease, PcuError> {
        let engine = pio_engine_for_executor(executor)?;
        let engine_claim = system_pio().claim_engine(engine)?;
        let lane_claim = match system_pio().claim_lanes(engine, PioLaneMask::from_lane(0)) {
            Ok(claim) => claim,
            Err(error) => {
                let _ = system_pio().release_engine(engine_claim);
                return Err(error);
            }
        };
        Ok(CortexMPioExecutorLease {
            executor,
            engine_claim: Some(engine_claim),
            lane_claim: Some(lane_claim),
        })
    }

    fn lease_executor(&self, lease: &Self::Lease) -> PcuExecutorId {
        lease.executor
    }

    fn install_stream_on_lease_direct(
        &self,
        lease: &mut Self::Lease,
        installation: PcuStreamInstallation<'_>,
        bindings: PcuInvocationBindings<'_>,
        parameters: PcuInvocationParameters<'_>,
    ) -> Result<Self::StreamHandle, PcuError> {
        validate_pio_u32_stream_invocation(installation.kernel, bindings, parameters)
            .map_err(pio_stream_profile_error)?;
        cortex_m_install_pio_stream_on_lease(lease, installation.kernel)
    }
}

fn cortex_m_install_pio_stream(
    installation: PcuStreamInstallation<'_>,
    bindings: PcuInvocationBindings<'_>,
    parameters: PcuInvocationParameters<'_>,
) -> Result<CortexMPioStreamHandle, PcuError> {
    let mut saw_busy = false;
    for descriptor in cortex_m_executors().iter().copied() {
        let mut lease =
            match PcuExclusiveStreamBackend::claim_stream_executor(&CortexMPcu, descriptor.id) {
                Ok(lease) => lease,
                Err(error) if error.kind() == PcuError::busy().kind() => {
                    saw_busy = true;
                    continue;
                }
                Err(error) => return Err(error),
            };
        return PcuExclusiveStreamBackend::install_stream_on_lease(
            &CortexMPcu,
            &mut lease,
            installation,
            bindings,
            parameters,
        );
    }
    if saw_busy {
        Err(PcuError::busy())
    } else {
        Err(PcuError::unsupported())
    }
}

fn cortex_m_install_pio_stream_on_lease(
    lease: &mut CortexMPioExecutorLease,
    kernel: &PcuStreamKernelIr<'_>,
) -> Result<CortexMPioStreamHandle, PcuError> {
    validate_pio_u32_stream_profile(kernel).map_err(pio_stream_profile_error)?;
    let engine_claim = lease
        .engine_claim
        .as_ref()
        .copied()
        .ok_or_else(PcuError::state_conflict)?;
    let lane_claim = lease
        .lane_claim
        .as_ref()
        .copied()
        .ok_or_else(PcuError::state_conflict)?;
    let engine = pio_engine_for_executor(lease.executor)?;
    let lane = claimed_pio_lane_for_executor(engine, engine_claim, lane_claim)?;
    let [pattern] = kernel.patterns else {
        return Err(PcuError::unsupported());
    };
    let program_lease =
        cortex_m_load_stream_pattern(engine_claim, lane_claim, kernel.id, *pattern)?;
    let engine_claim = lease
        .engine_claim
        .take()
        .ok_or_else(PcuError::state_conflict)?;
    let lane_claim = lease
        .lane_claim
        .take()
        .ok_or_else(PcuError::state_conflict)?;
    Ok(CortexMPioStreamHandle {
        engine_claim,
        lane_claim,
        lane,
        lease: program_lease,
        state: CortexMPersistentKernelState::Dormant,
    })
}

fn pio_engine_for_executor(
    executor: PcuExecutorId,
) -> Result<crate::pal::soc::cortex_m::hal::soc::pio::PioEngineId, PcuError> {
    let index = usize::from(executor.0);
    if index == 0 || index > pio_executor_count() {
        return Err(PcuError::invalid());
    }
    system_pio()
        .engines()
        .get(index - 1)
        .map(|engine| engine.id)
        .ok_or_else(PcuError::invalid)
}

fn claimed_pio_lane_for_executor(
    engine: crate::pal::soc::cortex_m::hal::soc::pio::PioEngineId,
    engine_claim: PioEngineClaim,
    lane_claim: PioLaneClaim,
) -> Result<crate::pal::soc::cortex_m::hal::soc::pio::PioLaneId, PcuError> {
    let lane = crate::pal::soc::cortex_m::hal::soc::pio::PioLaneId { engine, index: 0 };
    if engine_claim.engine() != engine
        || lane_claim.engine() != engine
        || !lane_claim.contains_lane(lane)
    {
        return Err(PcuError::state_conflict());
    }
    Ok(lane)
}

#[cfg(test)]
mod lease_tests {
    use super::claimed_pio_lane_for_executor;
    use crate::pal::soc::cortex_m::hal::soc::pio::{
        PioEngineClaim,
        PioEngineId,
        PioLaneClaim,
        PioLaneId,
        PioLaneMask,
    };

    #[test]
    fn lease_claims_must_match_the_selected_engine_lane() {
        let engine = PioEngineId(1);
        let lane = claimed_pio_lane_for_executor(
            engine,
            PioEngineClaim { engine },
            PioLaneClaim {
                engine,
                lanes: PioLaneMask::from_lane(0),
            },
        )
        .expect("matching engine and lane claims should be accepted");
        assert_eq!(lane, PioLaneId { engine, index: 0 });

        assert!(
            claimed_pio_lane_for_executor(
                PioEngineId(2),
                PioEngineClaim { engine },
                PioLaneClaim {
                    engine,
                    lanes: PioLaneMask::from_lane(0),
                },
            )
            .is_err()
        );
    }
}

const fn pio_stream_profile_error(error: PcuPioU32StreamProfileError) -> PcuError {
    match error {
        PcuPioU32StreamProfileError::InvalidPortCount
        | PcuPioU32StreamProfileError::InvalidPortShape
        | PcuPioU32StreamProfileError::InvalidValueType(_)
        | PcuPioU32StreamProfileError::InvalidPattern(_) => PcuError::invalid(),
        PcuPioU32StreamProfileError::KernelBindingsPresent
        | PcuPioU32StreamProfileError::ParametersPresent
        | PcuPioU32StreamProfileError::InvalidPatternCount { .. }
        | PcuPioU32StreamProfileError::UnsupportedPattern(_)
        | PcuPioU32StreamProfileError::RuntimeBindingsPresent
        | PcuPioU32StreamProfileError::RuntimeParametersPresent => PcuError::unsupported(),
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
        cortex_m_initialize_pio_lanes(lane_claim, program.execution.wrap_target.unwrap_or(0))
    {
        let _ = system_pio().unload_program(&engine_claim, lease);
        return Err(error);
    }
    Ok(lease)
}

#[cfg(feature = "soc-rp2350")]
fn cortex_m_initialize_pio_lanes(claim: PioLaneClaim, initial_pc: u8) -> Result<(), PcuError> {
    board::initialize_pio_lanes(&claim, initial_pc)
}

#[cfg(not(feature = "soc-rp2350"))]
fn cortex_m_initialize_pio_lanes(_claim: PioLaneClaim, _initial_pc: u8) -> Result<(), PcuError> {
    Err(PcuError::unsupported())
}
