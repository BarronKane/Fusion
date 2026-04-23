//! Shared hosted CPU-backed PCU helpers.

use crate::contract::drivers::pcu::{
    PcuCaps,
    PcuCommandSupport,
    PcuDispatchPolicyCaps,
    PcuDispatchSupport,
    PcuError,
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
    PcuParameterBinding,
    PcuParameterValue,
    PcuPersistentHandle,
    PcuPersistentState,
    PcuPrimitiveCaps,
    PcuPrimitiveSupport,
    PcuSignalSupport,
    PcuStreamCapabilities,
    PcuStreamInstallation,
    PcuStreamPattern,
    PcuStreamSupport,
    PcuStreamValueType,
    PcuSupport,
    PcuTransactionSupport,
};

const HOSTED_CPU_MAX_STREAM_PATTERNS: usize = 16;
const HOSTED_CPU_MAX_PARAMETER_BINDINGS: usize = 16;

pub const HOST_CPU_EXECUTOR_ID: PcuExecutorId = PcuExecutorId(0);

pub const HOST_CPU_STREAM_DIRECT_SUPPORT: PcuStreamCapabilities = PcuStreamCapabilities::FIFO_INPUT
    .union(PcuStreamCapabilities::FIFO_OUTPUT)
    .union(PcuStreamCapabilities::BIT_REVERSE)
    .union(PcuStreamCapabilities::BIT_INVERT)
    .union(PcuStreamCapabilities::INCREMENT)
    .union(PcuStreamCapabilities::DECREMENT)
    .union(PcuStreamCapabilities::ADD_PARAMETER)
    .union(PcuStreamCapabilities::XOR_PARAMETER)
    .union(PcuStreamCapabilities::SHIFT_LEFT)
    .union(PcuStreamCapabilities::SHIFT_RIGHT)
    .union(PcuStreamCapabilities::EXTRACT_BITS)
    .union(PcuStreamCapabilities::MASK_LOWER)
    .union(PcuStreamCapabilities::BYTE_SWAP32);

pub const HOST_CPU_EXECUTOR_SUPPORT: PcuExecutorSupport = PcuExecutorSupport {
    primitives: PcuPrimitiveCaps::STREAM,
    dispatch_policy: PcuDispatchPolicyCaps::PERSISTENT_INSTALL,
    dispatch_instructions: crate::contract::drivers::pcu::PcuDispatchOpCaps::empty(),
    dispatch_types: crate::contract::drivers::pcu::PcuValueTypeCaps::empty(),
    dispatch_features: crate::contract::drivers::pcu::PcuDispatchFeatureCaps::empty(),
    stream_instructions: HOST_CPU_STREAM_DIRECT_SUPPORT,
    command_instructions: crate::contract::drivers::pcu::PcuCommandOpCaps::empty(),
    transaction_features: crate::contract::drivers::pcu::PcuTransactionFeatureCaps::empty(),
    signal_instructions: crate::contract::drivers::pcu::PcuSignalOpCaps::empty(),
};

pub const HOST_PRIMITIVE_SUPPORT: PcuPrimitiveSupport = PcuPrimitiveSupport {
    primitives: PcuFeatureSupport::new(PcuPrimitiveCaps::STREAM, PcuPrimitiveCaps::empty()),
};

pub const HOST_DISPATCH_SUPPORT: PcuDispatchSupport = PcuDispatchSupport {
    flags: PcuDispatchPolicyCaps::PERSISTENT_INSTALL,
    instructions: PcuFeatureSupport::new(
        crate::contract::drivers::pcu::PcuDispatchOpCaps::empty(),
        crate::contract::drivers::pcu::PcuDispatchOpCaps::empty(),
    ),
    types: PcuFeatureSupport::new(
        crate::contract::drivers::pcu::PcuValueTypeCaps::empty(),
        crate::contract::drivers::pcu::PcuValueTypeCaps::empty(),
    ),
    features: PcuFeatureSupport::new(
        crate::contract::drivers::pcu::PcuDispatchFeatureCaps::empty(),
        crate::contract::drivers::pcu::PcuDispatchFeatureCaps::empty(),
    ),
};

pub const HOST_STREAM_SUPPORT: PcuStreamSupport = PcuStreamSupport {
    instructions: PcuFeatureSupport::new(
        HOST_CPU_STREAM_DIRECT_SUPPORT,
        PcuStreamCapabilities::empty(),
    ),
};

