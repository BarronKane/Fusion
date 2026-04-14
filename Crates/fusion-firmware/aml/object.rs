//! AML namespace object vocabulary.

use crate::aml::{
    AmlFieldDescriptor,
    AmlMethodDescriptor,
    AmlMutexDescriptor,
    AmlNamespaceNodeId,
    AmlOpRegionDescriptor,
    AmlValue,
};

/// One loaded AML object class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AmlObjectKind {
    Scope,
    Device,
    Method,
    Name,
    OpRegion,
    Field,
    BufferField,
    Mutex,
    Event,
    Processor,
    ThermalZone,
    PowerResource,
    Alias,
    External,
}

/// Borrowed AML namespace object payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AmlObject<'a> {
    Scope,
    Device,
    Method(AmlMethodDescriptor),
    Name(AmlValue<'a>),
    OpRegion(AmlOpRegionDescriptor),
    Field(AmlFieldDescriptor),
    BufferField,
    Mutex(AmlMutexDescriptor),
    Event,
    Processor,
    ThermalZone,
    PowerResource,
    Alias(AmlNamespaceNodeId),
    External,
}
