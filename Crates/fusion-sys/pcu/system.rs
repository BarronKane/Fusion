//! fusion-sys programmable-IO wrapper over the selected fusion-pal backend.

use fusion_pal::sys::pcu::{PlatformPcu, system_pcu as pal_system_pcu};

use super::{
    PcuBase,
    PcuControl,
    PcuEngineClaim,
    PcuEngineDescriptor,
    PcuEngineId,
    PcuError,
    PcuIrProgram,
    PcuLaneClaim,
    PcuLaneDescriptor,
    PcuLaneId,
    PcuLaneMask,
    PcuProgramImage,
    PcuProgramLease,
    PcuProgramSource,
    PcuSupport,
};
#[cfg(any(test, all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")))]
use super::{
    PcuIrExecutionConfig,
    PcuIrInSource,
    PcuIrInstruction,
    PcuIrIrqAction,
    PcuIrJumpCondition,
    PcuIrMovDestination,
    PcuIrMovOperation,
    PcuIrMovSource,
    PcuIrOutDestination,
    PcuIrSetDestination,
    PcuIrWaitCondition,
};

#[cfg(any(test, all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")))]
const RP2350_PIO_INSTRUCTION_LIMIT: usize = 32;
#[cfg(any(test, all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")))]
const RP2350_PIO_MAJOR_JMP: u16 = 0x0000;
#[cfg(any(test, all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")))]
const RP2350_PIO_MAJOR_WAIT: u16 = 0x2000;
#[cfg(any(test, all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")))]
const RP2350_PIO_MAJOR_IN: u16 = 0x4000;
#[cfg(any(test, all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")))]
const RP2350_PIO_MAJOR_OUT: u16 = 0x6000;
#[cfg(any(test, all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")))]
const RP2350_PIO_MAJOR_PUSH: u16 = 0x8000;
#[cfg(any(test, all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")))]
const RP2350_PIO_MAJOR_PULL: u16 = 0x8080;
#[cfg(any(test, all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")))]
const RP2350_PIO_MAJOR_MOV: u16 = 0xa000;
#[cfg(any(test, all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")))]
const RP2350_PIO_MAJOR_IRQ: u16 = 0xc000;
#[cfg(any(test, all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")))]
const RP2350_PIO_MAJOR_SET: u16 = 0xe000;
#[cfg(any(test, all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")))]
const RP2350_PIO_SRC_DEST_PINS: u16 = 0;
#[cfg(any(test, all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")))]
const RP2350_PIO_SRC_DEST_X: u16 = 1;
#[cfg(any(test, all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")))]
const RP2350_PIO_SRC_DEST_Y: u16 = 2;
#[cfg(any(test, all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")))]
const RP2350_PIO_SRC_DEST_NULL: u16 = 3;
#[cfg(any(test, all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")))]
const RP2350_PIO_SRC_DEST_PINDIRS: u16 = 4;
#[cfg(any(test, all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")))]
const RP2350_PIO_SRC_DEST_EXEC: u16 = 4;
#[cfg(any(test, all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")))]
const RP2350_PIO_SRC_DEST_STATUS: u16 = 5;
#[cfg(any(test, all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")))]
const RP2350_PIO_SRC_DEST_PC: u16 = 5;
#[cfg(any(test, all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")))]
const RP2350_PIO_SRC_DEST_ISR: u16 = 6;
#[cfg(any(test, all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")))]
const RP2350_PIO_SRC_DEST_OSR: u16 = 7;

#[cfg(any(test, all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")))]
const fn rp2350_encode_instr_and_args(instr_bits: u16, arg1: u16, arg2: u16) -> u16 {
    instr_bits | (arg1 << 5) | (arg2 & 0x1f)
}

#[cfg(any(test, all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")))]
const fn rp2350_encode_jmp_condition(condition: PcuIrJumpCondition, target: u8) -> u16 {
    let arg1 = match condition {
        PcuIrJumpCondition::Always => 0,
        PcuIrJumpCondition::XZero => 1,
        PcuIrJumpCondition::XDecNonZero => 2,
        PcuIrJumpCondition::YZero => 3,
        PcuIrJumpCondition::YDecNonZero => 4,
        PcuIrJumpCondition::XNotEqualY => 5,
        PcuIrJumpCondition::PinHigh => 6,
        PcuIrJumpCondition::OsrNotEmpty => 7,
    };
    rp2350_encode_instr_and_args(RP2350_PIO_MAJOR_JMP, arg1, target as u16)
}

