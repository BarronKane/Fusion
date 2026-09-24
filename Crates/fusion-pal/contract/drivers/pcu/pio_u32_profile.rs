//! RP2350 PIO U32 Stream admission and reference profile.

use crate::contract::drivers::pcu::{
    PcuInvocationBindings,
    PcuInvocationParameters,
    PcuPortDirection,
    PcuPortRate,
    PcuStreamKernelIr,
    PcuStreamPattern,
    PcuValueType,
};

/// Precise rejection reasons for the common RP2350 PIO U32 stream profile.
///
/// The profile intentionally admits exactly one operation per program, with one U32 stream
/// input and output and no resources or runtime parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuPioU32StreamProfileError {
    InvalidPortCount,
    InvalidPortShape,
    InvalidValueType(PcuValueType),
    KernelBindingsPresent,
    ParametersPresent,
    InvalidPatternCount { found: usize },
    UnsupportedPattern(PcuStreamPattern),
    InvalidPattern(PcuStreamPattern),
    RuntimeBindingsPresent,
    RuntimeParametersPresent,
}

/// Validates one program against the common one-pattern U32 subset shared with RP2350 PIO.
///
/// Port order is significant: port zero is a stream input and port one a stream output. The
/// accepted operation patterns are bit reverse, bit invert, increment, decrement, logical shifts,
/// bit extraction, low-bit mask, and U32 byte swap. Parameterized arithmetic and resource-backed
/// programs are outside this profile.
///
/// # Errors
///
/// Returns the first port, binding, parameter, or operation mismatch.
pub fn validate_pio_u32_stream_profile(
    kernel: &PcuStreamKernelIr<'_>,
) -> Result<(), PcuPioU32StreamProfileError> {
    let [input, output] = kernel.ports else {
        return Err(PcuPioU32StreamProfileError::InvalidPortCount);
    };
    if input.direction != PcuPortDirection::Input
        || output.direction != PcuPortDirection::Output
        || input.rate != PcuPortRate::Stream
        || output.rate != PcuPortRate::Stream
    {
        return Err(PcuPioU32StreamProfileError::InvalidPortShape);
    }
    for value_type in [input.value_type, output.value_type] {
        if value_type != PcuValueType::u32() {
            return Err(PcuPioU32StreamProfileError::InvalidValueType(value_type));
        }
    }
    if !kernel.bindings.is_empty() {
        return Err(PcuPioU32StreamProfileError::KernelBindingsPresent);
    }
    if !kernel.parameters.is_empty() {
        return Err(PcuPioU32StreamProfileError::ParametersPresent);
    }
    let [pattern] = kernel.patterns else {
        return Err(PcuPioU32StreamProfileError::InvalidPatternCount {
            found: kernel.patterns.len(),
        });
    };
    let pattern = *pattern;
    let supported = match pattern {
        PcuStreamPattern::BitReverse
        | PcuStreamPattern::BitInvert
        | PcuStreamPattern::Increment
        | PcuStreamPattern::Decrement
        | PcuStreamPattern::ByteSwap32 => true,
        PcuStreamPattern::ShiftLeft { bits } | PcuStreamPattern::ShiftRight { bits } => {
            (1..=32).contains(&bits)
        }
        PcuStreamPattern::ExtractBits { offset, width } => {
            width >= 1 && offset < 32 && u16::from(offset) + u16::from(width) <= 32
        }
        PcuStreamPattern::MaskLower { bits } => (1..=32).contains(&bits),
        PcuStreamPattern::AddParameter { .. } | PcuStreamPattern::XorParameter { .. } => false,
    };
    if !supported {
        return if matches!(
            pattern,
            PcuStreamPattern::AddParameter { .. } | PcuStreamPattern::XorParameter { .. }
        ) {
            Err(PcuPioU32StreamProfileError::UnsupportedPattern(pattern))
        } else {
            Err(PcuPioU32StreamProfileError::InvalidPattern(pattern))
        };
    }
    Ok(())
}

/// Validates both the static program shape and the empty runtime binding tables required by the
/// common RP2350 PIO U32 profile.
///
/// # Errors
///
/// Returns a profile error for unsupported program shape or nonempty runtime bindings.
pub fn validate_pio_u32_stream_invocation(
    kernel: &PcuStreamKernelIr<'_>,
    bindings: PcuInvocationBindings<'_>,
    parameters: PcuInvocationParameters<'_>,
) -> Result<(), PcuPioU32StreamProfileError> {
    validate_pio_u32_stream_profile(kernel)?;
    if !bindings.is_empty() {
        return Err(PcuPioU32StreamProfileError::RuntimeBindingsPresent);
    }
    if !parameters.bindings.is_empty() {
        return Err(PcuPioU32StreamProfileError::RuntimeParametersPresent);
    }
    Ok(())
}

/// A shared input/output example for the common PIO U32 profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuPioU32StreamVector {
    pub pattern: PcuStreamPattern,
    pub input: u32,
    pub expected: u32,
}

