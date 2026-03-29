//! Reusable deterministic PIO kernel helpers.

use crate::contract::drivers::pcu::PcuError;

use super::{
    PcuIrExecutionConfig,
    PcuIrInSource,
    PcuIrInstruction,
    PcuIrInstructionTiming,
    PcuIrMovDestination,
    PcuIrMovOperation,
    PcuIrMovSource,
    PcuIrOutDestination,
    PcuIrPinConfig,
    PcuIrProgram,
    PcuIrShiftConfig,
    PcuIrShiftDirection,
    PcuProgramId,
};

use super::PcuIrInstruction::{In, Jump, Mov, Nop, Out, Pull, Push};
use super::PcuIrJumpCondition;

pub fn streaming_parallel_tx(
    id: PcuProgramId,
    bit_count: u8,
    instructions: &mut [PcuIrInstruction; 3],
) -> Result<PcuIrProgram<'_>, PcuError> {
    if bit_count == 0 || bit_count > 32 {
        return Err(PcuError::invalid());
    }

    instructions[0] = Pull {
        if_empty: false,
        blocking: true,
    };
    instructions[1] = Out {
        destination: PcuIrOutDestination::Pins,
        bit_count,
    };
    instructions[2] = Jump {
        condition: PcuIrJumpCondition::Always,
        target: 0,
    };

    Ok(PcuIrProgram::new(id, &instructions[..]).with_wrap(0, 2))
}

fn streaming_unary_word_transform(
    id: PcuProgramId,
    operation: PcuIrMovOperation,
    instructions: &mut [PcuIrInstruction; 4],
) -> PcuIrProgram<'_> {
    instructions[0] = Pull {
        if_empty: false,
        blocking: true,
    };
    instructions[1] = Mov {
        destination: PcuIrMovDestination::Isr,
        operation,
        source: PcuIrMovSource::Osr,
    };
    instructions[2] = Push {
        if_full: false,
        blocking: true,
    };
    instructions[3] = Jump {
        condition: PcuIrJumpCondition::Always,
        target: 0,
    };

    PcuIrProgram::new(id, &instructions[..]).with_wrap(0, 3)
}

fn streaming_increment_word_transform(
    id: PcuProgramId,
    instructions: &mut [PcuIrInstruction; 8],
) -> PcuIrProgram<'_> {
    instructions[0] = Pull {
        if_empty: false,
        blocking: true,
    };
    instructions[1] = Mov {
        destination: PcuIrMovDestination::X,
        operation: PcuIrMovOperation::None,
        source: PcuIrMovSource::Osr,
    };
    instructions[2] = Mov {
        destination: PcuIrMovDestination::Y,
        operation: PcuIrMovOperation::Invert,
        source: PcuIrMovSource::X,
    };
    instructions[3] = Jump {
        condition: PcuIrJumpCondition::YDecNonZero,
        target: 4,
    };
    instructions[4] = Mov {
        destination: PcuIrMovDestination::X,
        operation: PcuIrMovOperation::Invert,
        source: PcuIrMovSource::Y,
    };
    instructions[5] = Mov {
        destination: PcuIrMovDestination::Isr,
        operation: PcuIrMovOperation::None,
        source: PcuIrMovSource::X,
    };
    instructions[6] = Push {
        if_full: false,
        blocking: true,
    };
    instructions[7] = Jump {
        condition: PcuIrJumpCondition::Always,
        target: 0,
    };

    PcuIrProgram::new(id, &instructions[..]).with_wrap(0, 7)
}

fn streaming_shifted_word_transform(
    id: PcuProgramId,
    bit_count: u8,
    direction: PcuIrShiftDirection,
    instructions: &mut [PcuIrInstruction; 5],
) -> Result<PcuIrProgram<'_>, PcuError> {
    if bit_count == 0 || bit_count > 32 {
        return Err(PcuError::invalid());
    }

    instructions[0] = Pull {
        if_empty: false,
        blocking: true,
    };
    instructions[1] = Out {
        destination: PcuIrOutDestination::Null,
        bit_count,
    };
    instructions[2] = Mov {
        destination: PcuIrMovDestination::Isr,
        operation: PcuIrMovOperation::None,
        source: PcuIrMovSource::Osr,
    };
    instructions[3] = Push {
        if_full: false,
        blocking: true,
    };
    instructions[4] = Jump {
        condition: PcuIrJumpCondition::Always,
        target: 0,
    };

    Ok(PcuIrProgram::new(id, &instructions[..])
        .with_wrap(0, 4)
        .with_execution(PcuIrExecutionConfig {
            shift: PcuIrShiftConfig {
                out_direction: Some(direction),
                ..PcuIrShiftConfig::default()
            },
            ..PcuIrExecutionConfig::default()
        }))
}