#[cfg(any(test, all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")))]
const fn rp2350_encode_wait_pin(polarity: bool, pin: u8) -> u16 {
    rp2350_encode_instr_and_args(
        RP2350_PIO_MAJOR_WAIT,
        1 | if polarity { 4 } else { 0 },
        pin as u16,
    )
}

#[cfg(any(test, all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")))]
const fn rp2350_encode_irq_index(relative: bool, irq: u8) -> u16 {
    (if relative { 0x10 } else { 0 }) | irq as u16
}

#[cfg(any(test, all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")))]
const fn rp2350_encode_wait_irq(polarity: bool, relative: bool, irq: u8) -> u16 {
    rp2350_encode_instr_and_args(
        RP2350_PIO_MAJOR_WAIT,
        2 | if polarity { 4 } else { 0 },
        rp2350_encode_irq_index(relative, irq),
    )
}

#[cfg(any(test, all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")))]
const fn rp2350_encode_in(source: PcuIrInSource, count: u8) -> u16 {
    let arg1 = match source {
        PcuIrInSource::Pins => RP2350_PIO_SRC_DEST_PINS,
        PcuIrInSource::X => RP2350_PIO_SRC_DEST_X,
        PcuIrInSource::Y => RP2350_PIO_SRC_DEST_Y,
        PcuIrInSource::Null => RP2350_PIO_SRC_DEST_NULL,
        PcuIrInSource::Status => RP2350_PIO_SRC_DEST_STATUS,
        PcuIrInSource::Isr => RP2350_PIO_SRC_DEST_ISR,
        PcuIrInSource::Osr => RP2350_PIO_SRC_DEST_OSR,
    };
    rp2350_encode_instr_and_args(
        RP2350_PIO_MAJOR_IN,
        arg1,
        rp2350_encode_bit_count(count),
    )
}

#[cfg(any(test, all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")))]
const fn rp2350_encode_out(destination: PcuIrOutDestination, count: u8) -> u16 {
    let arg1 = match destination {
        PcuIrOutDestination::Pins => RP2350_PIO_SRC_DEST_PINS,
        PcuIrOutDestination::X => RP2350_PIO_SRC_DEST_X,
        PcuIrOutDestination::Y => RP2350_PIO_SRC_DEST_Y,
        PcuIrOutDestination::Null => RP2350_PIO_SRC_DEST_NULL,
        PcuIrOutDestination::PinDirs => RP2350_PIO_SRC_DEST_PINDIRS,
        PcuIrOutDestination::Pc => RP2350_PIO_SRC_DEST_PC,
        PcuIrOutDestination::Isr => RP2350_PIO_SRC_DEST_ISR,
        PcuIrOutDestination::Exec => RP2350_PIO_SRC_DEST_OSR,
    };
    rp2350_encode_instr_and_args(
        RP2350_PIO_MAJOR_OUT,
        arg1,
        rp2350_encode_bit_count(count),
    )
}

#[cfg(any(test, all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")))]
const fn rp2350_encode_push(if_full: bool, block: bool) -> u16 {
    rp2350_encode_instr_and_args(
        RP2350_PIO_MAJOR_PUSH,
        (if if_full { 2 } else { 0 }) | if block { 1 } else { 0 },
        0,
    )
}

#[cfg(any(test, all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")))]
const fn rp2350_encode_pull(if_empty: bool, block: bool) -> u16 {
    rp2350_encode_instr_and_args(
        RP2350_PIO_MAJOR_PULL,
        (if if_empty { 2 } else { 0 }) | if block { 1 } else { 0 },
        0,
    )
}

