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

pub const AML_MAX_PACKAGE_ELEMENTS: usize = 16;
pub const AML_MAX_BUFFER_BYTES: usize = 64;

/// Opaque handle for one runtime AML buffer object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AmlRuntimeBufferHandle(pub u16);

/// One runtime value stored inside an aggregate slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AmlRuntimeAggregateValue {
    None,
    Integer(u64),
    Buffer(AmlRuntimeBufferHandle),
}

/// Opaque handle for one runtime AML package object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AmlRuntimePackageHandle(pub u16);

/// One runtime package slot for method-local aggregate objects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AmlRuntimePackageSlot {
    pub handle: AmlRuntimePackageHandle,
    pub len: u8,
    pub elements: [AmlRuntimeAggregateValue; AML_MAX_PACKAGE_ELEMENTS],
}

/// One runtime buffer slot for method-local aggregate objects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AmlRuntimeBufferSlot {
    pub handle: AmlRuntimeBufferHandle,
    pub len: u8,
    pub bytes: [u8; AML_MAX_BUFFER_BYTES],
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
    packages: &'a [Cell<Option<AmlRuntimePackageSlot>>],
    buffers: &'a [Cell<Option<AmlRuntimeBufferSlot>>],
    mutexes: &'a [Cell<Option<AmlRuntimeMutexSlot>>],
}

impl<'a> AmlRuntimeState<'a> {
    #[must_use]
    pub const fn new(integers: &'a [Cell<Option<AmlRuntimeIntegerSlot>>]) -> Self {
        Self {
            integers,
            packages: &[],
            buffers: &[],
            mutexes: &[],
        }
    }

    #[must_use]
    pub const fn with_packages(self, packages: &'a [Cell<Option<AmlRuntimePackageSlot>>]) -> Self {
        Self { packages, ..self }
    }

    #[must_use]
    pub const fn with_buffers(self, buffers: &'a [Cell<Option<AmlRuntimeBufferSlot>>]) -> Self {
        Self { buffers, ..self }
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

    pub fn create_package(&self, len: u8) -> AmlResult<AmlRuntimePackageHandle> {
        if usize::from(len) > AML_MAX_PACKAGE_ELEMENTS {
            return Err(AmlError::overflow());
        }

        let mut index = 0_usize;
        while index < self.packages.len() {
            if self.packages[index].get().is_none() {
                let handle = AmlRuntimePackageHandle(
                    u16::try_from(index).map_err(|_| AmlError::overflow())?,
                );
                self.packages[index].set(Some(AmlRuntimePackageSlot {
                    handle,
                    len,
                    elements: [AmlRuntimeAggregateValue::None; AML_MAX_PACKAGE_ELEMENTS],
                }));
                return Ok(handle);
            }
            index += 1;
        }
        Err(AmlError::overflow())
    }

    pub fn create_buffer(&self, len: u8) -> AmlResult<AmlRuntimeBufferHandle> {
        if usize::from(len) > AML_MAX_BUFFER_BYTES {
            return Err(AmlError::overflow());
        }

        let mut index = 0_usize;
        while index < self.buffers.len() {
            if self.buffers[index].get().is_none() {
                let handle =
                    AmlRuntimeBufferHandle(u16::try_from(index).map_err(|_| AmlError::overflow())?);
                self.buffers[index].set(Some(AmlRuntimeBufferSlot {
                    handle,
                    len,
                    bytes: [0; AML_MAX_BUFFER_BYTES],
                }));
                return Ok(handle);
            }
            index += 1;
        }
        Err(AmlError::overflow())
    }

    #[must_use]
    pub fn read_package_len(&self, handle: AmlRuntimePackageHandle) -> Option<u8> {
        self.package_slot(handle).map(|slot| slot.len)
    }

    #[must_use]
    pub fn read_package_integer(&self, handle: AmlRuntimePackageHandle, index: u8) -> Option<u64> {
        let slot = self.package_slot(handle)?;
        if index >= slot.len {
            return None;
        }
        match slot.elements[usize::from(index)] {
            AmlRuntimeAggregateValue::Integer(value) => Some(value),
            _ => None,
        }
    }

    #[must_use]
    pub fn read_package_value(
        &self,
        handle: AmlRuntimePackageHandle,
        index: u8,
    ) -> Option<AmlRuntimeAggregateValue> {
        let slot = self.package_slot(handle)?;
        if index >= slot.len {
            return None;
        }
        Some(slot.elements[usize::from(index)])
    }

    pub fn write_package_integer(
        &self,
        handle: AmlRuntimePackageHandle,
        index: u8,
        value: u64,
    ) -> AmlResult<()> {
        self.write_package_value(handle, index, AmlRuntimeAggregateValue::Integer(value))
    }

    pub fn write_package_value(
        &self,
        handle: AmlRuntimePackageHandle,
        index: u8,
        value: AmlRuntimeAggregateValue,
    ) -> AmlResult<()> {
        let slot_index = usize::from(handle.0);
        let slot = self
            .packages
            .get(slot_index)
            .and_then(Cell::get)
            .ok_or_else(AmlError::invalid_state)?;
        if index >= slot.len {
            return Err(AmlError::invalid_state());
        }
        let mut updated = slot;
        updated.elements[usize::from(index)] = value;
        self.packages[slot_index].set(Some(updated));
        Ok(())
    }

    #[must_use]
    pub fn read_buffer_len(&self, handle: AmlRuntimeBufferHandle) -> Option<u8> {
        self.buffer_slot(handle).map(|slot| slot.len)
    }

    #[must_use]
    pub fn read_buffer_byte(&self, handle: AmlRuntimeBufferHandle, index: u8) -> Option<u8> {
        let slot = self.buffer_slot(handle)?;
        if index >= slot.len {
            return None;
        }
        Some(slot.bytes[usize::from(index)])
    }

    pub fn write_buffer_byte(
        &self,
        handle: AmlRuntimeBufferHandle,
        index: u8,
        value: u8,
    ) -> AmlResult<()> {
        let slot_index = usize::from(handle.0);
        let slot = self
            .buffers
            .get(slot_index)
            .and_then(Cell::get)
            .ok_or_else(AmlError::invalid_state)?;
        if index >= slot.len {
            return Err(AmlError::invalid_state());
        }
        let mut updated = slot;
        updated.bytes[usize::from(index)] = value;
        self.buffers[slot_index].set(Some(updated));
        Ok(())
    }

    pub fn copy_bytes_into_buffer(
        &self,
        handle: AmlRuntimeBufferHandle,
        bytes: &[u8],
    ) -> AmlResult<()> {
        let slot_index = usize::from(handle.0);
        let slot = self
            .buffers
            .get(slot_index)
            .and_then(Cell::get)
            .ok_or_else(AmlError::invalid_state)?;
        let mut updated = slot;
        updated.bytes = [0; AML_MAX_BUFFER_BYTES];
        let len = usize::from(updated.len);
        let copy_len = core::cmp::min(len, bytes.len());
        updated.bytes[..copy_len].copy_from_slice(&bytes[..copy_len]);
        self.buffers[slot_index].set(Some(updated));
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

    fn package_slot(&self, handle: AmlRuntimePackageHandle) -> Option<AmlRuntimePackageSlot> {
        self.packages.get(usize::from(handle.0)).and_then(Cell::get)
    }

    fn buffer_slot(&self, handle: AmlRuntimeBufferHandle) -> Option<AmlRuntimeBufferSlot> {
        self.buffers.get(usize::from(handle.0)).and_then(Cell::get)
    }
}
