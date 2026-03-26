//! Generic fusion-sys wrapper over the selected coprocessor backend.

use fusion_pal::sys::pcu::{
    PcuBase,
    PcuControl,
    PcuDeviceClaim,
    PcuDeviceClass,
    PcuDeviceDescriptor,
    PcuDeviceId,
    PcuError,
    PcuSupport,
    PlatformPcu,
    system_pcu as pal_system_pcu,
};

use super::{
    PcuBackendKind,
    PcuDispatchPlan,
    PcuDispatchPolicy,
    PcuInvocationBindings,
    PcuInvocationDescriptor,
    PcuInvocationHandle,
    PcuPreparedKernel,
    PcuStreamKernelIr,
};

#[cfg(all(target_os = "none", feature = "sys-cortex-m"))]
use super::PcuStreamPattern;

/// fusion-sys wrapper around the selected generic coprocessor backend.
#[derive(Debug, Clone, Copy)]
pub struct PcuSystem {
    inner: PlatformPcu,
}

impl PcuSystem {
    /// Creates a wrapper for the selected generic coprocessor backend.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            inner: pal_system_pcu(),
        }
    }

    /// Reports the truthful generic coprocessor surface for the selected backend.
    #[must_use]
    pub fn support(&self) -> PcuSupport {
        PcuBase::support(&self.inner)
    }

    /// Returns the surfaced generic coprocessor devices.
    #[must_use]
    pub fn devices(&self) -> &'static [PcuDeviceDescriptor] {
        PcuBase::devices(&self.inner)
    }

    /// Claims one surfaced generic coprocessor device.
    ///
    /// # Errors
    ///
    /// Returns any honest backend claim failure.
    pub fn claim_device(&self, device: PcuDeviceId) -> Result<PcuDeviceClaim, PcuError> {
        PcuControl::claim_device(&self.inner, device)
    }

    /// Releases one previously claimed generic coprocessor device.
    ///
    /// # Errors
    ///
    /// Returns any honest backend release failure.
    pub fn release_device(&self, claim: PcuDeviceClaim) -> Result<(), PcuError> {
        PcuControl::release_device(&self.inner, claim)
    }

    fn stream_kernel_supports_pio(&self, kernel: PcuStreamKernelIr<'_>) -> bool {
        #[cfg(all(target_os = "none", feature = "sys-cortex-m"))]
        {
            use crate::pcu::cortex_m::pio::system_pio;

            if !kernel.simple_transform_patterns_are_valid()
                || kernel.simple_transform_type() != Some(super::PcuStreamValueType::U32)
                || kernel.patterns.len() != 1
            {
                return false;
            }
            if !matches!(
                kernel.patterns[0],
                PcuStreamPattern::BitReverse
                    | PcuStreamPattern::BitInvert
                    | PcuStreamPattern::Increment
                    | PcuStreamPattern::ShiftLeft { .. }
                    | PcuStreamPattern::ShiftRight { .. }
                    | PcuStreamPattern::ExtractBits { .. }
                    | PcuStreamPattern::MaskLower { .. }
                    | PcuStreamPattern::ByteSwap32
            ) {
                return false;
            }

            let support = system_pio().support();
            return support.engine_count > 0;
        }
        #[cfg(not(all(target_os = "none", feature = "sys-cortex-m")))]
        {
            let _ = kernel;
            false
        }
    }

    fn select_pio_device(&self) -> Option<PcuDeviceId> {
        self.devices()
            .iter()
            .find(|descriptor| descriptor.class == PcuDeviceClass::Io)
            .map(|descriptor| descriptor.id)
    }

    fn select_backend(
        &self,
        invocation: PcuInvocationDescriptor<'_>,
    ) -> Result<PcuBackendKind, PcuError> {
        let kernel = invocation.kernel;
        if let Some(stream) = kernel.as_stream()
            && !stream.simple_transform_patterns_are_valid()
        {
            return Err(PcuError::invalid());
        }
        let pio_supported = kernel
            .as_stream()
            .is_some_and(|stream| self.stream_kernel_supports_pio(stream));
        let cpu_supported = kernel.as_stream().is_some();

        match invocation.policy {
            PcuDispatchPolicy::CpuOnly => {
                if cpu_supported {
                    Ok(PcuBackendKind::Cpu)
                } else {
                    Err(PcuError::unsupported())
                }
            }
            PcuDispatchPolicy::Require(PcuBackendKind::Cpu) => {
                if cpu_supported {
                    Ok(PcuBackendKind::Cpu)
                } else {
                    Err(PcuError::unsupported())
                }
            }
            PcuDispatchPolicy::Require(PcuBackendKind::CortexMPio) => {
                if pio_supported {
                    Ok(PcuBackendKind::CortexMPio)
                } else {
                    Err(PcuError::unsupported())
                }
            }
            PcuDispatchPolicy::Prefer(PcuBackendKind::Cpu) => {
                if cpu_supported {
                    Ok(PcuBackendKind::Cpu)
                } else {
                    Err(PcuError::unsupported())
                }
            }
            PcuDispatchPolicy::Prefer(PcuBackendKind::CortexMPio) => {
                if pio_supported {
                    Ok(PcuBackendKind::CortexMPio)
                } else {
                    Err(PcuError::unsupported())
                }
            }
            PcuDispatchPolicy::PreferHardwareAllowCpuFallback => {
                if pio_supported {
                    Ok(PcuBackendKind::CortexMPio)
                } else if cpu_supported {
                    Ok(PcuBackendKind::Cpu)
                } else {
                    Err(PcuError::unsupported())
                }
            }
        }
    }

    /// Plans one kernel invocation against the available backends and supplied policy.
    ///
    /// # Errors
    ///
    /// Returns any honest capability or policy-selection failure.
    pub fn plan<'a>(
        &self,
        invocation: PcuInvocationDescriptor<'a>,
    ) -> Result<PcuDispatchPlan<'a>, PcuError> {
        let backend = self.select_backend(invocation)?;
        Ok(PcuDispatchPlan {
            kernel: invocation.kernel,
            shape: invocation.shape,
            backend,
            device: match backend {
                PcuBackendKind::Cpu => None,
                PcuBackendKind::CortexMPio => {
                    Some(self.select_pio_device().ok_or_else(PcuError::unsupported)?)
                }
            },
        })
    }

    /// Prepares one already-planned kernel for execution.
    ///
    /// # Errors
    ///
    /// Returns any honest lowering or preparation failure.
    pub fn prepare<'a>(
        &self,
        plan: PcuDispatchPlan<'a>,
    ) -> Result<PcuPreparedKernel<'a>, PcuError> {
        let _ = self;
        plan.prepare()
    }

    /// Plans, prepares, and dispatches one kernel invocation in one convenience call.
    ///
    /// # Errors
    ///
    /// Returns any honest planning, preparation, or dispatch failure.
    pub fn dispatch(
        &self,
        invocation: PcuInvocationDescriptor<'_>,
        bindings: PcuInvocationBindings<'_>,
    ) -> Result<impl PcuInvocationHandle, PcuError> {
        let prepared = self.prepare(self.plan(invocation)?)?;
        prepared.dispatch(bindings)
    }
}

