//! Driver-facing PCU invocation shape and typed binding vocabulary.
//!
//! This stays intentionally below orchestration policy. Backend choice, fallback preference, and
//! prepared-dispatch state belong to `fusion-sys`; the contract layer only describes what one PCU
//! kernel invocation looks like in the abstract.

use core::num::NonZeroU32;

use super::PcuKernel;

/// Invocation geometry for one kernel dispatch.
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

/// One abstract kernel invocation descriptor without backend-selection policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuInvocation<'a> {
    pub kernel: &'a PcuKernel<'a>,
    pub shape: PcuInvocationShape,
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

/// Typed invocation bindings for one prepared PCU kernel.
#[derive(Debug)]
pub enum PcuInvocationBindings<'a> {
    StreamBytes(PcuByteStreamBindings<'a>),
    StreamHalfWords(PcuHalfWordStreamBindings<'a>),
    StreamWords(PcuWordStreamBindings<'a>),
}
