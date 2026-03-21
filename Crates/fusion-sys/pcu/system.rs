//! fusion-sys programmable-IO wrapper over the selected fusion-pal backend.

use crate::event::EventSourceHandle;
use fusion_pal::sys::pcu::{PlatformPcu, system_pcu as pal_system_pcu};

use super::{
    PcuBase,
    PcuControl,
    PcuDmaAttachment,
    PcuEngineClaim,
    PcuEngineDescriptor,
    PcuEngineId,
    PcuError,
    PcuEventAttachment,
    PcuIrExecutionConfig,
    PcuIrInstruction,
    PcuIrProgram,
    PcuLaneClaim,
    PcuLaneDescriptor,
    PcuLaneId,
    PcuLaneMask,
    PcuPipelineHandoff,
    PcuPipelineStage,
    PcuPipelineStageLease,
    PcuProgramImage,
    PcuProgramLease,
    PcuProgramSource,
    PcuSupport,
};
#[cfg(any(
    test,
    all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")
))]
use super::{
    PcuIrInSource,
    PcuIrInstructionTiming,
    PcuIrIrqAction,
    PcuIrJumpCondition,
    PcuIrMovDestination,
    PcuIrMovOperation,
    PcuIrMovSource,
    PcuIrOutDestination,
    PcuIrSetDestination,
    PcuIrWaitCondition,
};

#[cfg(any(
    test,
    all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")
))]
const RP2350_PIO_INSTRUCTION_LIMIT: usize = 32;
#[cfg(any(
    test,
    all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")
))]
const RP2350_PIO_MAJOR_JMP: u16 = 0x0000;
#[cfg(any(
    test,
    all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")
))]
const RP2350_PIO_MAJOR_WAIT: u16 = 0x2000;
#[cfg(any(
    test,
    all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")
))]
const RP2350_PIO_MAJOR_IN: u16 = 0x4000;
#[cfg(any(
    test,
    all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")
))]
const RP2350_PIO_MAJOR_OUT: u16 = 0x6000;
#[cfg(any(
    test,
    all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")
))]
const RP2350_PIO_MAJOR_PUSH: u16 = 0x8000;
#[cfg(any(
    test,
    all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")
))]
const RP2350_PIO_MAJOR_PULL: u16 = 0x8080;
#[cfg(any(
    test,
    all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")
))]
const RP2350_PIO_MAJOR_MOV: u16 = 0xa000;
#[cfg(any(
    test,
    all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")
))]
const RP2350_PIO_MAJOR_IRQ: u16 = 0xc000;
#[cfg(any(
    test,
    all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")
))]
const RP2350_PIO_MAJOR_SET: u16 = 0xe000;
#[cfg(any(
    test,
    all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")
))]
const RP2350_PIO_SM_CLKDIV_RESET: u32 = 0x0001_0000;
#[cfg(any(
    test,
    all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")
))]
const RP2350_PIO_SM_EXECCTRL_RESET: u32 = 0x0001_f000;
#[cfg(any(
    test,
    all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")
))]
const RP2350_PIO_SM_SHIFTCTRL_RESET: u32 = 0x000c_0000;
#[cfg(any(
    test,
    all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")
))]
const RP2350_PIO_SM_PINCTRL_RESET: u32 = 0x1400_0000;
#[cfg(any(
    test,
    all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")
))]
const RP2350_PIO_SRC_DEST_PINS: u16 = 0;
#[cfg(any(
    test,
    all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")
))]
const RP2350_PIO_SRC_DEST_X: u16 = 1;
#[cfg(any(
    test,
    all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")
))]
const RP2350_PIO_SRC_DEST_Y: u16 = 2;
#[cfg(any(
    test,
    all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")
))]
const RP2350_PIO_SRC_DEST_NULL: u16 = 3;
#[cfg(any(
    test,
    all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")
))]
const RP2350_PIO_SRC_DEST_PINDIRS: u16 = 4;
#[cfg(any(
    test,
    all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")
))]
const RP2350_PIO_SRC_DEST_EXEC: u16 = 4;
#[cfg(any(
    test,
    all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")
))]
const RP2350_PIO_SRC_DEST_STATUS: u16 = 5;
#[cfg(any(
    test,
    all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")
))]
const RP2350_PIO_SRC_DEST_PC: u16 = 5;
#[cfg(any(
    test,
    all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")
))]
const RP2350_PIO_SRC_DEST_ISR: u16 = 6;
#[cfg(any(
    test,
    all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")
))]
const RP2350_PIO_SRC_DEST_OSR: u16 = 7;