impl Default for PcuSystem {
    fn default() -> Self {
        Self::new()
    }
}

/// Returns a wrapper for the selected generic coprocessor backend.
#[must_use]
pub const fn system_pcu() -> PcuSystem {
    PcuSystem::new()
}

#[cfg(test)]
mod tests {
    use core::num::NonZeroU32;

    use super::PcuSystem;
    use crate::pcu::{
        PcuByteStreamBindings,
        PcuDispatchPolicy,
        PcuInvocationBindings,
        PcuInvocationDescriptor,
        PcuInvocationHandle,
        PcuInvocationShape,
        PcuKernel,
        PcuKernelId,
        PcuStreamBinding,
        PcuStreamBindingClass,
        PcuStreamCapabilities,
        PcuStreamKernelIr,
        PcuStreamPattern,
        PcuStreamValueType,
        PcuWordStreamBindings,
    };

    #[test]
    fn generic_system_support_matches_device_inventory() {
        let system = PcuSystem::new();
        let support = system.support();
        assert_eq!(usize::from(support.device_count), system.devices().len());
    }

    #[test]
    fn host_stream_planning_falls_back_to_cpu() {
        let bindings = [
            PcuStreamBinding {
                name: Some("input"),
                class: PcuStreamBindingClass::Input,
                value_type: PcuStreamValueType::U32,
            },
            PcuStreamBinding {
                name: Some("output"),
                class: PcuStreamBindingClass::Output,
                value_type: PcuStreamValueType::U32,
            },
        ];
        let patterns = [PcuStreamPattern::BitReverse];
        let kernel = PcuKernel::Stream(PcuStreamKernelIr {
            id: PcuKernelId(41),
            entry_point: "bit_reverse",
            bindings: &bindings,
            patterns: &patterns,
            capabilities: PcuStreamCapabilities::FIFO_INPUT
                | PcuStreamCapabilities::FIFO_OUTPUT
                | PcuStreamCapabilities::BIT_REVERSE,
        });
        let system = PcuSystem::new();
        let plan = system
            .plan(PcuInvocationDescriptor {
                kernel: &kernel,
                shape: PcuInvocationShape::threads(NonZeroU32::new(1).unwrap()),
                policy: PcuDispatchPolicy::PreferHardwareAllowCpuFallback,
            })
            .expect("host fallback planning should succeed");

        assert_eq!(plan.backend(), crate::pcu::PcuBackendKind::Cpu);
        assert_eq!(plan.device(), None);
    }