fn streaming_extract_bits_word_transform(
    id: PcuProgramId,
    offset: u8,
    width: u8,
    instructions: &mut [PcuIrInstruction; 6],
) -> Result<PcuIrProgram<'_>, PcuError> {
    if width == 0 || width > 32 || offset >= 32 || u16::from(offset) + u16::from(width) > 32 {
        return Err(PcuError::invalid());
    }

    instructions[0] = Pull {
        if_empty: false,
        blocking: true,
    };
    instructions[1] = Mov {
        destination: PcuIrMovDestination::Isr,
        operation: PcuIrMovOperation::None,
        source: PcuIrMovSource::Null,
    };
    instructions[2] = if offset == 0 {
        Nop
    } else {
        Out {
            destination: PcuIrOutDestination::Null,
            bit_count: offset,
        }
    };
    instructions[3] = Out {
        destination: PcuIrOutDestination::Isr,
        bit_count: width,
    };
    instructions[4] = Push {
        if_full: false,
        blocking: true,
    };
    instructions[5] = Jump {
        condition: PcuIrJumpCondition::Always,
        target: 0,
    };

    Ok(PcuIrProgram::new(id, &instructions[..])
        .with_wrap(0, 5)
        .with_execution(PcuIrExecutionConfig {
            shift: PcuIrShiftConfig {
                out_direction: Some(PcuIrShiftDirection::Right),
                ..PcuIrShiftConfig::default()
            },
            ..PcuIrExecutionConfig::default()
        }))
}

fn streaming_byte_swap32_word_transform(
    id: PcuProgramId,
    instructions: &mut [PcuIrInstruction; 12],
) -> PcuIrProgram<'_> {
    instructions[0] = Pull {
        if_empty: false,
        blocking: true,
    };
    instructions[1] = Mov {
        destination: PcuIrMovDestination::Isr,
        operation: PcuIrMovOperation::None,
        source: PcuIrMovSource::Null,
    };
    instructions[2] = Out {
        destination: PcuIrOutDestination::X,
        bit_count: 8,
    };
    instructions[3] = Out {
        destination: PcuIrOutDestination::Y,
        bit_count: 8,
    };
    instructions[4] = In {
        source: PcuIrInSource::X,
        bit_count: 8,
    };
    instructions[5] = In {
        source: PcuIrInSource::Y,
        bit_count: 8,
    };
    instructions[6] = Out {
        destination: PcuIrOutDestination::X,
        bit_count: 8,
    };
    instructions[7] = Out {
        destination: PcuIrOutDestination::Y,
        bit_count: 8,
    };
    instructions[8] = In {
        source: PcuIrInSource::X,
        bit_count: 8,
    };
    instructions[9] = In {
        source: PcuIrInSource::Y,
        bit_count: 8,
    };
    instructions[10] = Push {
        if_full: false,
        blocking: true,
    };
    instructions[11] = Jump {
        condition: PcuIrJumpCondition::Always,
        target: 0,
    };

    PcuIrProgram::new(id, &instructions[..])
        .with_wrap(0, 11)
        .with_execution(PcuIrExecutionConfig {
            shift: PcuIrShiftConfig {
                in_direction: Some(PcuIrShiftDirection::Left),
                out_direction: Some(PcuIrShiftDirection::Right),
                ..PcuIrShiftConfig::default()
            },
            ..PcuIrExecutionConfig::default()
        })
}

#[must_use]
pub fn bit_reverse_stream_transform(
    id: PcuProgramId,
    instructions: &mut [PcuIrInstruction; 4],
) -> PcuIrProgram<'_> {
    streaming_unary_word_transform(id, PcuIrMovOperation::Reverse, instructions)
}