#[cfg(any(test, all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")))]
const fn rp2350_encode_mov(
    destination: PcuIrMovDestination,
    operation: PcuIrMovOperation,
    source: PcuIrMovSource,
) -> u16 {
    let dest = match destination {
        PcuIrMovDestination::Pins => RP2350_PIO_SRC_DEST_PINS,
        PcuIrMovDestination::X => RP2350_PIO_SRC_DEST_X,
        PcuIrMovDestination::Y => RP2350_PIO_SRC_DEST_Y,
        PcuIrMovDestination::Exec => RP2350_PIO_SRC_DEST_EXEC,
        PcuIrMovDestination::Pc => RP2350_PIO_SRC_DEST_PC,
        PcuIrMovDestination::Isr => RP2350_PIO_SRC_DEST_ISR,
        PcuIrMovDestination::Osr => RP2350_PIO_SRC_DEST_OSR,
    };
    let src = match source {
        PcuIrMovSource::Pins => RP2350_PIO_SRC_DEST_PINS,
        PcuIrMovSource::X => RP2350_PIO_SRC_DEST_X,
        PcuIrMovSource::Y => RP2350_PIO_SRC_DEST_Y,
        PcuIrMovSource::Null => RP2350_PIO_SRC_DEST_NULL,
        PcuIrMovSource::Status => RP2350_PIO_SRC_DEST_STATUS,
        PcuIrMovSource::Isr => RP2350_PIO_SRC_DEST_ISR,
        PcuIrMovSource::Osr => RP2350_PIO_SRC_DEST_OSR,
    };
    let op = match operation {
        PcuIrMovOperation::None => 0,
        PcuIrMovOperation::Invert => 1 << 3,
        PcuIrMovOperation::Reverse => 2 << 3,
    };
    rp2350_encode_instr_and_args(RP2350_PIO_MAJOR_MOV, dest, op | src)
}

#[cfg(any(test, all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")))]
const fn rp2350_encode_irq(action: PcuIrIrqAction, relative: bool, index: u8) -> u16 {
    let arg1 = match action {
        PcuIrIrqAction::Set => 0,
        PcuIrIrqAction::Wait => 1,
        PcuIrIrqAction::Clear => 2,
    };
    rp2350_encode_instr_and_args(
        RP2350_PIO_MAJOR_IRQ,
        arg1,
        rp2350_encode_irq_index(relative, index),
    )
}

#[cfg(any(test, all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")))]
const fn rp2350_encode_set(destination: PcuIrSetDestination, value: u8) -> u16 {
    let arg1 = match destination {
        PcuIrSetDestination::Pins => RP2350_PIO_SRC_DEST_PINS,
        PcuIrSetDestination::X => RP2350_PIO_SRC_DEST_X,
        PcuIrSetDestination::Y => RP2350_PIO_SRC_DEST_Y,
        PcuIrSetDestination::PinDirs => RP2350_PIO_SRC_DEST_PINDIRS,
    };
    rp2350_encode_instr_and_args(RP2350_PIO_MAJOR_SET, arg1, value as u16)
}

#[cfg(any(test, all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")))]
const fn rp2350_encode_nop() -> u16 {
    rp2350_encode_instr_and_args(
        RP2350_PIO_MAJOR_MOV,
        RP2350_PIO_SRC_DEST_Y,
        RP2350_PIO_SRC_DEST_Y,
    )
}

#[cfg(any(test, all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")))]
const fn rp2350_encode_delay(cycles: u8) -> u16 {
    ((cycles as u16) - 1) << 8
}

#[cfg(any(test, all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")))]
const fn rp2350_encode_bit_count(count: u8) -> u16 {
    if count == 32 { 0 } else { count as u16 }
}

#[cfg(any(test, all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")))]
const fn rp2350_execution_is_default(execution: &PcuIrExecutionConfig) -> bool {
    execution.clocking.divider_integer.is_none()
        && execution.clocking.divider_fractional.is_none()
        && execution.pins.input_base.is_none()
        && execution.pins.output_base.is_none()
        && execution.pins.set_base.is_none()
        && execution.pins.set_count.is_none()
        && execution.pins.sideset_base.is_none()
        && execution.pins.sideset_count.is_none()
        && !execution.pins.sideset_optional
        && execution.pins.jmp_pin.is_none()
        && execution.shift.in_direction.is_none()
        && execution.shift.out_direction.is_none()
        && execution.shift.autopush_threshold.is_none()
        && execution.shift.autopull_threshold.is_none()
        && execution.wrap_target.is_none()
        && execution.wrap_source.is_none()
}

#[cfg(any(test, all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")))]
const fn validate_rp2350_execution(execution: &PcuIrExecutionConfig) -> Result<(), PcuError> {
    if rp2350_execution_is_default(execution) {
        Ok(())
    } else {
        Err(PcuError::unsupported())
    }
}

