//! Generic coprocessor invocation, planning, and execution vocabulary.

use core::num::NonZeroU32;

use super::{
    PcuDeviceId,
    PcuError,
    PcuKernel,
    PcuStreamKernelIr,
    PcuStreamPattern,
    PcuStreamValueType,
};

#[cfg(all(target_os = "none", feature = "sys-cortex-m"))]
use super::PcuErrorKind;

#[cfg(all(target_os = "none", feature = "sys-cortex-m"))]
use crate::pcu::cortex_m::pio::{
    PcuIrExecutionConfig,
    PcuIrInstruction,
    PcuLaneId,
    PcuLaneMask,
    PcuProgramId,
    PcuProgramImage,
    bit_invert_stream_transform,
    bit_reverse_stream_transform,
    byte_swap32_stream_transform,
    extract_bits_stream_transform,
    increment_stream_transform,
    mask_lower_stream_transform,
    shift_left_stream_transform,
    shift_right_stream_transform,
    system_pio,
};

/// Compute-style invocation geometry for one kernel dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuInvocationShape {
    threads: NonZeroU32,
}

impl PcuInvocationShape {
    /// Creates one checked invocation shape.
    #[must_use]
    pub const fn threads(threads: NonZeroU32) -> Self {
        Self { threads }
    }

    /// Returns the requested logical thread count.
    #[must_use]
    pub const fn thread_count(self) -> NonZeroU32 {
        self.threads
    }
}

/// Concrete backend kind selected for one prepared or completed invocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuBackendKind {
    Cpu,
    CortexMPio,
}

/// Dispatch policy controlling backend selection and fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuDispatchPolicy {
    CpuOnly,
    Require(PcuBackendKind),
    Prefer(PcuBackendKind),
    PreferHardwareAllowCpuFallback,
}

/// One generic kernel-invocation descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuInvocationDescriptor<'a> {
    pub kernel: &'a PcuKernel<'a>,
    pub shape: PcuInvocationShape,
    pub policy: PcuDispatchPolicy,
}

/// Caller-provided input/output bindings for one `u8` stream transform.
#[derive(Debug)]
pub struct PcuByteStreamBindings<'a> {
    pub input: &'a [u8],
    pub output: &'a mut [u8],
}

/// Caller-provided input/output bindings for one `u16` stream transform.
#[derive(Debug)]
pub struct PcuHalfWordStreamBindings<'a> {
    pub input: &'a [u16],
    pub output: &'a mut [u16],
}

/// Caller-provided input/output bindings for one `u32` stream transform.
#[derive(Debug)]
pub struct PcuWordStreamBindings<'a> {
    pub input: &'a [u32],
    pub output: &'a mut [u32],
}

/// Invocation bindings for one prepared kernel.
#[derive(Debug)]
pub enum PcuInvocationBindings<'a> {
    StreamBytes(PcuByteStreamBindings<'a>),
    StreamHalfWords(PcuHalfWordStreamBindings<'a>),
    StreamWords(PcuWordStreamBindings<'a>),
}

/// Planned dispatch for one kernel invocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuDispatchPlan<'a> {
    pub(crate) kernel: &'a PcuKernel<'a>,
    pub(crate) shape: PcuInvocationShape,
    pub(crate) backend: PcuBackendKind,
    pub(crate) device: Option<PcuDeviceId>,
}

impl PcuDispatchPlan<'_> {
    /// Returns the selected backend.
    #[must_use]
    pub const fn backend(self) -> PcuBackendKind {
        self.backend
    }

    /// Returns the selected generic coprocessor device when one backend-specific claim is needed.
    #[must_use]
    pub const fn device(self) -> Option<PcuDeviceId> {
        self.device
    }
}

/// Prepared CPU fallback kernel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuCpuPreparedKernel<'a> {
    pub(crate) kernel: &'a PcuKernel<'a>,
    pub(crate) shape: PcuInvocationShape,
}

