//! AML parse/load/evaluation error vocabulary.

/// AML-specific error classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AmlErrorKind {
    Truncated,
    InvalidBytecode,
    InvalidDefinitionBlock,
    InvalidName,
    InvalidNamespace,
    InvalidState,
    NamespaceConflict,
    UndefinedObject,
    Unsupported,
    HostFailure,
    Overflow,
}

/// Minimal AML error payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AmlError {
    pub kind: AmlErrorKind,
    pub detail: &'static str,
}

impl AmlError {
    #[must_use]
    pub const fn new(kind: AmlErrorKind, detail: &'static str) -> Self {
        Self { kind, detail }
    }

    #[must_use]
    pub const fn truncated() -> Self {
        Self::new(AmlErrorKind::Truncated, "aml input truncated")
    }

    #[must_use]
    pub const fn invalid_bytecode() -> Self {
        Self::new(AmlErrorKind::InvalidBytecode, "invalid aml bytecode")
    }

    #[must_use]
    pub const fn invalid_definition_block() -> Self {
        Self::new(
            AmlErrorKind::InvalidDefinitionBlock,
            "invalid aml definition block",
        )
    }

    #[must_use]
    pub const fn invalid_name() -> Self {
        Self::new(AmlErrorKind::InvalidName, "invalid aml name")
    }

    #[must_use]
    pub const fn invalid_namespace() -> Self {
        Self::new(
            AmlErrorKind::InvalidNamespace,
            "invalid aml namespace state",
        )
    }

    #[must_use]
    pub const fn invalid_state() -> Self {
        Self::new(AmlErrorKind::InvalidState, "invalid aml vm state")
    }

    #[must_use]
    pub const fn namespace_conflict() -> Self {
        Self::new(AmlErrorKind::NamespaceConflict, "aml namespace conflict")
    }

    #[must_use]
    pub const fn undefined_object() -> Self {
        Self::new(AmlErrorKind::UndefinedObject, "aml object not found")
    }

    #[must_use]
    pub const fn unsupported() -> Self {
        Self::new(AmlErrorKind::Unsupported, "aml feature unsupported")
    }

    #[must_use]
    pub const fn host_failure() -> Self {
        Self::new(AmlErrorKind::HostFailure, "aml host interaction failed")
    }

    #[must_use]
    pub const fn overflow() -> Self {
        Self::new(AmlErrorKind::Overflow, "aml integer or buffer overflow")
    }
}

/// Result alias for AML surfaces.
pub type AmlResult<T> = Result<T, AmlError>;