#[cfg(any(test, all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")))]
fn lower_rp2350_program<'a>(
    program: &PcuIrProgram<'_>,
    storage: &'a mut [u16],
) -> Result<PcuProgramImage<'a>, PcuError> {
    if program.instructions.is_empty() {
        return Err(PcuError::invalid());
    }
    validate_rp2350_execution(&program.execution)?;
    if program.instructions.len() > RP2350_PIO_INSTRUCTION_LIMIT {
        return Err(PcuError::resource_exhausted());
    }
    if storage.len() < program.instructions.len() {
        return Err(PcuError::resource_exhausted());
    }

    for (index, instruction) in program.instructions.iter().copied().enumerate() {
        storage[index] = match instruction {
            PcuIrInstruction::Nop => rp2350_encode_nop(),
            PcuIrInstruction::Delay { cycles } => {
                if cycles == 0 || cycles > 32 {
                    return Err(PcuError::invalid());
                }
                rp2350_encode_nop() | rp2350_encode_delay(cycles)
            }
            PcuIrInstruction::Wait(PcuIrWaitCondition::PinLow { pin }) => {
                if pin > 31 {
                    return Err(PcuError::invalid());
                }
                rp2350_encode_wait_pin(false, pin)
            }
            PcuIrInstruction::Wait(PcuIrWaitCondition::PinHigh { pin }) => {
                if pin > 31 {
                    return Err(PcuError::invalid());
                }
                rp2350_encode_wait_pin(true, pin)
            }
            PcuIrInstruction::Wait(PcuIrWaitCondition::Irq {
                polarity,
                relative,
                index,
            }) => {
                if index > 7 {
                    return Err(PcuError::invalid());
                }
                rp2350_encode_wait_irq(polarity, relative, index)
            }
            PcuIrInstruction::In { source, bit_count } => {
                if bit_count == 0 || bit_count > 32 {
                    return Err(PcuError::invalid());
                }
                rp2350_encode_in(source, bit_count)
            }
            PcuIrInstruction::Out {
                destination,
                bit_count,
            } => {
                if bit_count == 0 || bit_count > 32 {
                    return Err(PcuError::invalid());
                }
                rp2350_encode_out(destination, bit_count)
            }
            PcuIrInstruction::Push { if_full, blocking } => {
                rp2350_encode_push(if_full, blocking)
            }
            PcuIrInstruction::Pull { if_empty, blocking } => {
                rp2350_encode_pull(if_empty, blocking)
            }
            PcuIrInstruction::Mov {
                destination,
                operation,
                source,
            } => rp2350_encode_mov(destination, operation, source),
            PcuIrInstruction::Irq {
                action,
                relative,
                index,
            } => {
                if index > 7 {
                    return Err(PcuError::invalid());
                }
                rp2350_encode_irq(action, relative, index)
            }
            PcuIrInstruction::Set { destination, value } => {
                if value > 31 {
                    return Err(PcuError::invalid());
                }
                rp2350_encode_set(destination, value)
            }
            PcuIrInstruction::Jump { condition, target } => {
                if usize::from(target) >= program.instructions.len() {
                    return Err(PcuError::invalid());
                }
                rp2350_encode_jmp_condition(condition, target)
            }
        };
    }

    Ok(PcuProgramImage {
        id: program.id,
        words: &storage[..program.instructions.len()],
    })
}

/// fusion-sys programmable-IO wrapper around the selected backend.
#[derive(Debug, Clone, Copy)]
pub struct PcuSystem {
    inner: PlatformPcu,
}