#[cfg(all(target_os = "none", feature = "sys-cortex-m"))]
/// Prepared Cortex-M PIO kernel lowered into one backend-native instruction image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuCortexMPioPreparedKernel<'a> {
    pub(crate) kernel: &'a PcuStreamKernelIr<'a>,
    pub(crate) shape: PcuInvocationShape,
    pub(crate) device_id: PcuDeviceId,
    pub(crate) program_id: PcuProgramId,
    pub(crate) word_count: u8,
    pub(crate) words: [u16; 32],
    pub(crate) execution: PcuIrExecutionConfig,
}

/// Prepared PCU kernel ready for dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuPreparedKernel<'a> {
    Cpu(PcuCpuPreparedKernel<'a>),
    #[cfg(all(target_os = "none", feature = "sys-cortex-m"))]
    CortexMPio(PcuCortexMPioPreparedKernel<'a>),
}

impl PcuPreparedKernel<'_> {
    /// Returns the selected backend.
    #[must_use]
    pub const fn backend(&self) -> PcuBackendKind {
        match self {
            Self::Cpu(_) => PcuBackendKind::Cpu,
            #[cfg(all(target_os = "none", feature = "sys-cortex-m"))]
            Self::CortexMPio(_) => PcuBackendKind::CortexMPio,
        }
    }
}

/// Completion contract for one dispatched PCU invocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuInvocationStatus {
    Pending,
    Complete,
}

/// Completion contract for one dispatched PCU invocation.
pub trait PcuInvocationHandle {
    /// Reports the backend that actually executed this invocation.
    fn backend(&self) -> PcuBackendKind;

    /// Returns the current invocation completion state.
    ///
    /// # Errors
    ///
    /// Returns any honest backend completion-query failure.
    fn status(&self) -> Result<PcuInvocationStatus, PcuError>;

    /// Returns whether the invocation has completed.
    ///
    /// # Errors
    ///
    /// Returns any honest backend completion-query failure.
    fn is_complete(&self) -> Result<bool, PcuError> {
        Ok(matches!(self.status()?, PcuInvocationStatus::Complete))
    }

    /// Waits synchronously for one invocation to finish.
    ///
    /// # Errors
    ///
    /// Returns any honest backend completion failure.
    fn wait(self) -> Result<(), PcuError>;
}

/// One completed invocation handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuCompletedInvocation {
    backend: PcuBackendKind,
    completion: Result<(), PcuError>,
}

impl PcuCompletedInvocation {
    /// Creates one completed invocation result.
    #[must_use]
    pub const fn new(backend: PcuBackendKind, completion: Result<(), PcuError>) -> Self {
        Self {
            backend,
            completion,
        }
    }
}

impl PcuInvocationHandle for PcuCompletedInvocation {
    fn backend(&self) -> PcuBackendKind {
        self.backend
    }

    fn status(&self) -> Result<PcuInvocationStatus, PcuError> {
        Ok(PcuInvocationStatus::Complete)
    }

    fn wait(self) -> Result<(), PcuError> {
        self.completion
    }
}

impl<'a> PcuDispatchPlan<'a> {
    /// Prepares one selected backend for the planned invocation.
    ///
    /// # Errors
    ///
    /// Returns any honest lowering or preparation failure.
    pub fn prepare(self) -> Result<PcuPreparedKernel<'a>, PcuError> {
        match self.backend {
            PcuBackendKind::Cpu => Ok(PcuPreparedKernel::Cpu(PcuCpuPreparedKernel {
                kernel: self.kernel,
                shape: self.shape,
            })),
            PcuBackendKind::CortexMPio => {
                #[cfg(all(target_os = "none", feature = "sys-cortex-m"))]
                {
                    return prepare_cortex_m_pio(self);
                }
                #[cfg(not(all(target_os = "none", feature = "sys-cortex-m")))]
                {
                    Err(PcuError::unsupported())
                }
            }
        }
    }
}