/// Small deterministic vectors suitable for CPU and hardware conformance checks.
pub const PCU_PIO_U32_STREAM_VECTORS: &[PcuPioU32StreamVector] = &[
    PcuPioU32StreamVector {
        pattern: PcuStreamPattern::BitReverse,
        input: 0x0000_0001,
        expected: 0x8000_0000,
    },
    PcuPioU32StreamVector {
        pattern: PcuStreamPattern::BitInvert,
        input: 0x00ff_00ff,
        expected: 0xff00_ff00,
    },
    PcuPioU32StreamVector {
        pattern: PcuStreamPattern::Increment,
        input: u32::MAX,
        expected: 0,
    },
    PcuPioU32StreamVector {
        pattern: PcuStreamPattern::Decrement,
        input: 0,
        expected: u32::MAX,
    },
    PcuPioU32StreamVector {
        pattern: PcuStreamPattern::ShiftLeft { bits: 3 },
        input: 0x8000_0003,
        expected: 0x0000_0018,
    },
    PcuPioU32StreamVector {
        pattern: PcuStreamPattern::ShiftRight { bits: 4 },
        input: 0x8000_003f,
        expected: 0x0800_0003,
    },
    PcuPioU32StreamVector {
        pattern: PcuStreamPattern::ShiftLeft { bits: 32 },
        input: 0x1234_5678,
        expected: 0,
    },
    PcuPioU32StreamVector {
        pattern: PcuStreamPattern::ExtractBits {
            offset: 8,
            width: 8,
        },
        input: 0x1234_56ab,
        expected: 0x56,
    },
    PcuPioU32StreamVector {
        pattern: PcuStreamPattern::MaskLower { bits: 12 },
        input: 0xabcd_1234,
        expected: 0x234,
    },
    PcuPioU32StreamVector {
        pattern: PcuStreamPattern::ByteSwap32,
        input: 0x1234_56ab,
        expected: 0xab56_3412,
    },
];

/// Executes one admitted U32 profile pattern as a CPU reference operation.
#[must_use]
pub fn execute_pio_u32_stream_reference(pattern: PcuStreamPattern, input: u32) -> Option<u32> {
    Some(match pattern {
        PcuStreamPattern::BitReverse => input.reverse_bits(),
        PcuStreamPattern::BitInvert => !input,
        PcuStreamPattern::Increment => input.wrapping_add(1),
        PcuStreamPattern::Decrement => input.wrapping_sub(1),
        PcuStreamPattern::ShiftLeft { bits } if (1..=32).contains(&bits) => {
            if bits == 32 {
                0
            } else {
                input << bits
            }
        }
        PcuStreamPattern::ShiftRight { bits } if (1..=32).contains(&bits) => {
            if bits == 32 {
                0
            } else {
                input >> bits
            }
        }
        PcuStreamPattern::ExtractBits { offset, width }
            if width >= 1 && offset < 32 && u16::from(offset) + u16::from(width) <= 32 =>
        {
            let mask = if width == 32 {
                u32::MAX
            } else {
                (1_u32 << width) - 1
            };
            (input >> offset) & mask
        }
        PcuStreamPattern::MaskLower { bits } if (1..=32).contains(&bits) => {
            if bits == 32 {
                input
            } else {
                input & ((1_u32 << bits) - 1)
            }
        }
        PcuStreamPattern::ByteSwap32 => input.swap_bytes(),
        _ => return None,
    })
}

#[cfg(test)]
mod pio_u32_profile_tests {
    use super::*;
    use fusion_pcu::model::PcuStreamKernelBuilder;
    use fusion_pcu::PcuParameterSlot;

    #[test]
    fn shared_vectors_match_cpu_reference() {
        for vector in PCU_PIO_U32_STREAM_VECTORS {
            assert_eq!(
                execute_pio_u32_stream_reference(vector.pattern, vector.input),
                Some(vector.expected),
                "pattern {:?} input {:#010x}",
                vector.pattern,
                vector.input,
            );
            let builder = PcuStreamKernelBuilder::<1>::words(99, "conformance")
                .with_pattern(vector.pattern)
                .expect("one pattern fits");
            assert_eq!(validate_pio_u32_stream_profile(&builder.ir()), Ok(()));
        }
    }

    #[test]
    fn admits_one_u32_pattern_without_runtime_state() {
        let builder = PcuStreamKernelBuilder::<1>::words(1, "profile")
            .increment()
            .expect("one pattern fits");
        let kernel = builder.ir();
        assert_eq!(validate_pio_u32_stream_profile(&kernel), Ok(()));
        assert_eq!(
            validate_pio_u32_stream_invocation(
                &kernel,
                PcuInvocationBindings::empty(),
                PcuInvocationParameters::empty(),
            ),
            Ok(())
        );
    }

    #[test]
    fn profile_rejects_multiple_patterns_parameters_and_non_u32() {
        let multiple_builder = PcuStreamKernelBuilder::<2>::words(2, "multiple")
            .increment()
            .expect("pattern fits")
            .decrement()
            .expect("pattern fits");
        let multiple = multiple_builder.ir();
        assert_eq!(
            validate_pio_u32_stream_profile(&multiple),
            Err(PcuPioU32StreamProfileError::InvalidPatternCount { found: 2 })
        );

        let parameter =
            fusion_pcu::PcuParameter::named(PcuParameterSlot(0), "delta", PcuValueType::u32());
        let parameterized_builder = PcuStreamKernelBuilder::<1>::words(3, "parameterized")
            .with_parameters(core::slice::from_ref(&parameter))
            .with_pattern(PcuStreamPattern::AddParameter {
                parameter: PcuParameterSlot(0),
            })
            .expect("pattern fits");
        let parameterized = parameterized_builder.ir();
        assert_eq!(
            validate_pio_u32_stream_profile(&parameterized),
            Err(PcuPioU32StreamProfileError::ParametersPresent)
        );

        let non_u32_builder = PcuStreamKernelBuilder::<1>::half_words(4, "u16")
            .increment()
            .expect("pattern fits");
        let non_u32 = non_u32_builder.ir();
        assert_eq!(
            validate_pio_u32_stream_profile(&non_u32),
            Err(PcuPioU32StreamProfileError::InvalidValueType(
                PcuValueType::u16()
            ))
        );
    }
}