    #[test]
    fn host_stream_dispatch_executes_cpu_fallback() {
        let bindings = [
            PcuStreamBinding {
                name: Some("input"),
                class: PcuStreamBindingClass::Input,
                value_type: PcuStreamValueType::U32,
            },
            PcuStreamBinding {
                name: Some("output"),
                class: PcuStreamBindingClass::Output,
                value_type: PcuStreamValueType::U32,
            },
        ];
        let patterns = [PcuStreamPattern::BitReverse];
        let kernel = PcuKernel::Stream(PcuStreamKernelIr {
            id: PcuKernelId(42),
            entry_point: "bit_reverse",
            bindings: &bindings,
            patterns: &patterns,
            capabilities: PcuStreamCapabilities::FIFO_INPUT
                | PcuStreamCapabilities::FIFO_OUTPUT
                | PcuStreamCapabilities::BIT_REVERSE,
        });
        let system = PcuSystem::new();
        let input = [0x0000_00f0, 0x8000_0001];
        let mut output = [0u32; 2];
        let handle = system
            .dispatch(
                PcuInvocationDescriptor {
                    kernel: &kernel,
                    shape: PcuInvocationShape::threads(NonZeroU32::new(1).unwrap()),
                    policy: PcuDispatchPolicy::PreferHardwareAllowCpuFallback,
                },
                PcuInvocationBindings::StreamWords(PcuWordStreamBindings {
                    input: &input,
                    output: &mut output,
                }),
            )
            .expect("CPU fallback dispatch should succeed");

        assert_eq!(handle.backend(), crate::pcu::PcuBackendKind::Cpu);
        handle
            .wait()
            .expect("completed CPU fallback should succeed");
        assert_eq!(output, [0x0f00_0000, 0x8000_0001]);
    }

    #[test]
    fn host_byte_stream_dispatch_executes_cpu_fallback() {
        let bindings = [
            PcuStreamBinding {
                name: Some("input"),
                class: PcuStreamBindingClass::Input,
                value_type: PcuStreamValueType::U8,
            },
            PcuStreamBinding {
                name: Some("output"),
                class: PcuStreamBindingClass::Output,
                value_type: PcuStreamValueType::U8,
            },
        ];
        let patterns = [PcuStreamPattern::BitInvert];
        let kernel = PcuKernel::Stream(PcuStreamKernelIr {
            id: PcuKernelId(43),
            entry_point: "bit_invert",
            bindings: &bindings,
            patterns: &patterns,
            capabilities: PcuStreamCapabilities::FIFO_INPUT
                | PcuStreamCapabilities::FIFO_OUTPUT
                | PcuStreamCapabilities::BIT_INVERT,
        });
        let system = PcuSystem::new();
        let input = [0x0fu8, 0xa0];
        let mut output = [0u8; 2];
        let handle = system
            .dispatch(
                PcuInvocationDescriptor {
                    kernel: &kernel,
                    shape: PcuInvocationShape::threads(NonZeroU32::new(2).unwrap()),
                    policy: PcuDispatchPolicy::PreferHardwareAllowCpuFallback,
                },
                PcuInvocationBindings::StreamBytes(PcuByteStreamBindings {
                    input: &input,
                    output: &mut output,
                }),
            )
            .expect("byte CPU fallback dispatch should succeed");

        assert_eq!(handle.backend(), crate::pcu::PcuBackendKind::Cpu);
        handle
            .wait()
            .expect("completed byte CPU fallback should succeed");
        assert_eq!(output, [!0x0f, !0xa0]);
    }

    #[test]
    fn host_stream_planning_rejects_invalid_specialization() {
        let bindings = [
            PcuStreamBinding {
                name: Some("input"),
                class: PcuStreamBindingClass::Input,
                value_type: PcuStreamValueType::U8,
            },
            PcuStreamBinding {
                name: Some("output"),
                class: PcuStreamBindingClass::Output,
                value_type: PcuStreamValueType::U8,
            },
        ];
        let patterns = [PcuStreamPattern::ByteSwap32];
        let kernel = PcuKernel::Stream(PcuStreamKernelIr {
            id: PcuKernelId(44),
            entry_point: "illegal_bswap32_u8",
            bindings: &bindings,
            patterns: &patterns,
            capabilities: PcuStreamCapabilities::FIFO_INPUT
                | PcuStreamCapabilities::FIFO_OUTPUT
                | PcuStreamCapabilities::BYTE_SWAP32,
        });
        let system = PcuSystem::new();
        let error = system
            .plan(PcuInvocationDescriptor {
                kernel: &kernel,
                shape: PcuInvocationShape::threads(NonZeroU32::new(1).unwrap()),
                policy: PcuDispatchPolicy::PreferHardwareAllowCpuFallback,
            })
            .expect_err("invalid pattern/type pairing should be rejected at planning time");

        assert_eq!(error.kind(), crate::pcu::PcuError::invalid().kind());
    }
}