#[cfg(any(
    test,
    all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")
))]
const fn rp2350_encode_instr_and_args(instr_bits: u16, arg1: u16, arg2: u16) -> u16 {
    instr_bits | (arg1 << 5) | (arg2 & 0x1f)
}

#[cfg(any(
    test,
    all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")
))]
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

#[cfg(any(
    test,
    all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")
))]
const fn rp2350_encode_wait_pin(polarity: bool, pin: u8) -> u16 {
    rp2350_encode_instr_and_args(
        RP2350_PIO_MAJOR_WAIT,
        1 | if polarity { 4 } else { 0 },
        pin as u16,
    )
}

#[cfg(any(
    test,
    all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")
))]
const fn rp2350_encode_irq_index(relative: bool, irq: u8) -> u16 {
    (if relative { 0x10 } else { 0 }) | irq as u16
}

#[cfg(any(
    test,
    all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")
))]
const fn rp2350_encode_wait_irq(polarity: bool, relative: bool, irq: u8) -> u16 {
    rp2350_encode_instr_and_args(
        RP2350_PIO_MAJOR_WAIT,
        2 | if polarity { 4 } else { 0 },
        rp2350_encode_irq_index(relative, irq),
    )
}

#[cfg(any(
    test,
    all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")
))]
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
    rp2350_encode_instr_and_args(RP2350_PIO_MAJOR_IN, arg1, rp2350_encode_bit_count(count))
}

#[cfg(any(
    test,
    all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")
))]
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
    rp2350_encode_instr_and_args(RP2350_PIO_MAJOR_OUT, arg1, rp2350_encode_bit_count(count))
}

#[cfg(any(
    test,
    all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")
))]
const fn rp2350_encode_push(if_full: bool, block: bool) -> u16 {
    rp2350_encode_instr_and_args(
        RP2350_PIO_MAJOR_PUSH,
        (if if_full { 2 } else { 0 }) | if block { 1 } else { 0 },
        0,
    )
}

#[cfg(any(
    test,
    all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")
))]
const fn rp2350_encode_pull(if_empty: bool, block: bool) -> u16 {
    rp2350_encode_instr_and_args(
        RP2350_PIO_MAJOR_PULL,
        (if if_empty { 2 } else { 0 }) | if block { 1 } else { 0 },
        0,
    )
}

#[cfg(any(
    test,
    all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")
))]
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

#[cfg(any(
    test,
    all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")
))]
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

#[cfg(any(
    test,
    all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")
))]
const fn rp2350_encode_set(destination: PcuIrSetDestination, value: u8) -> u16 {
    let arg1 = match destination {
        PcuIrSetDestination::Pins => RP2350_PIO_SRC_DEST_PINS,
        PcuIrSetDestination::X => RP2350_PIO_SRC_DEST_X,
        PcuIrSetDestination::Y => RP2350_PIO_SRC_DEST_Y,
        PcuIrSetDestination::PinDirs => RP2350_PIO_SRC_DEST_PINDIRS,
    };
    rp2350_encode_instr_and_args(RP2350_PIO_MAJOR_SET, arg1, value as u16)
}

#[cfg(any(
    test,
    all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")
))]
const fn rp2350_encode_nop() -> u16 {
    rp2350_encode_instr_and_args(
        RP2350_PIO_MAJOR_MOV,
        RP2350_PIO_SRC_DEST_Y,
        RP2350_PIO_SRC_DEST_Y,
    )
}

#[cfg(any(
    test,
    all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")
))]
fn rp2350_timing_field_bits(execution: &PcuIrExecutionConfig) -> u8 {
    execution.pins.sideset_count.unwrap_or(0) + u8::from(execution.pins.sideset_optional)
}

#[cfg(any(
    test,
    all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")
))]
fn rp2350_encode_instruction_timing(
    execution: &PcuIrExecutionConfig,
    instruction: PcuIrInstruction,
    timing: PcuIrInstructionTiming,
) -> Result<u16, PcuError> {
    let timing_bits = rp2350_timing_field_bits(execution);
    if timing_bits > 5 {
        return Err(PcuError::invalid());
    }

    let available_delay_bits = 5 - timing_bits;
    let max_stall_cycles = if available_delay_bits == 0 {
        0
    } else {
        (1_u8 << available_delay_bits) - 1
    };

    let implied_stall_cycles = match instruction {
        PcuIrInstruction::Delay { cycles } => {
            if cycles == 0 || cycles > 32 {
                return Err(PcuError::invalid());
            }
            cycles - 1
        }
        _ => 0,
    };
    let total_stall_cycles = implied_stall_cycles
        .checked_add(timing.stall_cycles)
        .ok_or_else(PcuError::resource_exhausted)?;
    if total_stall_cycles > max_stall_cycles {
        return Err(PcuError::invalid());
    }

    let mut encoded = u16::from(total_stall_cycles) << 8;
    match (
        execution.pins.sideset_optional,
        execution.pins.sideset_count,
        timing.sideset_bits,
    ) {
        (false, Some(bit_count), Some(value)) => {
            if bit_count == 0 || bit_count > 5 || value >= (1_u8 << bit_count) {
                return Err(PcuError::invalid());
            }
            encoded |= u16::from(value) << (13 - bit_count);
        }
        (false | true, Some(_) | None, None) => {}
        (false | true, None, Some(_)) => return Err(PcuError::invalid()),
        (true, Some(bit_count), Some(value)) => {
            if bit_count > 4 || value >= (1_u8 << bit_count) {
                return Err(PcuError::invalid());
            }
            encoded |= 0x1000 | (u16::from(value) << (12 - bit_count));
        }
    }

    Ok(encoded)
}

