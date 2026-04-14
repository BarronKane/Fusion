//! AML runtime state overlays for mutable namespace truth.

use core::cell::Cell;

use crate::aml::{
    AmlError,
    AmlNamespaceNodeId,
    AmlResult,
};

/// One mutable integer-name override in runtime AML state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AmlRuntimeIntegerSlot {
    pub node: AmlNamespaceNodeId,
    pub value: u64,
}

/// One runtime mutex slot in AML state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AmlRuntimeMutexSlot {
    pub node: AmlNamespaceNodeId,
    pub held: bool,
}

/// Borrowed mutable AML runtime state overlay.
///
/// The loaded AML namespace stays immutable. Runtime writes flow into this overlay instead.
#[derive(Debug)]
pub struct AmlRuntimeState<'a> {
    integers: &'a [Cell<Option<AmlRuntimeIntegerSlot>>],
    mutexes: &'a [Cell<Option<AmlRuntimeMutexSlot>>],
}

impl<'a> AmlRuntimeState<'a> {
    #[must_use]
    pub const fn new(integers: &'a [Cell<Option<AmlRuntimeIntegerSlot>>]) -> Self {
        Self {
            integers,
            mutexes: &[],
        }
    }

    #[must_use]
    pub const fn with_mutexes(self, mutexes: &'a [Cell<Option<AmlRuntimeMutexSlot>>]) -> Self {
        Self { mutexes, ..self }
    }

    #[must_use]
    pub fn read_integer(&self, node: AmlNamespaceNodeId) -> Option<u64> {
        let mut index = 0_usize;
        while index < self.integers.len() {
            if let Some(slot) = self.integers[index].get() {
                if slot.node == node {
                    return Some(slot.value);
                }
            }
            index += 1;
        }
        None
    }

    pub fn write_integer(&self, node: AmlNamespaceNodeId, value: u64) -> AmlResult<()> {
        let mut empty_index = None;
        let mut index = 0_usize;
        while index < self.integers.len() {
            match self.integers[index].get() {
                Some(slot) if slot.node == node => {
                    self.integers[index].set(Some(AmlRuntimeIntegerSlot { node, value }));
                    return Ok(());
                }
                None if empty_index.is_none() => empty_index = Some(index),
                _ => {}
            }
            index += 1;
        }

        let Some(index) = empty_index else {
            return Err(AmlError::overflow());
        };
        self.integers[index].set(Some(AmlRuntimeIntegerSlot { node, value }));
        Ok(())
    }

    pub fn try_acquire_mutex(&self, node: AmlNamespaceNodeId) -> AmlResult<bool> {
        let mut empty_index = None;
        let mut index = 0_usize;
        while index < self.mutexes.len() {
            match self.mutexes[index].get() {
                Some(slot) if slot.node == node => {
                    if slot.held {
                        return Ok(false);
                    }
                    self.mutexes[index].set(Some(AmlRuntimeMutexSlot { node, held: true }));
                    return Ok(true);
                }
                None if empty_index.is_none() => empty_index = Some(index),
                _ => {}
            }
            index += 1;
        }

        let Some(index) = empty_index else {
            return Err(AmlError::overflow());
        };
        self.mutexes[index].set(Some(AmlRuntimeMutexSlot { node, held: true }));
        Ok(true)
    }

    pub fn release_mutex(&self, node: AmlNamespaceNodeId) -> AmlResult<()> {
        let mut index = 0_usize;
        while index < self.mutexes.len() {
            match self.mutexes[index].get() {
                Some(slot) if slot.node == node => {
                    if !slot.held {
                        return Err(AmlError::invalid_state());
                    }
                    self.mutexes[index].set(Some(AmlRuntimeMutexSlot { node, held: false }));
                    return Ok(());
                }
                _ => {}
            }
            index += 1;
        }
        Err(AmlError::invalid_state())
    }
}