#[must_use]
pub fn bit_invert_stream_transform(
    id: PcuProgramId,
    instructions: &mut [PcuIrInstruction; 4],
) -> PcuIrProgram<'_> {
    streaming_unary_word_transform(id, PcuIrMovOperation::Invert, instructions)
}

#[must_use]
pub fn increment_stream_transform(
    id: PcuProgramId,
    instructions: &mut [PcuIrInstruction; 8],
) -> PcuIrProgram<'_> {
    streaming_increment_word_transform(id, instructions)
}

pub fn shift_left_stream_transform(
    id: PcuProgramId,
    bit_count: u8,
    instructions: &mut [PcuIrInstruction; 5],
) -> Result<PcuIrProgram<'_>, PcuError> {
    streaming_shifted_word_transform(id, bit_count, PcuIrShiftDirection::Left, instructions)
}

pub fn shift_right_stream_transform(
    id: PcuProgramId,
    bit_count: u8,
    instructions: &mut [PcuIrInstruction; 5],
) -> Result<PcuIrProgram<'_>, PcuError> {
    streaming_shifted_word_transform(id, bit_count, PcuIrShiftDirection::Right, instructions)
}

pub fn extract_bits_stream_transform(
    id: PcuProgramId,
    offset: u8,
    width: u8,
    instructions: &mut [PcuIrInstruction; 6],
) -> Result<PcuIrProgram<'_>, PcuError> {
    streaming_extract_bits_word_transform(id, offset, width, instructions)
}

pub fn mask_lower_stream_transform(
    id: PcuProgramId,
    bit_count: u8,
    instructions: &mut [PcuIrInstruction; 6],
) -> Result<PcuIrProgram<'_>, PcuError> {
    streaming_extract_bits_word_transform(id, 0, bit_count, instructions)
}

#[must_use]
pub fn byte_swap32_stream_transform(
    id: PcuProgramId,
    instructions: &mut [PcuIrInstruction; 12],
) -> PcuIrProgram<'_> {
    streaming_byte_swap32_word_transform(id, instructions)
}