#[cfg(any(
    test,
    all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")
))]
const fn rp2350_encode_bit_count(count: u8) -> u16 {
    if count == 32 { 0 } else { count as u16 }
}

const fn rp2350_execution_is_default(execution: &PcuIrExecutionConfig) -> bool {
    execution.clocking.divider_integer.is_none()
        && execution.clocking.divider_fractional.is_none()
        && execution.pins.input_base.is_none()
        && execution.pins.input_count.is_none()
        && execution.pins.output_base.is_none()
        && execution.pins.output_count.is_none()
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

#[cfg(any(
    test,
    all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")
))]
const fn rp2350_pinctrl_count(value: u8, maximum: u8) -> Result<u32, PcuError> {
    if value <= maximum {
        Ok(if value == 32 { 0 } else { value as u32 })
    } else {
        Err(PcuError::invalid())
    }
}

#[cfg(any(
    test,
    all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")
))]
fn rp2350_effective_output_count(
    execution: &PcuIrExecutionConfig,
    instructions: Option<&[PcuIrInstruction]>,
) -> Result<u8, PcuError> {
    if let Some(explicit) = execution.pins.output_count {
        if explicit <= 32 {
            return Ok(explicit);
        }
        return Err(PcuError::invalid());
    }

    let mut count = 0u8;
    if let Some(instructions) = instructions {
        for instruction in instructions {
            match *instruction {
                PcuIrInstruction::Out {
                    destination: PcuIrOutDestination::Pins | PcuIrOutDestination::PinDirs,
                    bit_count,
                } => {
                    count = count.max(bit_count);
                }
                PcuIrInstruction::Mov {
                    destination: PcuIrMovDestination::Pins,
                    ..
                } => {
                    count = 32;
                }
                _ => {}
            }
        }
    }

    Ok(count)
}

#[cfg(any(
    test,
    all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")
))]
fn rp2350_effective_set_count(
    execution: &PcuIrExecutionConfig,
    instructions: Option<&[PcuIrInstruction]>,
) -> Result<u8, PcuError> {
    if let Some(explicit) = execution.pins.set_count {
        if explicit <= 5 {
            return Ok(explicit);
        }
        return Err(PcuError::invalid());
    }

    let mut uses_set_pins = false;
    if let Some(instructions) = instructions {
        for instruction in instructions {
            if matches!(
                *instruction,
                PcuIrInstruction::Set {
                    destination: PcuIrSetDestination::Pins | PcuIrSetDestination::PinDirs,
                    ..
                }
            ) {
                uses_set_pins = true;
                break;
            }
        }
    }

    Ok(if uses_set_pins { 5 } else { 0 })
}