impl PcuSystem {
    /// Creates a wrapper for the selected programmable-IO backend.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            inner: pal_system_pcu(),
        }
    }

    /// Reports the truthful programmable-IO surface for the selected backend.
    #[must_use]
    pub fn support(&self) -> PcuSupport {
        PcuBase::support(&self.inner)
    }

    /// Returns the surfaced engine descriptors.
    #[must_use]
    pub fn engines(&self) -> &'static [PcuEngineDescriptor] {
        PcuBase::engines(&self.inner)
    }

    /// Returns the surfaced lane descriptors for one engine.
    #[must_use]
    pub fn lanes(&self, engine: PcuEngineId) -> &'static [PcuLaneDescriptor] {
        PcuBase::lanes(&self.inner, engine)
    }

    /// Claims one engine exclusively.
    ///
    /// # Errors
    ///
    /// Returns any honest backend claim failure.
    pub fn claim_engine(&self, engine: PcuEngineId) -> Result<PcuEngineClaim, PcuError> {
        PcuControl::claim_engine(&self.inner, engine)
    }

    /// Releases one previously claimed engine.
    ///
    /// # Errors
    ///
    /// Returns any honest backend release failure.
    pub fn release_engine(&self, claim: PcuEngineClaim) -> Result<(), PcuError> {
        PcuControl::release_engine(&self.inner, claim)
    }

    /// Claims one or more lanes within one engine.
    ///
    /// # Errors
    ///
    /// Returns any honest backend claim failure.
    pub fn claim_lanes(
        &self,
        engine: PcuEngineId,
        lanes: PcuLaneMask,
    ) -> Result<PcuLaneClaim, PcuError> {
        PcuControl::claim_lanes(&self.inner, engine, lanes)
    }

    /// Releases one previously claimed lane mask.
    ///
    /// # Errors
    ///
    /// Returns any honest backend release failure.
    pub fn release_lanes(&self, claim: PcuLaneClaim) -> Result<(), PcuError> {
        PcuControl::release_lanes(&self.inner, claim)
    }

    /// Loads one backend-native program image into one claimed engine.
    ///
    /// # Errors
    ///
    /// Returns any honest backend load failure.
    pub fn load_program(
        &self,
        claim: &PcuEngineClaim,
        image: &PcuProgramImage<'_>,
    ) -> Result<PcuProgramLease, PcuError> {
        PcuControl::load_program(&self.inner, claim, image)
    }

    /// Lowers one portable deterministic IO kernel into backend-native words.
    ///
    /// # Errors
    ///
    /// Returns `unsupported` when the selected backend does not yet implement portable lowering
    /// for the supplied kernel.
    #[cfg(all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350"))]
    pub fn lower_program<'a>(
        &self,
        program: &PcuIrProgram<'_>,
        storage: &'a mut [u16],
    ) -> Result<PcuProgramImage<'a>, PcuError> {
        lower_rp2350_program(program, storage)
    }

    /// Lowers one portable deterministic IO kernel into backend-native words.
    ///
    /// # Errors
    ///
    /// Returns `unsupported` when the selected backend does not yet implement portable lowering
    /// for the supplied kernel.
    #[cfg(not(all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")))]
    pub const fn lower_program<'a>(
        &self,
        program: &PcuIrProgram<'_>,
        storage: &'a mut [u16],
    ) -> Result<PcuProgramImage<'a>, PcuError> {
        let _ = storage;
        let _ = program;
        Err(PcuError::unsupported())
    }

    /// Loads one native or portable program source into one claimed engine.
    ///
    /// # Errors
    ///
    /// Returns any honest backend load or lowering failure.
    pub fn load_program_source(
        &self,
        claim: &PcuEngineClaim,
        source: PcuProgramSource<'_>,
        lowering_storage: &mut [u16],
    ) -> Result<PcuProgramLease, PcuError> {
        match source {
            PcuProgramSource::Native(image) => self.load_program(claim, image),
            PcuProgramSource::Ir(program) => {
                let image = self.lower_program(program, lowering_storage)?;
                self.load_program(claim, &image)
            }
        }
    }

    /// Unloads one previously loaded program image.
    ///
    /// # Errors
    ///
    /// Returns any honest backend unload failure.
    pub fn unload_program(
        &self,
        claim: &PcuEngineClaim,
        lease: PcuProgramLease,
    ) -> Result<(), PcuError> {
        PcuControl::unload_program(&self.inner, claim, lease)
    }

    /// Starts one claimed lane set.
    ///
    /// # Errors
    ///
    /// Returns any honest backend control failure.
    pub fn start_lanes(&self, claim: &PcuLaneClaim) -> Result<(), PcuError> {
        PcuControl::start_lanes(&self.inner, claim)
    }

    /// Stops one claimed lane set.
    ///
    /// # Errors
    ///
    /// Returns any honest backend control failure.
    pub fn stop_lanes(&self, claim: &PcuLaneClaim) -> Result<(), PcuError> {
        PcuControl::stop_lanes(&self.inner, claim)
    }

    /// Restarts one claimed lane set.
    ///
    /// # Errors
    ///
    /// Returns any honest backend control failure.
    pub fn restart_lanes(&self, claim: &PcuLaneClaim) -> Result<(), PcuError> {
        PcuControl::restart_lanes(&self.inner, claim)
    }

    /// Writes one word into one claimed TX FIFO.
    ///
    /// # Errors
    ///
    /// Returns any honest backend FIFO failure.
    pub fn write_tx_fifo(
        &self,
        claim: &PcuLaneClaim,
        lane: PcuLaneId,
        word: u32,
    ) -> Result<(), PcuError> {
        PcuControl::write_tx_fifo(&self.inner, claim, lane, word)
    }

    /// Reads one word from one claimed RX FIFO.
    ///
    /// # Errors
    ///
    /// Returns any honest backend FIFO failure.
    pub fn read_rx_fifo(&self, claim: &PcuLaneClaim, lane: PcuLaneId) -> Result<u32, PcuError> {
        PcuControl::read_rx_fifo(&self.inner, claim, lane)
    }
}