pub const HOST_COMMAND_SUPPORT: PcuCommandSupport = PcuCommandSupport {
    instructions: PcuFeatureSupport::new(
        crate::contract::drivers::pcu::PcuCommandOpCaps::empty(),
        crate::contract::drivers::pcu::PcuCommandOpCaps::empty(),
    ),
};

pub const HOST_TRANSACTION_SUPPORT: PcuTransactionSupport = PcuTransactionSupport {
    features: PcuFeatureSupport::new(
        crate::contract::drivers::pcu::PcuTransactionFeatureCaps::empty(),
        crate::contract::drivers::pcu::PcuTransactionFeatureCaps::empty(),
    ),
};

pub const HOST_SIGNAL_SUPPORT: PcuSignalSupport = PcuSignalSupport {
    instructions: PcuFeatureSupport::new(
        crate::contract::drivers::pcu::PcuSignalOpCaps::empty(),
        crate::contract::drivers::pcu::PcuSignalOpCaps::empty(),
    ),
};

#[must_use]
pub const fn host_cpu_executor_descriptor() -> PcuExecutorDescriptor {
    PcuExecutorDescriptor {
        id: HOST_CPU_EXECUTOR_ID,
        name: "host-cpu",
        class: PcuExecutorClass::Cpu,
        origin: PcuExecutorOrigin::Synthetic,
        support: HOST_CPU_EXECUTOR_SUPPORT,
    }
}

