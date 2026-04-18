//! AML runtime value vocabulary.

use crate::aml::{
    AmlCodeLocation,
    AmlError,
    AmlResult,
    AmlRuntimeBufferHandle,
    AmlRuntimePackageHandle,
};

/// Effective AML integer width for one namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AmlIntegerWidth {
    Bits32,
    Bits64,
}

impl AmlIntegerWidth {
    #[must_use]
    pub const fn from_definition_block_revision(revision: u8) -> Self {
        if revision < 2 {
            Self::Bits32
        } else {
            Self::Bits64
        }
    }
}

/// Borrowed AML runtime value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AmlValue<'a> {
    Integer(u64),
    String(&'a str),
    StaticString(AmlCodeLocation),
    Buffer(&'a [u8]),
    BufferHandle(AmlRuntimeBufferHandle),
    Package(&'a [AmlValue<'a>]),
    StaticPackage(AmlCodeLocation),
    PackageHandle(AmlRuntimePackageHandle),
    DebugObject,
    None,
}

impl<'a> AmlValue<'a> {
    #[must_use]
    pub const fn integer(value: u64, width: AmlIntegerWidth) -> Self {
        match width {
            AmlIntegerWidth::Bits32 => Self::Integer((value as u32) as u64),
            AmlIntegerWidth::Bits64 => Self::Integer(value),
        }
    }

    pub fn as_integer(self) -> AmlResult<u64> {
        match self {
            Self::Integer(value) => Ok(value),
            _ => Err(AmlError::unsupported()),
        }
    }

    pub fn as_package_handle(self) -> AmlResult<AmlRuntimePackageHandle> {
        match self {
            Self::PackageHandle(handle) => Ok(handle),
            _ => Err(AmlError::unsupported()),
        }
    }

    pub fn as_buffer_handle(self) -> AmlResult<AmlRuntimeBufferHandle> {
        match self {
            Self::BufferHandle(handle) => Ok(handle),
            _ => Err(AmlError::unsupported()),
        }
    }

    #[must_use]
    pub fn as_logic(self) -> bool {
        match self {
            Self::Integer(value) => value != 0,
            Self::None => false,
            Self::DebugObject => true,
            Self::String(value) => !value.is_empty(),
            Self::StaticString(_) => true,
            Self::Buffer(value) => !value.is_empty(),
            Self::BufferHandle(_) => true,
            Self::Package(value) => !value.is_empty(),
            Self::StaticPackage(_) => true,
            Self::PackageHandle(_) => true,
        }
    }
}
