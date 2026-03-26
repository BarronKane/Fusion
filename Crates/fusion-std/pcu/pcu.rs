//! Public coprocessor sugar layered over `fusion-sys::pcu`.
//!
//! `fusion-std::pcu` stays honest about the layering boundary:
//! `fusion-sys` owns the real IR, planning, and backend dispatch substrate; this module just
//! makes the common stream-kernel path less syntactically hostile.

mod stream;

use core::num::NonZeroU32;
pub use fusion_std_pcu_macros::PCU;
pub use stream::*;

pub use fusion_sys::pcu::{
    PcuBackendKind,
    PcuByteStreamBindings,
    PcuCompletedInvocation,
    PcuComputeBinding,
    PcuComputeBuiltin,
    PcuComputeCapabilities,
    PcuComputeEntryPoint,
    PcuComputeKernelIr,
    PcuComputeScalarType,
    PcuComputeShaderIr,
    PcuComputeStorageClass,
    PcuComputeValueType,
    PcuDeviceClaim,
    PcuDeviceClass,
    PcuDeviceDescriptor,
    PcuDeviceId,
    PcuDispatchPlan,
    PcuDispatchPolicy,
    PcuError,
    PcuErrorKind,
    PcuHalfWordStreamBindings,
    PcuInvocationBindings,
    PcuInvocationDescriptor,
    PcuInvocationHandle,
    PcuInvocationShape,
    PcuInvocationStatus,
    PcuIrKind,
    PcuKernel,
    PcuKernelId,
    PcuKernelIr,
    PcuStreamBinding,
    PcuStreamBindingClass,
    PcuStreamCapabilities,
    PcuStreamKernelIr,
    PcuStreamOp,
    PcuStreamPattern,
    PcuStreamValueType,
    PcuSupport,
    PcuWordStreamBindings,
};

/// Reusable dispatch profile for one family of PCU submissions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuDispatchProfile {
    threads: NonZeroU32,
    policy: PcuDispatchPolicy,
}

impl PcuDispatchProfile {
    /// Creates the default dispatch profile.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            threads: match NonZeroU32::new(1) {
                Some(value) => value,
                None => unreachable!(),
            },
            policy: PcuDispatchPolicy::PreferHardwareAllowCpuFallback,
        }
    }

    /// Returns the requested logical thread count.
    #[must_use]
    pub const fn thread_count(self) -> NonZeroU32 {
        self.threads
    }

    /// Returns the selected dispatch policy.
    #[must_use]
    pub const fn policy(self) -> PcuDispatchPolicy {
        self.policy
    }

    /// Replaces the requested logical thread count with one checked scalar value.
    ///
    /// # Errors
    ///
    /// Returns `Invalid` when `threads == 0`.
    pub fn with_thread_count(mut self, threads: u32) -> Result<Self, PcuError> {
        self.threads = NonZeroU32::new(threads).ok_or_else(PcuError::invalid)?;
        Ok(self)
    }

    /// Replaces the requested logical thread count with one already validated non-zero value.
    #[must_use]
    pub const fn threads(mut self, threads: NonZeroU32) -> Self {
        self.threads = threads;
        self
    }

    /// Replaces the dispatch policy.
    #[must_use]
    pub const fn with_policy(mut self, policy: PcuDispatchPolicy) -> Self {
        self.policy = policy;
        self
    }

    /// Forces CPU fallback execution only.
    #[must_use]
    pub const fn cpu_only(self) -> Self {
        self.with_policy(PcuDispatchPolicy::CpuOnly)
    }

    /// Requires one specific backend.
    #[must_use]
    pub const fn require_backend(self, backend: PcuBackendKind) -> Self {
        self.with_policy(PcuDispatchPolicy::Require(backend))
    }

    /// Requires Cortex-M PIO execution.
    #[must_use]
    pub const fn require_pio(self) -> Self {
        self.require_backend(PcuBackendKind::CortexMPio)
    }

    /// Prefers one specific backend.
    #[must_use]
    pub const fn prefer_backend(self, backend: PcuBackendKind) -> Self {
        self.with_policy(PcuDispatchPolicy::Prefer(backend))
    }

    /// Prefers hardware execution and allows CPU fallback.
    #[must_use]
    pub const fn prefer_hardware(self) -> Self {
        self.with_policy(PcuDispatchPolicy::PreferHardwareAllowCpuFallback)
    }
}

impl Default for PcuDispatchProfile {
    fn default() -> Self {
        Self::new()
    }
}

/// `fusion-std` PCU facade plus one reusable dispatch profile.
#[derive(Debug, Clone, Copy)]
pub struct ProfiledPcu<'a> {
    system: &'a Pcu,
    profile: PcuDispatchProfile,
}

/// `fusion-std` facade for the selected generic coprocessor backend.
#[derive(Debug, Clone, Copy, Default)]
pub struct Pcu {
    inner: fusion_sys::pcu::PcuSystem,
}

