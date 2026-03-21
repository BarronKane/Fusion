//! Reusable deterministic PCU kernel helpers.

use super::{
    PcuError,
    PcuIrExecutionConfig,
    PcuIrInstruction,
    PcuIrInstructionTiming,
    PcuIrOutDestination,
    PcuIrPinConfig,
    PcuIrProgram,
    PcuProgramId,
};

use super::PcuIrInstruction::{Jump, Out, Pull};
use super::PcuIrJumpCondition;

/// Builds one simple streaming TX kernel that continuously pulls words and shifts them to pins.
///
/// The returned program loops forever with wrap bounds `0..=2`.
///
/// # Errors
///
/// Returns an error when the supplied output width is zero or exceeds the RP2350-model pin width.
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

/// Builds one clocked scanline-style TX kernel using one side-set clock pin.
///
/// This helper is intentionally narrow and RP2350-shaped: it models one data write framed by a
/// low/high/low side-set clock pulse suitable for display- or protocol-style scanline engines.
///
/// # Errors
///
/// Returns an error when the output width is zero, exceeds 32 pins, or the side-set pin is out
/// of the backend-visible range.
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
}