#[cfg(any(
    test,
    all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")
))]
fn rp2350_build_execution_registers(
    execution: &PcuIrExecutionConfig,
    instructions: Option<&[PcuIrInstruction]>,
) -> Result<(u32, u32, u32, u32), PcuError> {
    let divider_integer = execution.clocking.divider_integer.unwrap_or(1);
    let divider_fractional = execution.clocking.divider_fractional.unwrap_or(0);
    if divider_integer == 0 && divider_fractional != 0 {
        return Err(PcuError::invalid());
    }

    let input_base = execution.pins.input_base.unwrap_or(0);
    let input_count = execution.pins.input_count.unwrap_or(32);
    let output_base = execution.pins.output_base.unwrap_or(0);
    let output_count = rp2350_effective_output_count(execution, instructions)?;
    let set_base = execution.pins.set_base.unwrap_or(0);
    let set_count = rp2350_effective_set_count(execution, instructions)?;
    let sideset_base = execution.pins.sideset_base.unwrap_or(0);
    let sideset_count = execution.pins.sideset_count.unwrap_or(0);
    let sideset_field_count = sideset_count
        .checked_add(u8::from(execution.pins.sideset_optional))
        .ok_or_else(PcuError::invalid)?;
    let jmp_pin = execution.pins.jmp_pin.unwrap_or(0);
    let wrap_target = execution.wrap_target.unwrap_or(0);
    let wrap_source = execution.wrap_source.unwrap_or(31);
    let autopush_threshold = execution.shift.autopush_threshold.unwrap_or(0);
    let autopull_threshold = execution.shift.autopull_threshold.unwrap_or(0);

    if input_base > 31
        || output_base > 31
        || set_base > 31
        || sideset_base > 31
        || jmp_pin > 31
        || wrap_target > 31
        || wrap_source > 31
        || input_count == 0
        || input_count > 32
        || sideset_field_count > 5
        || autopush_threshold > 32
        || autopull_threshold > 32
    {
        return Err(PcuError::invalid());
    }

    let mut clkdiv = RP2350_PIO_SM_CLKDIV_RESET;
    clkdiv &= !(0xffff_u32 << 16);
    clkdiv &= !(0xff_u32 << 8);
    clkdiv |= u32::from(divider_integer) << 16;
    clkdiv |= u32::from(divider_fractional) << 8;

    let mut execctrl = RP2350_PIO_SM_EXECCTRL_RESET;
    execctrl &= !(1_u32 << 30);
    execctrl &= !(0x1f_u32 << 24);
    execctrl &= !(0x1f_u32 << 12);
    execctrl &= !(0x1f_u32 << 7);
    execctrl |= u32::from(execution.pins.sideset_optional) << 30;
    execctrl |= u32::from(jmp_pin) << 24;
    execctrl |= u32::from(wrap_source) << 12;
    execctrl |= u32::from(wrap_target) << 7;

    let mut shiftctrl = RP2350_PIO_SM_SHIFTCTRL_RESET;
    shiftctrl &= !(0x1f_u32 << 25);
    shiftctrl &= !(0x1f_u32 << 20);
    shiftctrl &= !(1_u32 << 19);
    shiftctrl &= !(1_u32 << 18);
    shiftctrl &= !(1_u32 << 17);
    shiftctrl &= !(1_u32 << 16);
    shiftctrl &= !0x1f_u32;
    shiftctrl |= rp2350_pinctrl_count(autopull_threshold, 32)? << 25;
    shiftctrl |= rp2350_pinctrl_count(autopush_threshold, 32)? << 20;
    shiftctrl |= u32::from(
        execution.shift.out_direction.unwrap_or_default() == super::PcuIrShiftDirection::Right,
    ) << 19;
    shiftctrl |= u32::from(
        execution.shift.in_direction.unwrap_or_default() == super::PcuIrShiftDirection::Right,
    ) << 18;
    shiftctrl |= u32::from(execution.shift.autopull_threshold.is_some()) << 17;
    shiftctrl |= u32::from(execution.shift.autopush_threshold.is_some()) << 16;
    shiftctrl |= rp2350_pinctrl_count(input_count, 32)?;

    let mut pinctrl = RP2350_PIO_SM_PINCTRL_RESET;
    pinctrl &= !(0x7_u32 << 29);
    pinctrl &= !(0x7_u32 << 26);
    pinctrl &= !(0x3f_u32 << 20);
    pinctrl &= !(0x1f_u32 << 15);
    pinctrl &= !(0x1f_u32 << 10);
    pinctrl &= !(0x1f_u32 << 5);
    pinctrl &= !0x1f_u32;
    pinctrl |= u32::from(sideset_field_count) << 29;
    pinctrl |= rp2350_pinctrl_count(set_count, 5)? << 26;
    pinctrl |= rp2350_pinctrl_count(output_count, 32)? << 20;
    pinctrl |= u32::from(input_base) << 15;
    pinctrl |= u32::from(sideset_base) << 10;
    pinctrl |= u32::from(set_base) << 5;
    pinctrl |= u32::from(output_base);

    Ok((clkdiv, execctrl, shiftctrl, pinctrl))
}