impl PcuPreparedKernel<'_> {
    /// Dispatches one prepared kernel and returns a completion handle.
    ///
    /// # Errors
    ///
    /// Returns any honest dispatch or binding failure.
    pub fn dispatch(
        self,
        bindings: PcuInvocationBindings<'_>,
    ) -> Result<PcuCompletedInvocation, PcuError> {
        let backend = self.backend();
        let completion = match self {
            Self::Cpu(prepared) => dispatch_cpu(prepared, bindings),
            #[cfg(all(target_os = "none", feature = "sys-cortex-m"))]
            Self::CortexMPio(prepared) => dispatch_cortex_m_pio(prepared, bindings),
        }?;
        Ok(PcuCompletedInvocation::new(backend, completion))
    }
}

fn dispatch_cpu(
    prepared: PcuCpuPreparedKernel<'_>,
    bindings: PcuInvocationBindings<'_>,
) -> Result<Result<(), PcuError>, PcuError> {
    let result = match (prepared.kernel, bindings) {
        (PcuKernel::Stream(kernel), bindings) => execute_cpu_stream_kernel(*kernel, bindings),
        _ => Err(PcuError::unsupported()),
    };
    Ok(result)
}

fn execute_cpu_stream_kernel(
    kernel: PcuStreamKernelIr<'_>,
    bindings: PcuInvocationBindings<'_>,
) -> Result<(), PcuError> {
    match (kernel.simple_transform_type(), bindings) {
        (Some(PcuStreamValueType::U8), PcuInvocationBindings::StreamBytes(bindings)) => {
            if bindings.input.len() != bindings.output.len() {
                return Err(PcuError::invalid());
            }
            for (input, output) in bindings.input.iter().zip(bindings.output.iter_mut()) {
                let mut value = *input;
                for pattern in kernel.patterns.iter().copied() {
                    value = apply_stream_pattern_u8(value, pattern)?;
                }
                *output = value;
            }
            Ok(())
        }
        (Some(PcuStreamValueType::U16), PcuInvocationBindings::StreamHalfWords(bindings)) => {
            if bindings.input.len() != bindings.output.len() {
                return Err(PcuError::invalid());
            }
            for (input, output) in bindings.input.iter().zip(bindings.output.iter_mut()) {
                let mut value = *input;
                for pattern in kernel.patterns.iter().copied() {
                    value = apply_stream_pattern_u16(value, pattern)?;
                }
                *output = value;
            }
            Ok(())
        }
        (Some(PcuStreamValueType::U32), PcuInvocationBindings::StreamWords(bindings)) => {
            if bindings.input.len() != bindings.output.len() {
                return Err(PcuError::invalid());
            }
            for (input, output) in bindings.input.iter().zip(bindings.output.iter_mut()) {
                let mut value = *input;
                for pattern in kernel.patterns.iter().copied() {
                    value = apply_stream_pattern_u32(value, pattern)?;
                }
                *output = value;
            }
            Ok(())
        }
        _ => Err(PcuError::invalid()),
    }
}

