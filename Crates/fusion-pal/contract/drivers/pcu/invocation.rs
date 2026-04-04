//! Driver-facing PCU invocation shape and typed binding vocabulary.
//!
//! This stays intentionally below orchestration policy. Backend choice, fallback preference, and
//! prepared-dispatch state belong to `fusion-sys`; the contract layer only describes what one PCU
//! kernel invocation looks like in the abstract.

use core::num::NonZeroU32;

use super::{
    PcuKernel,
    PcuParameter,
    PcuParameterBinding,
    PcuParameterSlot,
    PcuParameterValue,
};

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

/// Caller-provided runtime-parameter bindings for one prepared PCU kernel.
#[derive(Debug, Clone, Copy)]
pub struct PcuInvocationParameters<'a> {
    pub bindings: &'a [PcuParameterBinding],
}

impl PcuInvocationParameters<'_> {
    /// Returns one empty runtime-parameter table.
    #[must_use]
    pub const fn empty() -> Self {
        Self { bindings: &[] }
    }

    /// Returns whether no runtime parameters are supplied.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.bindings.is_empty()
    }

    /// Returns one submit-time binding for the requested slot when present.
    #[must_use]
    pub fn binding(self, slot: PcuParameterSlot) -> Option<PcuParameterBinding> {
        self.bindings
            .iter()
            .copied()
            .find(|binding| binding.slot == slot)
    }

    /// Returns one submit-time runtime value for the requested slot when present.
    #[must_use]
    pub fn value(self, slot: PcuParameterSlot) -> Option<PcuParameterValue> {
        self.binding(slot).map(|binding| binding.value)
    }

    /// Returns whether these submit-time runtime parameters satisfy one kernel parameter
    /// declaration list exactly.
    #[must_use]
    pub fn validate_against(self, parameters: &[PcuParameter<'_>]) -> bool {
        for (index, parameter) in parameters.iter().enumerate() {
            if parameters[..index]
                .iter()
                .any(|existing| existing.slot == parameter.slot)
            {
                return false;
            }
            let Some(value) = self.value(parameter.slot) else {
                return false;
            };
            if !value.matches_type(parameter.value_type) {
                return false;
            }
        }

        for (index, binding) in self.bindings.iter().enumerate() {
            if self.bindings[..index]
                .iter()
                .any(|existing| existing.slot == binding.slot)
            {
                return false;
            }
            if !parameters
                .iter()
                .any(|parameter| parameter.slot == binding.slot)
            {
                return false;
            }
        }

        true
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::drivers::pcu::PcuValueType;

    #[test]
    fn invocation_parameters_validate_exact_slot_and_type_matches() {
        let parameters = [PcuParameter::named(
            PcuParameterSlot(0),
            "amount",
            PcuValueType::u32(),
        )];
        let good = PcuInvocationParameters {
            bindings: &[PcuParameterBinding::new(
                PcuParameterSlot(0),
                PcuParameterValue::U32(9),
            )],
        };
        let bad = PcuInvocationParameters {
            bindings: &[PcuParameterBinding::new(
                PcuParameterSlot(0),
                PcuParameterValue::U16(9),
            )],
        };

        assert!(good.validate_against(&parameters));
        assert!(!bad.validate_against(&parameters));
        assert!(PcuInvocationParameters::empty().is_empty());
    }
}