impl Pcu {
    /// Creates one `fusion-std` wrapper around the selected generic coprocessor backend.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            inner: fusion_sys::pcu::PcuSystem::new(),
        }
    }

    /// Returns the truthful generic coprocessor support surface.
    #[must_use]
    pub fn support(&self) -> PcuSupport {
        self.inner.support()
    }

    /// Returns the surfaced generic coprocessor devices.
    #[must_use]
    pub fn devices(&self) -> &'static [PcuDeviceDescriptor] {
        self.inner.devices()
    }

    /// Claims one surfaced generic coprocessor device.
    ///
    /// # Errors
    ///
    /// Returns any honest backend claim failure.
    pub fn claim_device(&self, device: PcuDeviceId) -> Result<PcuDeviceClaim, PcuError> {
        self.inner.claim_device(device)
    }

    /// Releases one previously claimed generic coprocessor device.
    ///
    /// # Errors
    ///
    /// Returns any honest backend release failure.
    pub fn release_device(&self, claim: PcuDeviceClaim) -> Result<(), PcuError> {
        self.inner.release_device(claim)
    }

    /// Returns the raw `fusion-sys` PCU wrapper when the caller needs the substrate directly.
    #[must_use]
    pub const fn raw(&self) -> &fusion_sys::pcu::PcuSystem {
        &self.inner
    }

    /// Returns one profiled view over this PCU facade.
    #[must_use]
    pub const fn with_profile(&self, profile: PcuDispatchProfile) -> ProfiledPcu<'_> {
        ProfiledPcu {
            system: self,
            profile,
        }
    }

    /// Returns one default dispatch profile over this PCU facade.
    #[must_use]
    pub const fn profile(&self) -> ProfiledPcu<'_> {
        self.with_profile(PcuDispatchProfile::new())
    }

    /// Returns one single-lane PIO-profiled view over this PCU facade.
    #[must_use]
    pub const fn pio(&self) -> ProfiledPcu<'_> {
        self.with_profile(PcuDispatchProfile::new().require_pio())
    }

    /// Returns one CPU-only profiled view over this PCU facade.
    #[must_use]
    pub const fn cpu(&self) -> ProfiledPcu<'_> {
        self.with_profile(PcuDispatchProfile::new().cpu_only())
    }

    /// Returns one PIO-profiled view with an explicit thread count.
    ///
    /// # Errors
    ///
    /// Returns `Invalid` when `threads == 0`.
    pub fn pio_threads(&self, threads: u32) -> Result<ProfiledPcu<'_>, PcuError> {
        Ok(self.with_profile(
            PcuDispatchProfile::new()
                .with_thread_count(threads)?
                .require_pio(),
        ))
    }

    /// Returns one CPU-only profiled view with an explicit thread count.
    ///
    /// # Errors
    ///
    /// Returns `Invalid` when `threads == 0`.
    pub fn cpu_threads(&self, threads: u32) -> Result<ProfiledPcu<'_>, PcuError> {
        Ok(self.with_profile(
            PcuDispatchProfile::new()
                .with_thread_count(threads)?
                .cpu_only(),
        ))
    }

    /// Starts one byte-stream transform builder.
    #[must_use]
    pub fn stream_bytes<'a>(
        &'a self,
        kernel_id: u32,
        entry_point: &'a str,
    ) -> PcuStreamDispatchBuilder<'a> {
        PcuStreamDispatchBuilder::new(
            self,
            PcuKernelId(kernel_id),
            entry_point,
            PcuStreamValueType::U8,
        )
    }

    /// Starts one byte-stream transform builder.
    #[must_use]
    pub fn bytes<'a>(
        &'a self,
        kernel_id: u32,
        entry_point: &'a str,
    ) -> PcuStreamDispatchBuilder<'a> {
        self.stream_bytes(kernel_id, entry_point)
    }

    /// Starts one half-word stream transform builder.
    #[must_use]
    pub fn stream_half_words<'a>(
        &'a self,
        kernel_id: u32,
        entry_point: &'a str,
    ) -> PcuStreamDispatchBuilder<'a> {
        PcuStreamDispatchBuilder::new(
            self,
            PcuKernelId(kernel_id),
            entry_point,
            PcuStreamValueType::U16,
        )
    }

    /// Starts one half-word stream transform builder.
    #[must_use]
    pub fn half_words<'a>(
        &'a self,
        kernel_id: u32,
        entry_point: &'a str,
    ) -> PcuStreamDispatchBuilder<'a> {
        self.stream_half_words(kernel_id, entry_point)
    }

    /// Starts one word-stream transform builder.
    #[must_use]
    pub fn stream_words<'a>(
        &'a self,
        kernel_id: u32,
        entry_point: &'a str,
    ) -> PcuStreamDispatchBuilder<'a> {
        PcuStreamDispatchBuilder::new(
            self,
            PcuKernelId(kernel_id),
            entry_point,
            PcuStreamValueType::U32,
        )
    }

    /// Starts one word-stream transform builder.
    #[must_use]
    pub fn words<'a>(
        &'a self,
        kernel_id: u32,
        entry_point: &'a str,
    ) -> PcuStreamDispatchBuilder<'a> {
        self.stream_words(kernel_id, entry_point)
    }
}