#[cfg(all(target_os = "none", feature = "sys-cortex-m"))]
fn prepare_cortex_m_pio(plan: PcuDispatchPlan<'_>) -> Result<PcuPreparedKernel<'_>, PcuError> {
    let PcuKernel::Stream(kernel) = plan.kernel else {
        return Err(PcuError::unsupported());
    };
    if kernel.simple_transform_type() != Some(PcuStreamValueType::U32) {
        return Err(PcuError::unsupported());
    }

    let mut instruction_storage = [PcuIrInstruction::Nop; 12];
    let program = match kernel.patterns {
        [PcuStreamPattern::BitReverse] => bit_reverse_stream_transform(
            PcuProgramId(kernel.id.0),
            (&mut instruction_storage[..4])
                .try_into()
                .map_err(|_| PcuError::invalid())?,
        ),
        [PcuStreamPattern::BitInvert] => bit_invert_stream_transform(
            PcuProgramId(kernel.id.0),
            (&mut instruction_storage[..4])
                .try_into()
                .map_err(|_| PcuError::invalid())?,
        ),
        [PcuStreamPattern::Increment] => increment_stream_transform(
            PcuProgramId(kernel.id.0),
            (&mut instruction_storage[..8])
                .try_into()
                .map_err(|_| PcuError::invalid())?,
        ),
        [PcuStreamPattern::ShiftLeft { bits }] => shift_left_stream_transform(
            PcuProgramId(kernel.id.0),
            *bits,
            (&mut instruction_storage[..5])
                .try_into()
                .map_err(|_| PcuError::invalid())?,
        )?,
        [PcuStreamPattern::ShiftRight { bits }] => shift_right_stream_transform(
            PcuProgramId(kernel.id.0),
            *bits,
            (&mut instruction_storage[..5])
                .try_into()
                .map_err(|_| PcuError::invalid())?,
        )?,
        [PcuStreamPattern::ExtractBits { offset, width }] => extract_bits_stream_transform(
            PcuProgramId(kernel.id.0),
            *offset,
            *width,
            (&mut instruction_storage[..6])
                .try_into()
                .map_err(|_| PcuError::invalid())?,
        )?,
        [PcuStreamPattern::MaskLower { bits }] => mask_lower_stream_transform(
            PcuProgramId(kernel.id.0),
            *bits,
            (&mut instruction_storage[..6])
                .try_into()
                .map_err(|_| PcuError::invalid())?,
        )?,
        [PcuStreamPattern::ByteSwap32] => {
            byte_swap32_stream_transform(PcuProgramId(kernel.id.0), &mut instruction_storage)
        }
        _ => return Err(PcuError::unsupported()),
    };

    let pio = system_pio();
    let mut lowering_storage = [0u16; 32];
    let image = pio.lower_program(&program, &mut lowering_storage)?;
    let mut words = [0u16; 32];
    words[..image.words.len()].copy_from_slice(image.words);

    Ok(PcuPreparedKernel::CortexMPio(PcuCortexMPioPreparedKernel {
        kernel,
        shape: plan.shape,
        device_id: plan.device.ok_or_else(PcuError::unsupported)?,
        program_id: image.id,
        word_count: image.words.len() as u8,
        words,
        execution: program.execution,
    }))
}

#[cfg(all(target_os = "none", feature = "sys-cortex-m"))]
fn dispatch_cortex_m_pio(
    prepared: PcuCortexMPioPreparedKernel<'_>,
    bindings: PcuInvocationBindings<'_>,
) -> Result<Result<(), PcuError>, PcuError> {
    const POLL_LIMIT: usize = 1_000_000;

    let PcuInvocationBindings::StreamWords(bindings) = bindings else {
        return Err(PcuError::invalid());
    };
    if prepared.kernel.simple_transform_type() != Some(PcuStreamValueType::U32)
        || bindings.input.len() != bindings.output.len()
    {
        return Err(PcuError::invalid());
    }

    let generic_pcu = crate::pcu::system_pcu();
    let device_claim = generic_pcu.claim_device(prepared.device_id)?;
    let pio = system_pio();
    let engine = pio
        .engines()
        .iter()
        .find(|descriptor| descriptor.lane_count > 0)
        .copied()
        .ok_or_else(|| {
            let _ = generic_pcu.release_device(device_claim);
            PcuError::unsupported()
        })?;
    let lanes_in_use = core::cmp::min(prepared.shape.thread_count().get() as u8, engine.lane_count);
    if lanes_in_use == 0 || lanes_in_use > 8 {
        return Err(PcuError::resource_exhausted());
    }
    let lane_bits = ((1u16 << lanes_in_use) - 1) as u8;
    let lane_mask = PcuLaneMask::new(lane_bits)?;
    let engine_claim = pio.claim_engine(engine.id)?;
    let lane_claim = match pio.claim_lanes(engine.id, lane_mask) {
        Ok(claim) => claim,
        Err(error) => {
            let _ = pio.release_engine(engine_claim);
            let _ = generic_pcu.release_device(device_claim);
            return Err(error);
        }
    };
    let image = PcuProgramImage {
        id: prepared.program_id,
        words: &prepared.words[..usize::from(prepared.word_count)],
    };
    let lease = match pio.load_program(&engine_claim, &image) {
        Ok(lease) => lease,
        Err(error) => {
            let _ = pio.release_lanes(lane_claim);
            let _ = pio.release_engine(engine_claim);
            let _ = generic_pcu.release_device(device_claim);
            return Err(error);
        }
    };

    let dispatch_result = (|| {
        pio.configure_execution(&lane_claim, &prepared.execution)?;
        pio.initialize_lanes(&lane_claim, 0)?;

        let chunk_width = usize::from(lanes_in_use);
        let mut input_chunks = bindings.input.chunks(chunk_width);
        let mut output_chunks = bindings.output.chunks_mut(chunk_width);

        let write_chunk = |input_chunk: &[u32],
                           pio: &crate::pcu::cortex_m::pio::PioSystem|
         -> Result<(), PcuError> {
            for (lane_offset, input) in input_chunk.iter().copied().enumerate() {
                let lane = PcuLaneId {
                    engine: engine.id,
                    index: lane_offset as u8,
                };
                let mut wrote = false;
                for _ in 0..POLL_LIMIT {
                    match pio.write_tx_fifo(&lane_claim, lane, input) {
                        Ok(()) => {
                            wrote = true;
                            break;
                        }
                        Err(error) if error.kind() == PcuErrorKind::Busy => {}
                        Err(error) => return Err(error),
                    }
                }
                if !wrote {
                    return Err(PcuError::busy());
                }
            }
            Ok(())
        };

        let read_chunk = |output_chunk: &mut [u32],
                          pio: &crate::pcu::cortex_m::pio::PioSystem|
         -> Result<(), PcuError> {
            for (lane_offset, output) in output_chunk.iter_mut().enumerate() {
                let lane = PcuLaneId {
                    engine: engine.id,
                    index: lane_offset as u8,
                };
                let mut received = None;
                for _ in 0..POLL_LIMIT {
                    match pio.read_rx_fifo(&lane_claim, lane) {
                        Ok(word) => {
                            received = Some(word);
                            break;
                        }
                        Err(error) if error.kind() == PcuErrorKind::Busy => {}
                        Err(error) => return Err(error),
                    }
                }
                *output = received.ok_or_else(PcuError::busy)?;
            }
            Ok(())
        };

        let Some(first_input_chunk) = input_chunks.next() else {
            return Ok(());
        };
        let Some(first_output_chunk) = output_chunks.next() else {
            return Ok(());
        };

        write_chunk(first_input_chunk, &pio)?;
        pio.start_lanes(&lane_claim)?;
        read_chunk(first_output_chunk, &pio)?;

        for (input_chunk, output_chunk) in input_chunks.zip(output_chunks) {
            write_chunk(input_chunk, &pio)?;
            read_chunk(output_chunk, &pio)?;
        }

        Ok(())
    })();

    let stop_error = pio.stop_lanes(&lane_claim).err();
    let unload_error = pio.unload_program(&engine_claim, lease).err();
    let release_lanes_error = pio.release_lanes(lane_claim).err();
    let release_engine_error = pio.release_engine(engine_claim).err();
    let release_device_error = generic_pcu.release_device(device_claim).err();

    if let Err(error) = dispatch_result {
        return Ok(Err(error));
    }
    if let Some(error) = stop_error
        .or(unload_error)
        .or(release_lanes_error)
        .or(release_engine_error)
        .or(release_device_error)
    {
        return Ok(Err(error));
    }

    Ok(Ok(()))
}

fn apply_stream_pattern_u8(value: u8, pattern: PcuStreamPattern) -> Result<u8, PcuError> {
    Ok(match pattern {
        PcuStreamPattern::BitReverse => value.reverse_bits(),
        PcuStreamPattern::BitInvert => !value,
        PcuStreamPattern::Increment => value.wrapping_add(1),
        PcuStreamPattern::ShiftLeft { bits } => {
            if bits > 8 {
                return Err(PcuError::invalid());
            }
            if bits == 8 { 0 } else { value << bits }
        }
        PcuStreamPattern::ShiftRight { bits } => {
            if bits > 8 {
                return Err(PcuError::invalid());
            }
            if bits == 8 { 0 } else { value >> bits }
        }
        PcuStreamPattern::ExtractBits { offset, width } => {
            extract_bits_u32(u32::from(value), offset, width)? as u8
        }
        PcuStreamPattern::MaskLower { bits } => mask_lower_u32(u32::from(value), bits)? as u8,
        PcuStreamPattern::ByteSwap32 => return Err(PcuError::invalid()),
    })
}

fn apply_stream_pattern_u16(value: u16, pattern: PcuStreamPattern) -> Result<u16, PcuError> {
    Ok(match pattern {
        PcuStreamPattern::BitReverse => value.reverse_bits(),
        PcuStreamPattern::BitInvert => !value,
        PcuStreamPattern::Increment => value.wrapping_add(1),
        PcuStreamPattern::ShiftLeft { bits } => {
            if bits > 16 {
                return Err(PcuError::invalid());
            }
            if bits == 16 { 0 } else { value << bits }
        }
        PcuStreamPattern::ShiftRight { bits } => {
            if bits > 16 {
                return Err(PcuError::invalid());
            }
            if bits == 16 { 0 } else { value >> bits }
        }
        PcuStreamPattern::ExtractBits { offset, width } => {
            extract_bits_u32(u32::from(value), offset, width)? as u16
        }
        PcuStreamPattern::MaskLower { bits } => mask_lower_u32(u32::from(value), bits)? as u16,
        PcuStreamPattern::ByteSwap32 => return Err(PcuError::invalid()),
    })
}

fn apply_stream_pattern_u32(value: u32, pattern: PcuStreamPattern) -> Result<u32, PcuError> {
    Ok(match pattern {
        PcuStreamPattern::BitReverse => value.reverse_bits(),
        PcuStreamPattern::BitInvert => !value,
        PcuStreamPattern::Increment => value.wrapping_add(1),
        PcuStreamPattern::ShiftLeft { bits } => {
            if bits > 32 {
                return Err(PcuError::invalid());
            }
            if bits == 32 { 0 } else { value << bits }
        }
        PcuStreamPattern::ShiftRight { bits } => {
            if bits > 32 {
                return Err(PcuError::invalid());
            }
            if bits == 32 { 0 } else { value >> bits }
        }
        PcuStreamPattern::ExtractBits { offset, width } => extract_bits_u32(value, offset, width)?,
        PcuStreamPattern::MaskLower { bits } => mask_lower_u32(value, bits)?,
        PcuStreamPattern::ByteSwap32 => value.swap_bytes(),
    })
}

fn extract_bits_u32(value: u32, offset: u8, width: u8) -> Result<u32, PcuError> {
    if width == 0 || width > 32 || offset >= 32 || u16::from(offset) + u16::from(width) > 32 {
        return Err(PcuError::invalid());
    }
    Ok((value >> offset) & bit_mask_u32(width))
}

fn mask_lower_u32(value: u32, bits: u8) -> Result<u32, PcuError> {
    if bits == 0 || bits > 32 {
        return Err(PcuError::invalid());
    }
    Ok(value & bit_mask_u32(bits))
}

const fn bit_mask_u32(bits: u8) -> u32 {
    if bits >= 32 {
        u32::MAX
    } else {
        (1u32 << bits) - 1
    }
}

#[cfg(test)]
mod tests {
    use core::num::NonZeroU32;

    use super::{
        PcuBackendKind,
        PcuByteStreamBindings,
        PcuCompletedInvocation,
        PcuHalfWordStreamBindings,
        PcuInvocationBindings,
        PcuInvocationHandle,
        PcuInvocationShape,
        PcuInvocationStatus,
        PcuStreamPattern,
    };

    #[test]
    fn invocation_shape_preserves_thread_count() {
        let shape = PcuInvocationShape::threads(NonZeroU32::new(32).unwrap());
        assert_eq!(shape.thread_count().get(), 32);
    }

    #[test]
    fn completed_invocation_reports_backend_and_completion() {
        let handle = PcuCompletedInvocation::new(PcuBackendKind::Cpu, Ok(()));

        assert_eq!(handle.backend(), PcuBackendKind::Cpu);
        assert_eq!(
            handle.status().expect("completed handle should answer"),
            PcuInvocationStatus::Complete
        );
        assert!(
            handle
                .is_complete()
                .expect("completed handle should answer")
        );
        assert!(handle.wait().is_ok());
    }

    #[test]
    fn cpu_stream_helpers_cover_multiple_word_widths() {
        assert_eq!(
            super::apply_stream_pattern_u8(0b0001_0110, PcuStreamPattern::BitReverse)
                .expect("u8 bit reverse should succeed"),
            0b0110_1000
        );
        assert_eq!(
            super::apply_stream_pattern_u16(0x00f0, PcuStreamPattern::BitInvert)
                .expect("u16 bit invert should succeed"),
            !0x00f0
        );
        assert_eq!(
            super::apply_stream_pattern_u32(u32::MAX, PcuStreamPattern::Increment)
                .expect("u32 increment should wrap"),
            0
        );
        assert_eq!(
            super::apply_stream_pattern_u32(0x0000_0001, PcuStreamPattern::ShiftLeft { bits: 4 })
                .expect("u32 shift should succeed"),
            0x0000_0010
        );
        assert_eq!(
            super::apply_stream_pattern_u32(
                0b1111_0000,
                PcuStreamPattern::ExtractBits {
                    offset: 4,
                    width: 4
                }
            )
            .expect("u32 extract should succeed"),
            0b1111
        );
        assert_eq!(
            super::apply_stream_pattern_u32(0xabcd_1234, PcuStreamPattern::MaskLower { bits: 12 })
                .expect("u32 mask should succeed"),
            0x234
        );
        assert_eq!(
            super::apply_stream_pattern_u32(0x1122_3344, PcuStreamPattern::ByteSwap32)
                .expect("u32 byte swap should succeed"),
            0x4433_2211
        );
        assert_eq!(
            super::apply_stream_pattern_u8(0xff, PcuStreamPattern::ShiftLeft { bits: 8 })
                .expect("full-width u8 shift should zero"),
            0
        );
        assert_eq!(
            super::apply_stream_pattern_u16(0xffff, PcuStreamPattern::ShiftRight { bits: 16 })
                .expect("full-width u16 shift should zero"),
            0
        );
        assert_eq!(
            super::apply_stream_pattern_u32(0xffff_ffff, PcuStreamPattern::ShiftLeft { bits: 32 })
                .expect("full-width u32 shift should zero"),
            0
        );

        let _ = PcuInvocationBindings::StreamBytes(PcuByteStreamBindings {
            input: &[1u8],
            output: &mut [0u8],
        });
        let _ = PcuInvocationBindings::StreamHalfWords(PcuHalfWordStreamBindings {
            input: &[1u16],
            output: &mut [0u16],
        });
    }
}