pub fn clocked_parallel_scanline_tx<'a>(
    id: PcuProgramId,
    bit_count: u8,
    output_base: u8,
    clock_pin: u8,
    instructions: &'a mut [PcuIrInstruction; 3],
    timing: &'a mut [PcuIrInstructionTiming; 3],
) -> Result<PcuIrProgram<'a>, PcuError> {
    if bit_count == 0 || bit_count > 32 || output_base > 31 || clock_pin > 31 {
        return Err(PcuError::invalid());
    }

    let mut program = streaming_parallel_tx(id, bit_count, instructions)?;
    timing[0] = PcuIrInstructionTiming {
        stall_cycles: 0,
        sideset_bits: Some(0),
    };
    timing[1] = PcuIrInstructionTiming {
        stall_cycles: 0,
        sideset_bits: Some(1),
    };
    timing[2] = PcuIrInstructionTiming {
        stall_cycles: 0,
        sideset_bits: Some(0),
    };
    program = program
        .with_timing(&timing[..])
        .with_execution(PcuIrExecutionConfig {
            pins: PcuIrPinConfig {
                output_base: Some(output_base),
                output_count: Some(bit_count),
                sideset_base: Some(clock_pin),
                sideset_count: Some(1),
                sideset_optional: false,
                ..PcuIrPinConfig::default()
            },
            ..PcuIrExecutionConfig::default()
        });
    Ok(program)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streaming_parallel_tx_builds_looping_kernel() {
        let mut instructions = [PcuIrInstruction::Nop; 3];
        let program = streaming_parallel_tx(PcuProgramId(3), 8, &mut instructions)
            .expect("streaming helper should build");

        assert_eq!(program.instructions.len(), 3);
        assert_eq!(program.execution.wrap_target, Some(0));
        assert_eq!(program.execution.wrap_source, Some(2));
    }

    #[test]
    fn clocked_parallel_scanline_tx_applies_sideset_config() {
        let mut instructions = [PcuIrInstruction::Nop; 3];
        let mut timing = [PcuIrInstructionTiming::default(); 3];
        let program =
            clocked_parallel_scanline_tx(PcuProgramId(4), 8, 2, 10, &mut instructions, &mut timing)
                .expect("clocked scanline helper should build");

        assert_eq!(program.execution.pins.output_base, Some(2));
        assert_eq!(program.execution.pins.sideset_base, Some(10));
        assert_eq!(program.execution.pins.sideset_count, Some(1));
        assert_eq!(
            program
                .timing
                .expect("clocked scanline helper should attach timing")[1]
                .sideset_bits,
            Some(1)
        );
    }

    #[test]
    fn bit_reverse_stream_transform_builds_looping_kernel() {
        let mut instructions = [PcuIrInstruction::Nop; 4];
        let program = bit_reverse_stream_transform(PcuProgramId(5), &mut instructions);

        assert_eq!(program.instructions.len(), 4);
        assert_eq!(program.execution.wrap_target, Some(0));
        assert_eq!(program.execution.wrap_source, Some(3));
    }

    #[test]
    fn shift_stream_transforms_apply_explicit_shift_direction() {
        let mut left_instructions = [PcuIrInstruction::Nop; 5];
        let left = shift_left_stream_transform(PcuProgramId(6), 7, &mut left_instructions)
            .expect("left shift helper should build");
        let mut right_instructions = [PcuIrInstruction::Nop; 5];
        let right = shift_right_stream_transform(PcuProgramId(7), 9, &mut right_instructions)
            .expect("right shift helper should build");

        assert_eq!(
            left.execution.shift.out_direction,
            Some(PcuIrShiftDirection::Left)
        );
        assert_eq!(
            right.execution.shift.out_direction,
            Some(PcuIrShiftDirection::Right)
        );
        assert!(matches!(
            left.instructions[1],
            PcuIrInstruction::Out {
                destination: PcuIrOutDestination::Null,
                bit_count: 7
            }
        ));
    }

    #[test]
    fn extract_and_mask_helpers_build_parameterized_programs() {
        let mut extract_instructions = [PcuIrInstruction::Nop; 6];
        let extract =
            extract_bits_stream_transform(PcuProgramId(8), 5, 11, &mut extract_instructions)
                .expect("extract helper should build");
        let mut mask_instructions = [PcuIrInstruction::Nop; 6];
        let mask = mask_lower_stream_transform(PcuProgramId(9), 12, &mut mask_instructions)
            .expect("mask helper should build");

        assert!(matches!(
            extract.instructions[2],
            PcuIrInstruction::Out {
                destination: PcuIrOutDestination::Null,
                bit_count: 5
            }
        ));
        assert!(matches!(
            extract.instructions[3],
            PcuIrInstruction::Out {
                destination: PcuIrOutDestination::Isr,
                bit_count: 11
            }
        ));
        assert!(matches!(mask.instructions[2], PcuIrInstruction::Nop));
    }

    #[test]
    fn byte_swap32_helper_uses_scratch_reassembly() {
        let mut instructions = [PcuIrInstruction::Nop; 12];
        let program = byte_swap32_stream_transform(PcuProgramId(10), &mut instructions);

        assert_eq!(program.instructions.len(), 12);
        assert_eq!(
            program.execution.shift.in_direction,
            Some(PcuIrShiftDirection::Left)
        );
        assert_eq!(
            program.execution.shift.out_direction,
            Some(PcuIrShiftDirection::Right)
        );
        assert!(matches!(
            program.instructions[2],
            PcuIrInstruction::Out {
                destination: PcuIrOutDestination::X,
                bit_count: 8
            }
        ));
        assert!(matches!(
            program.instructions[4],
            PcuIrInstruction::In {
                source: PcuIrInSource::X,
                bit_count: 8
            }
        ));
    }

    #[test]
    fn increment_helper_builds_wrapped_kernel() {
        let mut instructions = [PcuIrInstruction::Nop; 8];
        let program = increment_stream_transform(PcuProgramId(10), &mut instructions);

        assert_eq!(program.instructions.len(), 8);
        assert_eq!(program.execution.wrap_target, Some(0));
        assert_eq!(program.execution.wrap_source, Some(7));
        assert!(matches!(
            program.instructions[3],
            PcuIrInstruction::Jump {
                condition: PcuIrJumpCondition::YDecNonZero,
                target: 4
            }
        ));
    }
}