impl<'a> ProfiledPcu<'a> {
    /// Returns the dispatch profile carried by this profiled facade.
    #[must_use]
    pub const fn settings(&self) -> PcuDispatchProfile {
        self.profile
    }

    /// Replaces the requested logical thread count carried by this profiled facade.
    ///
    /// # Errors
    ///
    /// Returns `Invalid` when `threads == 0`.
    pub fn with_thread_count(self, threads: u32) -> Result<Self, PcuError> {
        Ok(Self {
            system: self.system,
            profile: self.profile.with_thread_count(threads)?,
        })
    }

    /// Replaces the dispatch policy carried by this profiled facade.
    #[must_use]
    pub const fn with_policy(self, policy: PcuDispatchPolicy) -> Self {
        Self {
            system: self.system,
            profile: self.profile.with_policy(policy),
        }
    }

    /// Forces CPU fallback execution for builders created through this profiled facade.
    #[must_use]
    pub const fn cpu_only(self) -> Self {
        self.with_policy(PcuDispatchPolicy::CpuOnly)
    }

    /// Requires one specific backend for builders created through this profiled facade.
    #[must_use]
    pub const fn require_backend(self, backend: PcuBackendKind) -> Self {
        self.with_policy(PcuDispatchPolicy::Require(backend))
    }

    /// Requires Cortex-M PIO execution for builders created through this profiled facade.
    #[must_use]
    pub const fn require_pio(self) -> Self {
        self.require_backend(PcuBackendKind::CortexMPio)
    }

    /// Starts one byte-stream transform builder with the profiled dispatch settings already
    /// applied.
    #[must_use]
    pub fn stream_bytes(
        &self,
        kernel_id: u32,
        entry_point: &'a str,
    ) -> PcuStreamDispatchBuilder<'a> {
        self.system
            .stream_bytes(kernel_id, entry_point)
            .threads(self.profile.thread_count())
            .with_policy(self.profile.policy())
    }

    /// Starts one byte-stream transform builder with the profiled dispatch settings already
    /// applied.
    #[must_use]
    pub fn bytes(&self, kernel_id: u32, entry_point: &'a str) -> PcuStreamDispatchBuilder<'a> {
        self.stream_bytes(kernel_id, entry_point)
    }

    /// Starts one half-word stream transform builder with the profiled dispatch settings already
    /// applied.
    #[must_use]
    pub fn stream_half_words(
        &self,
        kernel_id: u32,
        entry_point: &'a str,
    ) -> PcuStreamDispatchBuilder<'a> {
        self.system
            .stream_half_words(kernel_id, entry_point)
            .threads(self.profile.thread_count())
            .with_policy(self.profile.policy())
    }

    /// Starts one half-word stream transform builder with the profiled dispatch settings already
    /// applied.
    #[must_use]
    pub fn half_words(&self, kernel_id: u32, entry_point: &'a str) -> PcuStreamDispatchBuilder<'a> {
        self.stream_half_words(kernel_id, entry_point)
    }

    /// Starts one word-stream transform builder with the profiled dispatch settings already
    /// applied.
    #[must_use]
    pub fn stream_words(
        &self,
        kernel_id: u32,
        entry_point: &'a str,
    ) -> PcuStreamDispatchBuilder<'a> {
        self.system
            .stream_words(kernel_id, entry_point)
            .threads(self.profile.thread_count())
            .with_policy(self.profile.policy())
    }

    /// Starts one word-stream transform builder with the profiled dispatch settings already
    /// applied.
    #[must_use]
    pub fn words(&self, kernel_id: u32, entry_point: &'a str) -> PcuStreamDispatchBuilder<'a> {
        self.stream_words(kernel_id, entry_point)
    }
}

/// Returns the public `fusion-std` PCU facade for the selected backend.
#[must_use]
pub const fn system_pcu() -> Pcu {
    Pcu::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatch_profile_applies_threads_and_policy_to_stream_builder() {
        let profile = PcuDispatchProfile::new()
            .with_thread_count(4)
            .expect("non-zero thread count should be valid")
            .require_pio();
        let system = Pcu::new();
        let builder = system
            .with_profile(profile)
            .stream_words(0x210, "increment");

        assert_eq!(builder.thread_count().get(), 4);
        assert_eq!(
            builder.policy(),
            PcuDispatchPolicy::Require(PcuBackendKind::CortexMPio)
        );
    }

    #[test]
    fn pio_helper_applies_single_lane_pio_defaults() {
        let system = Pcu::new();
        let builder = system.pio().words(0x301, "increment");

        assert_eq!(builder.thread_count().get(), 1);
        assert_eq!(
            builder.policy(),
            PcuDispatchPolicy::Require(PcuBackendKind::CortexMPio)
        );
    }

    #[test]
    fn pio_threads_helper_applies_requested_threads() {
        let system = Pcu::new();
        let builder = system
            .pio_threads(4)
            .expect("non-zero thread count should be valid")
            .words(0x302, "bit_reverse");

        assert_eq!(builder.thread_count().get(), 4);
        assert_eq!(
            builder.policy(),
            PcuDispatchPolicy::Require(PcuBackendKind::CortexMPio)
        );
    }
}