impl Default for PcuSystem {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rp2350_lowering_encodes_supported_subset() {
        let program = PcuIrProgram::new(
            super::super::PcuProgramId(7),
            &[
                PcuIrInstruction::Pull {
                    if_empty: false,
                    blocking: true,
                },
                PcuIrInstruction::Out {
                    destination: PcuIrOutDestination::Pins,
                    bit_count: 8,
                },
                PcuIrInstruction::Set {
                    destination: PcuIrSetDestination::Pins,
                    value: 3,
                },
                PcuIrInstruction::Wait(PcuIrWaitCondition::PinHigh { pin: 2 }),
                PcuIrInstruction::Irq {
                    action: PcuIrIrqAction::Set,
                    relative: false,
                    index: 1,
                },
                PcuIrInstruction::Jump {
                    condition: PcuIrJumpCondition::Always,
                    target: 0,
                },
            ],
        );
        let mut storage = [0_u16; 8];
        let image = lower_rp2350_program(&program, &mut storage).expect("program should lower");

        assert_eq!(image.id, program.id);
        assert_eq!(
            image.words,
            &[
                rp2350_encode_pull(false, true),
                rp2350_encode_out(PcuIrOutDestination::Pins, 8),
                rp2350_encode_set(PcuIrSetDestination::Pins, 3),
                rp2350_encode_wait_pin(true, 2),
                rp2350_encode_irq(PcuIrIrqAction::Set, false, 1),
                rp2350_encode_jmp_condition(PcuIrJumpCondition::Always, 0),
            ]
        );
    }

    #[test]
    fn rp2350_lowering_rejects_non_default_execution_config() {
        let program = PcuIrProgram::new(super::super::PcuProgramId(1), &[PcuIrInstruction::Nop])
            .with_execution(PcuIrExecutionConfig {
                wrap_target: Some(0),
                wrap_source: Some(0),
                ..PcuIrExecutionConfig::default()
            });
        let mut storage = [0_u16; 1];
        let error = lower_rp2350_program(&program, &mut storage)
            .expect_err("non-default execution config should stay unsupported without control wiring");
        assert_eq!(error.kind(), PcuError::unsupported().kind());
    }

    #[test]
    fn rp2350_lowering_maps_exact_delay_cycles() {
        let program = PcuIrProgram::new(
            super::super::PcuProgramId(2),
            &[PcuIrInstruction::Delay { cycles: 1 }, PcuIrInstruction::Delay { cycles: 32 }],
        );
        let mut storage = [0_u16; 2];
        let image = lower_rp2350_program(&program, &mut storage).expect("delays should lower");

        assert_eq!(image.words[0], rp2350_encode_nop());
        assert_eq!(image.words[1], rp2350_encode_nop() | (31 << 8));
    }

    #[test]
    fn rp2350_lowering_encodes_mov_and_conditional_jump_variants() {
        let program = PcuIrProgram::new(
            super::super::PcuProgramId(9),
            &[
                PcuIrInstruction::Mov {
                    destination: PcuIrMovDestination::X,
                    operation: PcuIrMovOperation::Invert,
                    source: PcuIrMovSource::Status,
                },
                PcuIrInstruction::Jump {
                    condition: PcuIrJumpCondition::PinHigh,
                    target: 1,
                },
            ],
        );
        let mut storage = [0_u16; 2];
        let image = lower_rp2350_program(&program, &mut storage).expect("program should lower");

        assert_eq!(
            image.words,
            &[
                rp2350_encode_mov(
                    PcuIrMovDestination::X,
                    PcuIrMovOperation::Invert,
                    PcuIrMovSource::Status,
                ),
                rp2350_encode_jmp_condition(PcuIrJumpCondition::PinHigh, 1),
            ]
        );
    }
}