#[cfg(any(
    test,
    all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")
))]
fn lower_rp2350_program<'a>(
    program: &PcuIrProgram<'_>,
    storage: &'a mut [u16],
) -> Result<PcuProgramImage<'a>, PcuError> {
    if program.instructions.is_empty() {
        return Err(PcuError::invalid());
    }
    if program.instructions.len() > RP2350_PIO_INSTRUCTION_LIMIT {
        return Err(PcuError::resource_exhausted());
    }
    if storage.len() < program.instructions.len() {
        return Err(PcuError::resource_exhausted());
    }
    if let Some(timing) = program.timing
        && timing.len() != program.instructions.len()
    {
        return Err(PcuError::invalid());
    }

    for (index, instruction) in program.instructions.iter().copied().enumerate() {
        let base = match instruction {
            PcuIrInstruction::Nop | PcuIrInstruction::Delay { .. } => rp2350_encode_nop(),
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
            PcuIrInstruction::Push { if_full, blocking } => rp2350_encode_push(if_full, blocking),
            PcuIrInstruction::Pull { if_empty, blocking } => rp2350_encode_pull(if_empty, blocking),
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
        let timing = program
            .timing
            .and_then(|timing| timing.get(index).copied())
            .unwrap_or_default();
        storage[index] =
            base | rp2350_encode_instruction_timing(&program.execution, instruction, timing)?;
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

    fn engine_descriptor(
        self,
        engine: PcuEngineId,
    ) -> Result<&'static PcuEngineDescriptor, PcuError> {
        self.engines()
            .iter()
            .find(|descriptor| descriptor.id == engine)
            .ok_or_else(PcuError::invalid)
    }

    fn resolved_execution_for_source(
        source: PcuProgramSource<'_>,
        execution_override: Option<PcuIrExecutionConfig>,
    ) -> Option<PcuIrExecutionConfig> {
        execution_override.or(match source {
            PcuProgramSource::Native(_) => None,
            PcuProgramSource::Ir(program) => Some(program.execution),
        })
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

    /// Returns one DMA pacing attachment for the TX FIFO of the supplied lane.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the engine is unknown or does not expose per-lane TX DREQ
    /// selectors.
    pub fn tx_dma_attachment(&self, lane: PcuLaneId) -> Result<PcuDmaAttachment, PcuError> {
        let engine = self.engine_descriptor(lane.engine)?;
        if lane.index >= engine.lane_count {
            return Err(PcuError::invalid());
        }
        let base = engine.tx_dreq_base.ok_or_else(PcuError::unsupported)?;
        Ok(PcuDmaAttachment::tx_for_lane(
            lane,
            base + u16::from(lane.index),
        ))
    }

    /// Returns one DMA pacing attachment for the RX FIFO of the supplied lane.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the engine is unknown or does not expose per-lane RX DREQ
    /// selectors.
    pub fn rx_dma_attachment(&self, lane: PcuLaneId) -> Result<PcuDmaAttachment, PcuError> {
        let engine = self.engine_descriptor(lane.engine)?;
        if lane.index >= engine.lane_count {
            return Err(PcuError::invalid());
        }
        let base = engine.rx_dreq_base.ok_or_else(PcuError::unsupported)?;
        Ok(PcuDmaAttachment::rx_for_lane(
            lane,
            base + u16::from(lane.index),
        ))
    }

    /// Returns one event attachment for one engine-local IRQ output.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the engine is unknown or the requested IRQ output does not
    /// exist.
    pub fn engine_event_attachment(
        &self,
        engine: PcuEngineId,
        line_index: usize,
    ) -> Result<PcuEventAttachment, PcuError> {
        let descriptor = self.engine_descriptor(engine)?;
        let irqn = *descriptor
            .irq_lines
            .get(line_index)
            .ok_or_else(PcuError::invalid)?;
        Ok(PcuEventAttachment {
            engine,
            irqn,
            source: EventSourceHandle(usize::from(irqn)),
        })
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
        self.load_program_source_with_execution(claim, None, source, None, lowering_storage)
    }

    /// Loads one native or portable program source into one claimed engine and applies any
    /// supplied execution-state configuration.
    ///
    /// # Errors
    ///
    /// Returns any honest backend load, lowering, or execution-configuration failure.
    pub fn load_program_source_with_execution(
        &self,
        claim: &PcuEngineClaim,
        lane_claim: Option<&PcuLaneClaim>,
        source: PcuProgramSource<'_>,
        execution_override: Option<PcuIrExecutionConfig>,
        lowering_storage: &mut [u16],
    ) -> Result<PcuProgramLease, PcuError> {
        match source {
            PcuProgramSource::Native(image) => {
                let lease = self.load_program(claim, image)?;
                if let (Some(lane_claim), Some(execution)) = (
                    lane_claim,
                    Self::resolved_execution_for_source(
                        PcuProgramSource::Native(image),
                        execution_override,
                    ),
                ) {
                    Self::apply_execution_config_for_program(*lane_claim, &execution, None)?;
                }
                Ok(lease)
            }
            PcuProgramSource::Ir(program) => {
                let image = self.lower_program(program, lowering_storage)?;
                let lease = self.load_program(claim, &image)?;
                if let (Some(lane_claim), Some(execution)) = (
                    lane_claim,
                    Self::resolved_execution_for_source(
                        PcuProgramSource::Ir(program),
                        execution_override,
                    ),
                ) {
                    Self::apply_execution_config_for_program(
                        *lane_claim,
                        &execution,
                        Some(program.instructions),
                    )?;
                }
                Ok(lease)
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

    /// Applies one execution-state bundle to a claimed lane set.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the selected backend cannot realize the requested execution
    /// state.
    pub fn configure_execution(
        &self,
        claim: &PcuLaneClaim,
        execution: &PcuIrExecutionConfig,
    ) -> Result<(), PcuError> {
        Self::apply_execution_config_for_program(*claim, execution, None)
    }

    #[allow(clippy::missing_const_for_fn)]
    fn apply_execution_config_for_program(
        claim: PcuLaneClaim,
        execution: &PcuIrExecutionConfig,
        instructions: Option<&[PcuIrInstruction]>,
    ) -> Result<(), PcuError> {
        if rp2350_execution_is_default(execution) {
            return Ok(());
        }
        #[cfg(all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350"))]
        {
            let (clkdiv, execctrl, shiftctrl, pinctrl) =
                rp2350_build_execution_registers(execution, instructions)?;
            return fusion_pal::sys::cortex_m::hal::soc::board::apply_pcu_execution_config(
                &claim, clkdiv, execctrl, shiftctrl, pinctrl,
            );
        }
        #[cfg(not(all(target_os = "none", feature = "sys-cortex-m", feature = "soc-rp2350")))]
        {
            let _ = claim;
            let _ = execution;
            let _ = instructions;
            Err(PcuError::unsupported())
        }
    }

    fn claim_pipeline_resources(
        self,
        stage: &PcuPipelineStage<'_>,
    ) -> Result<(PcuEngineClaim, PcuLaneClaim), PcuError> {
        let requested_lanes = u8::try_from(stage.lanes.bits().count_ones()).unwrap_or(u8::MAX);
        if let Some(engine) = stage.engine {
            let engine_claim = self.claim_engine(engine)?;
            match self.claim_lanes(engine, stage.lanes) {
                Ok(lane_claim) => return Ok((engine_claim, lane_claim)),
                Err(error) => {
                    let _ = self.release_engine(engine_claim);
                    return Err(error);
                }
            }
        }

        let mut last_error = PcuError::unsupported();
        for descriptor in self.engines() {
            if requested_lanes > descriptor.lane_count {
                continue;
            }
            let Ok(engine_claim) = self.claim_engine(descriptor.id) else {
                continue;
            };
            match self.claim_lanes(descriptor.id, stage.lanes) {
                Ok(lane_claim) => return Ok((engine_claim, lane_claim)),
                Err(error) => {
                    last_error = error;
                    let _ = self.release_engine(engine_claim);
                }
            }
        }
        Err(last_error)
    }

    /// Claims, loads, and configures one pipeline stage.
    ///
    /// # Errors
    ///
    /// Returns any honest claim, load, or execution-configuration failure.
    pub fn prepare_pipeline_stage<'a>(
        &self,
        stage: &PcuPipelineStage<'a>,
        lowering_storage: &mut [u16],
    ) -> Result<PcuPipelineStageLease<'a>, PcuError> {
        let (engine_claim, lane_claim) = self.claim_pipeline_resources(stage)?;
        let program_lease = match self.load_program_source_with_execution(
            &engine_claim,
            Some(&lane_claim),
            stage.program,
            stage.execution,
            lowering_storage,
        ) {
            Ok(lease) => lease,
            Err(error) => {
                let _ = self.release_lanes(lane_claim);
                let _ = self.release_engine(engine_claim);
                return Err(error);
            }
        };

        Ok(PcuPipelineStageLease {
            name: stage.name,
            engine_claim,
            lane_claim,
            program_lease,
            trigger: stage.trigger,
            handoff: stage.handoff,
        })
    }

    /// Starts one prepared pipeline stage.
    ///
    /// # Errors
    ///
    /// Returns any honest backend control failure.
    pub fn activate_pipeline_stage(
        &self,
        stage: &PcuPipelineStageLease<'_>,
    ) -> Result<(), PcuError> {
        self.start_lanes(&stage.lane_claim)
    }

    /// Releases one prepared pipeline stage and its claimed resources.
    ///
    /// # Errors
    ///
    /// Returns the first honest failure encountered while stopping, unloading, or releasing the
    /// stage.
    pub fn release_pipeline_stage(&self, stage: PcuPipelineStageLease<'_>) -> Result<(), PcuError> {
        self.stop_lanes(&stage.lane_claim)?;
        self.unload_program(&stage.engine_claim, stage.program_lease)?;
        self.release_lanes(stage.lane_claim)?;
        self.release_engine(stage.engine_claim)
    }

    /// Performs one staged handoff from the current stage into the next stage.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the selected handoff mode is unsupported for the prepared
    /// stages.
    pub fn handoff_pipeline_stage(
        &self,
        current: &PcuPipelineStageLease<'_>,
        next: &PcuPipelineStageLease<'_>,
    ) -> Result<(), PcuError> {
        match next.handoff {
            PcuPipelineHandoff::Manual => Ok(()),
            PcuPipelineHandoff::StopThenStart => {
                self.stop_lanes(&current.lane_claim)?;
                self.start_lanes(&next.lane_claim)
            }
            PcuPipelineHandoff::PreloadedStartThenStop => {
                if current.engine_claim.engine() == next.engine_claim.engine() {
                    return Err(PcuError::unsupported());
                }
                self.start_lanes(&next.lane_claim)?;
                self.stop_lanes(&current.lane_claim)
            }
            PcuPipelineHandoff::RestartClaimedLanes => Err(PcuError::unsupported()),
        }
    }

    /// Reprograms one prepared stage in place and restarts the claimed lanes.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the next stage is incompatible with the current claim or the
    /// backend cannot reload or restart the lanes.
    pub fn reprogram_pipeline_stage<'a>(
        &self,
        current: &mut PcuPipelineStageLease<'a>,
        next: &PcuPipelineStage<'a>,
        lowering_storage: &mut [u16],
    ) -> Result<(), PcuError> {
        let next_engine = next.engine.unwrap_or_else(|| current.engine_claim.engine());
        if next_engine != current.engine_claim.engine()
            || next.lanes.bits() != current.lane_claim.lanes().bits()
        {
            return Err(PcuError::unsupported());
        }

        self.stop_lanes(&current.lane_claim)?;
        self.unload_program(&current.engine_claim, current.program_lease)?;
        let program_lease = self.load_program_source_with_execution(
            &current.engine_claim,
            Some(&current.lane_claim),
            next.program,
            next.execution,
            lowering_storage,
        )?;
        self.restart_lanes(&current.lane_claim)?;
        self.start_lanes(&current.lane_claim)?;
        current.name = next.name;
        current.program_lease = program_lease;
        current.trigger = next.trigger;
        current.handoff = next.handoff;
        Ok(())
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
    fn rp2350_lowering_keeps_non_default_execution_for_later_application() {
        let program = PcuIrProgram::new(super::super::PcuProgramId(1), &[PcuIrInstruction::Nop])
            .with_execution(PcuIrExecutionConfig {
                wrap_target: Some(0),
                wrap_source: Some(0),
                ..PcuIrExecutionConfig::default()
            });
        let mut storage = [0_u16; 1];
        let image = lower_rp2350_program(&program, &mut storage).expect(
            "instruction lowering should stay independent from execution-state application",
        );
        assert_eq!(image.words, &[rp2350_encode_nop()]);
    }

    #[test]
    fn rp2350_lowering_maps_exact_delay_cycles() {
        let program = PcuIrProgram::new(
            super::super::PcuProgramId(2),
            &[
                PcuIrInstruction::Delay { cycles: 1 },
                PcuIrInstruction::Delay { cycles: 32 },
            ],
        );
        let mut storage = [0_u16; 2];
        let image = lower_rp2350_program(&program, &mut storage).expect("delays should lower");

        assert_eq!(image.words[0], rp2350_encode_nop());
        assert_eq!(image.words[1], rp2350_encode_nop() | (31 << 8));
    }

    #[test]
    fn rp2350_lowering_applies_per_instruction_sideset_and_stall_cycles() {
        let timing = [
            PcuIrInstructionTiming {
                stall_cycles: 1,
                sideset_bits: Some(0),
            },
            PcuIrInstructionTiming {
                stall_cycles: 0,
                sideset_bits: Some(1),
            },
        ];
        let program = PcuIrProgram::new(
            super::super::PcuProgramId(5),
            &[
                PcuIrInstruction::Pull {
                    if_empty: false,
                    blocking: true,
                },
                PcuIrInstruction::Out {
                    destination: PcuIrOutDestination::Pins,
                    bit_count: 8,
                },
            ],
        )
        .with_execution(PcuIrExecutionConfig {
            pins: super::super::PcuIrPinConfig {
                sideset_base: Some(12),
                sideset_count: Some(1),
                sideset_optional: false,
                ..super::super::PcuIrPinConfig::default()
            },
            ..PcuIrExecutionConfig::default()
        })
        .with_timing(&timing);
        let mut storage = [0_u16; 2];
        let image = lower_rp2350_program(&program, &mut storage)
            .expect("timed program should lower with side-set payloads");

        assert_eq!(
            image.words,
            &[
                rp2350_encode_pull(false, true) | (1 << 8),
                rp2350_encode_out(PcuIrOutDestination::Pins, 8) | (1 << 12),
            ]
        );
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

    #[test]
    fn rp2350_lowering_supports_clocked_scanline_kernel_helper() {
        let mut instructions = [PcuIrInstruction::Nop; 3];
        let mut timing = [PcuIrInstructionTiming::default(); 3];
        let program = super::super::clocked_parallel_scanline_tx(
            super::super::PcuProgramId(13),
            8,
            2,
            10,
            &mut instructions,
            &mut timing,
        )
        .expect("kernel helper should build");
        let mut storage = [0_u16; 3];
        let image = lower_rp2350_program(&program, &mut storage)
            .expect("clocked scanline kernel should lower");

        assert_eq!(image.words[0], rp2350_encode_pull(false, true));
        assert_eq!(
            image.words[1],
            rp2350_encode_out(PcuIrOutDestination::Pins, 8) | (1 << 12)
        );
        assert_eq!(
            image.words[2],
            rp2350_encode_jmp_condition(PcuIrJumpCondition::Always, 0)
        );
    }

    #[test]
    fn rp2350_execution_registers_encode_supported_state() {
        let execution = PcuIrExecutionConfig {
            clocking: super::super::PcuIrClockConfig {
                divider_integer: Some(2),
                divider_fractional: Some(64),
            },
            pins: super::super::PcuIrPinConfig {
                input_base: Some(3),
                input_count: Some(6),
                output_base: Some(5),
                output_count: Some(8),
                set_base: Some(7),
                set_count: Some(2),
                sideset_base: Some(9),
                sideset_count: Some(2),
                sideset_optional: true,
                jmp_pin: Some(11),
            },
            shift: super::super::PcuIrShiftConfig {
                in_direction: Some(super::super::PcuIrShiftDirection::Left),
                out_direction: Some(super::super::PcuIrShiftDirection::Right),
                autopush_threshold: Some(8),
                autopull_threshold: Some(16),
            },
            wrap_target: Some(1),
            wrap_source: Some(9),
        };

        let (clkdiv, execctrl, shiftctrl, pinctrl) =
            rp2350_build_execution_registers(&execution, None).expect("config should encode");

        assert_eq!(clkdiv, 0x0002_4000);
        assert_eq!(execctrl, 0x4b00_9080);
        assert_eq!(shiftctrl, 0x208b_0006);
        assert_eq!(pinctrl, 0x6881_a4e5);
    }

    #[test]
    fn rp2350_execution_registers_derive_output_and_set_counts_from_program() {
        let program = PcuIrProgram::new(
            super::super::PcuProgramId(12),
            &[
                PcuIrInstruction::Out {
                    destination: PcuIrOutDestination::Pins,
                    bit_count: 6,
                },
                PcuIrInstruction::Mov {
                    destination: PcuIrMovDestination::Pins,
                    operation: PcuIrMovOperation::None,
                    source: PcuIrMovSource::Osr,
                },
                PcuIrInstruction::Set {
                    destination: PcuIrSetDestination::Pins,
                    value: 7,
                },
            ],
        )
        .with_execution(PcuIrExecutionConfig {
            pins: super::super::PcuIrPinConfig {
                output_base: Some(2),
                set_base: Some(4),
                ..super::super::PcuIrPinConfig::default()
            },
            ..PcuIrExecutionConfig::default()
        });

        let (_, _, _, pinctrl) =
            rp2350_build_execution_registers(&program.execution, Some(program.instructions))
                .expect("program-derived counts should encode");

        assert_eq!((pinctrl >> 26) & 0x7, 5);
        assert_eq!((pinctrl >> 20) & 0x3f, 0);
        assert_eq!(pinctrl & 0x1f, 2);
        assert_eq!((pinctrl >> 5) & 0x1f, 4);
    }

    #[test]
    fn rp2350_execution_registers_reject_invalid_fractional_zero_divider() {
        let execution = PcuIrExecutionConfig {
            clocking: super::super::PcuIrClockConfig {
                divider_integer: Some(0),
                divider_fractional: Some(1),
            },
            ..PcuIrExecutionConfig::default()
        };

        let error = rp2350_build_execution_registers(&execution, None)
            .expect_err("fractional divider without integer component should fail");
        assert_eq!(error.kind(), PcuError::invalid().kind());
    }
}