#[must_use]
pub const fn host_pcu_support() -> PcuSupport {
    PcuSupport {
        caps: PcuCaps::ENUMERATE_EXECUTORS
            .union(PcuCaps::CLAIM_EXECUTOR)
            .union(PcuCaps::DISPATCH)
            .union(PcuCaps::COMPLETION_STATUS),
        implementation: PcuImplementationKind::Native,
        executor_count: 1,
        primitive_support: HOST_PRIMITIVE_SUPPORT,
        dispatch_support: HOST_DISPATCH_SUPPORT,
        stream_support: HOST_STREAM_SUPPORT,
        command_support: HOST_COMMAND_SUPPORT,
        transaction_support: HOST_TRANSACTION_SUPPORT,
        signal_support: HOST_SIGNAL_SUPPORT,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostedCpuUnsupportedFiniteHandle;

impl PcuFiniteHandle for HostedCpuUnsupportedFiniteHandle {
    fn state(&self) -> Result<PcuFiniteState, PcuError> {
        Err(PcuError::unsupported())
    }

    fn wait(self) -> Result<(), PcuError> {
        Err(PcuError::unsupported())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostedCpuUnsupportedPersistentHandle;

impl PcuPersistentHandle for HostedCpuUnsupportedPersistentHandle {
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
enum HostedCpuStreamState {
    Dormant,
    Active,
    Stopped,
}

#[derive(Debug)]
pub struct HostedCpuStreamHandle {
    value_type: PcuStreamValueType,
    state: HostedCpuStreamState,
    patterns: [PcuStreamPattern; HOSTED_CPU_MAX_STREAM_PATTERNS],
    pattern_len: usize,
    parameter_bindings: [Option<PcuParameterBinding>; HOSTED_CPU_MAX_PARAMETER_BINDINGS],
    parameter_len: usize,
}

impl HostedCpuStreamHandle {
    fn new(value_type: PcuStreamValueType) -> Self {
        Self {
            value_type,
            state: HostedCpuStreamState::Dormant,
            patterns: [PcuStreamPattern::BitReverse; HOSTED_CPU_MAX_STREAM_PATTERNS],
            pattern_len: 0,
            parameter_bindings: [None; HOSTED_CPU_MAX_PARAMETER_BINDINGS],
            parameter_len: 0,
        }
    }

    pub fn process_byte(&mut self, value: u8) -> Result<u8, PcuError> {
        if self.value_type != PcuStreamValueType::U8 {
            return Err(PcuError::invalid());
        }
        let result = self.process_bits(u32::from(value))?;
        u8::try_from(result).map_err(|_| PcuError::invalid())
    }

    pub fn process_half_word(&mut self, value: u16) -> Result<u16, PcuError> {
        if self.value_type != PcuStreamValueType::U16 {
            return Err(PcuError::invalid());
        }
        let result = self.process_bits(u32::from(value))?;
        u16::try_from(result).map_err(|_| PcuError::invalid())
    }

    pub fn process_word(&mut self, value: u32) -> Result<u32, PcuError> {
        if self.value_type != PcuStreamValueType::U32 {
            return Err(PcuError::invalid());
        }
        self.process_bits(value)
    }

    fn process_bits(&mut self, value: u32) -> Result<u32, PcuError> {
        if self.state != HostedCpuStreamState::Active {
            return Err(PcuError::state_conflict());
        }
        let mut current = mask_value(value, self.value_type);
        let mut index = 0;
        while index < self.pattern_len {
            current = apply_pattern(
                current,
                self.patterns[index],
                self.value_type,
                &self.parameter_bindings[..self.parameter_len],
            )?;
            index += 1;
        }
        Ok(mask_value(current, self.value_type))
    }
}

impl PcuPersistentHandle for HostedCpuStreamHandle {
    fn state(&self) -> Result<PcuPersistentState, PcuError> {
        Ok(match self.state {
            HostedCpuStreamState::Dormant => PcuPersistentState::Dormant,
            HostedCpuStreamState::Active => PcuPersistentState::Active,
            HostedCpuStreamState::Stopped => PcuPersistentState::Stopped,
        })
    }

    fn start(&mut self) -> Result<(), PcuError> {
        if self.state == HostedCpuStreamState::Active {
            return Err(PcuError::state_conflict());
        }
        self.state = HostedCpuStreamState::Active;
        Ok(())
    }

    fn stop(&mut self) -> Result<(), PcuError> {
        if self.state != HostedCpuStreamState::Active {
            return Err(PcuError::state_conflict());
        }
        self.state = HostedCpuStreamState::Stopped;
        Ok(())
    }

    fn uninstall(mut self) -> Result<(), PcuError> {
        if self.state == HostedCpuStreamState::Active {
            self.state = HostedCpuStreamState::Stopped;
        }
        Ok(())
    }
}

pub fn install_host_cpu_stream(
    installation: PcuStreamInstallation<'_>,
    bindings: PcuInvocationBindings<'_>,
    parameters: PcuInvocationParameters<'_>,
) -> Result<HostedCpuStreamHandle, PcuError> {
    let kernel = installation.kernel;
    let value_type = kernel
        .validate_simple_transform()
        .map_err(|_| PcuError::invalid())?;
    if !bindings.is_empty() || !kernel.bindings.is_empty() {
        return Err(PcuError::unsupported());
    }
    if !kernel.invocation_parameters_are_valid(parameters) {
        return Err(PcuError::invalid());
    }
    if kernel.patterns.len() > HOSTED_CPU_MAX_STREAM_PATTERNS
        || parameters.bindings.len() > HOSTED_CPU_MAX_PARAMETER_BINDINGS
    {
        return Err(PcuError::resource_exhausted());
    }

    let mut handle = HostedCpuStreamHandle::new(value_type);

    let mut pattern_index = 0;
    while pattern_index < kernel.patterns.len() {
        handle.patterns[pattern_index] = kernel.patterns[pattern_index];
        pattern_index += 1;
    }
    handle.pattern_len = kernel.patterns.len();

    let mut parameter_index = 0;
    while parameter_index < parameters.bindings.len() {
        handle.parameter_bindings[parameter_index] = Some(parameters.bindings[parameter_index]);
        parameter_index += 1;
    }
    handle.parameter_len = parameters.bindings.len();

    Ok(handle)
}

fn apply_pattern(
    value: u32,
    pattern: PcuStreamPattern,
    value_type: PcuStreamValueType,
    parameter_bindings: &[Option<PcuParameterBinding>],
) -> Result<u32, PcuError> {
    let result = match pattern {
        PcuStreamPattern::BitReverse => match value_type {
            PcuStreamValueType::U8 => u32::from((value as u8).reverse_bits()),
            PcuStreamValueType::U16 => u32::from((value as u16).reverse_bits()),
            PcuStreamValueType::U32 => value.reverse_bits(),
        },
        PcuStreamPattern::BitInvert => !value,
        PcuStreamPattern::Increment => match value_type {
            PcuStreamValueType::U8 => u32::from((value as u8).wrapping_add(1)),
            PcuStreamValueType::U16 => u32::from((value as u16).wrapping_add(1)),
            PcuStreamValueType::U32 => value.wrapping_add(1),
        },
        PcuStreamPattern::Decrement => match value_type {
            PcuStreamValueType::U8 => u32::from((value as u8).wrapping_sub(1)),
            PcuStreamValueType::U16 => u32::from((value as u16).wrapping_sub(1)),
            PcuStreamValueType::U32 => value.wrapping_sub(1),
        },
        PcuStreamPattern::AddParameter { parameter } => {
            let operand = lookup_parameter_value(parameter_bindings, parameter, value_type)?;
            match value_type {
                PcuStreamValueType::U8 => u32::from((value as u8).wrapping_add(operand as u8)),
                PcuStreamValueType::U16 => u32::from((value as u16).wrapping_add(operand as u16)),
                PcuStreamValueType::U32 => value.wrapping_add(operand),
            }
        }
        PcuStreamPattern::XorParameter { parameter } => {
            value ^ lookup_parameter_value(parameter_bindings, parameter, value_type)?
        }
        PcuStreamPattern::ShiftLeft { bits } => {
            if bits >= 32 {
                0
            } else {
                value.wrapping_shl(u32::from(bits))
            }
        }
        PcuStreamPattern::ShiftRight { bits } => {
            if bits >= 32 {
                0
            } else {
                value.wrapping_shr(u32::from(bits))
            }
        }
        PcuStreamPattern::ExtractBits { offset, width } => {
            let shifted = if offset >= 32 {
                0
            } else {
                value.wrapping_shr(u32::from(offset))
            };
            shifted & bit_mask(width)
        }
        PcuStreamPattern::MaskLower { bits } => value & bit_mask(bits),
        PcuStreamPattern::ByteSwap32 => match value_type {
            PcuStreamValueType::U32 => value.swap_bytes(),
            _ => return Err(PcuError::invalid()),
        },
    };
    Ok(mask_value(result, value_type))
}

fn lookup_parameter_value(
    parameter_bindings: &[Option<PcuParameterBinding>],
    slot: crate::contract::drivers::pcu::PcuParameterSlot,
    value_type: PcuStreamValueType,
) -> Result<u32, PcuError> {
    let mut index = 0;
    while index < parameter_bindings.len() {
        if let Some(binding) = parameter_bindings[index] {
            if binding.slot == slot {
                return parameter_as_u32(binding.value, value_type).ok_or_else(PcuError::invalid);
            }
        }
        index += 1;
    }
    Err(PcuError::invalid())
}

const fn mask_value(value: u32, value_type: PcuStreamValueType) -> u32 {
    value
        & match value_type {
            PcuStreamValueType::U8 => u8::MAX as u32,
            PcuStreamValueType::U16 => u16::MAX as u32,
            PcuStreamValueType::U32 => u32::MAX,
        }
}

const fn bit_mask(bits: u8) -> u32 {
    if bits >= 32 {
        u32::MAX
    } else if bits == 0 {
        0
    } else {
        (1u32 << bits) - 1
    }
}

fn parameter_as_u32(value: PcuParameterValue, value_type: PcuStreamValueType) -> Option<u32> {
    match value_type {
        PcuStreamValueType::U8 => match value.as_u8() {
            Some(value) => Some(u32::from(value)),
            None => None,
        },
        PcuStreamValueType::U16 => match value.as_u16() {
            Some(value) => Some(u32::from(value)),
            None => None,
        },
        PcuStreamValueType::U32 => value.as_u32(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        HOST_CPU_EXECUTOR_SUPPORT,
        HOSTED_CPU_MAX_STREAM_PATTERNS,
        HostedCpuStreamHandle,
        host_pcu_support,
        install_host_cpu_stream,
    };
    use crate::contract::drivers::pcu::{
        PcuErrorKind,
        PcuKernel,
        PcuParameter,
        PcuParameterBinding,
        PcuParameterSlot,
        PcuParameterValue,
        PcuPersistentHandle,
        PcuStreamInstallation,
    };
    use crate::contract::drivers::pcu::{
        PcuInvocationBindings,
        PcuInvocationParameters,
        PcuPrimitiveCaps,
    };
    use fusion_pcu::model::PcuStreamKernelBuilder;

    fn active_word_handle<const MAX_PATTERNS: usize>(
        builder: PcuStreamKernelBuilder<'static, MAX_PATTERNS>,
        parameters: &[PcuParameterBinding],
    ) -> HostedCpuStreamHandle {
        let kernel = builder.ir();
        let mut handle = install_host_cpu_stream(
            PcuStreamInstallation { kernel: &kernel },
            PcuInvocationBindings::empty(),
            PcuInvocationParameters {
                bindings: parameters,
            },
        )
        .expect("host cpu stream install should succeed");
        handle.start().expect("handle should start");
        handle
    }

    #[test]
    fn hosted_support_reports_direct_stream_only() {
        let support = host_pcu_support();

        assert!(
            support
                .primitive_support
                .supports_direct(PcuPrimitiveCaps::STREAM)
        );
        assert!(
            !support
                .primitive_support
                .supports_direct(PcuPrimitiveCaps::COMMAND)
        );
        assert_eq!(support.executor_count, 1);
        assert!(
            HOST_CPU_EXECUTOR_SUPPORT.supports_kernel_direct(PcuKernel::Stream(
                PcuStreamKernelBuilder::<{ HOSTED_CPU_MAX_STREAM_PATTERNS }>::words(7, "stream")
                    .increment()
                    .expect("builder should accept one pattern")
                    .ir(),
            ))
        );
    }

    #[test]
    fn hosted_stream_processes_u32_patterns() {
        let mut handle = active_word_handle(
            PcuStreamKernelBuilder::<{ HOSTED_CPU_MAX_STREAM_PATTERNS }>::words(9, "stream")
                .increment()
                .expect("builder should accept increment")
                .decrement()
                .expect("builder should accept decrement")
                .shift_left(1)
                .expect("builder should accept left shift")
                .mask_lower(8)
                .expect("builder should accept mask"),
            &[],
        );

        assert_eq!(
            handle.process_word(0x21).expect("word should process"),
            0x42
        );
    }

    #[test]
    fn hosted_stream_processes_parameterized_u16_patterns() {
        const PARAMETER: PcuParameter<'static> = PcuParameter::named(
            PcuParameterSlot(0),
            "delta",
            crate::contract::drivers::pcu::PcuValueType::u16(),
        );
        const PARAMETERS: [PcuParameter<'static>; 1] = [PARAMETER];
        let bindings = [PcuParameterBinding::new(
            PcuParameterSlot(0),
            PcuParameterValue::U16(0x0010),
        )];
        let mut handle = active_word_handle(
            PcuStreamKernelBuilder::<{ HOSTED_CPU_MAX_STREAM_PATTERNS }>::half_words(12, "stream")
                .with_parameters(&PARAMETERS)
                .with_pattern(
                    crate::contract::drivers::pcu::PcuStreamPattern::AddParameter {
                        parameter: PcuParameterSlot(0),
                    },
                )
                .expect("builder should accept add parameter")
                .with_pattern(
                    crate::contract::drivers::pcu::PcuStreamPattern::XorParameter {
                        parameter: PcuParameterSlot(0),
                    },
                )
                .expect("builder should accept xor parameter"),
            &bindings,
        );

        assert_eq!(
            handle
                .process_half_word(0x0020)
                .expect("half word should process"),
            0x0020
        );
    }

    #[test]
    fn hosted_stream_rejects_runtime_bindings() {
        let builder =
            PcuStreamKernelBuilder::<{ HOSTED_CPU_MAX_STREAM_PATTERNS }>::words(13, "stream")
                .increment()
                .expect("builder should accept pattern");
        let kernel = builder.ir();
        let mut output = [0_u32; 1];
        let binding = crate::contract::drivers::pcu::PcuInvocationBinding {
            target: crate::contract::drivers::pcu::PcuInvocationTarget::Port("out"),
            buffer: crate::contract::drivers::pcu::PcuInvocationBuffer::WordsOut(&mut output),
        };

        let error = install_host_cpu_stream(
            PcuStreamInstallation { kernel: &kernel },
            PcuInvocationBindings {
                bindings: &[binding],
            },
            PcuInvocationParameters::empty(),
        )
        .expect_err("runtime bindings should be unsupported");

        assert_eq!(error.kind(), PcuErrorKind::Unsupported);
    }
}
