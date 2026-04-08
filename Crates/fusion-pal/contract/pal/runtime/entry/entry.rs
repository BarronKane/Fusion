//! PAL-owned process/entry boundary contracts.
//!
//! This module exists so higher layers can talk about target entry honestly without smuggling
//! target runtime crates through random support crates like contraband.

/// Coarse platform entry family selected by the PAL.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EntryKind {
    /// Bare-metal reset/ABI entry owned by Fusion.
    BareMetalReset,
    /// Hosted ambient process entry supplied by the operating system.
    HostedProcess,
}

/// Truthful implementation ownership for the current platform entry boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EntryImplementationKind {
    /// Fusion owns the true entry boundary for this target family.
    FusionOwned,
    /// The ambient host owns the process entry boundary and Fusion composes within it.
    AmbientProcess,
}

/// Static support record for the selected platform entry boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EntrySupport {
    pub kind: EntryKind,
    pub implementation: EntryImplementationKind,
}

impl EntrySupport {
    #[must_use]
    pub const fn fusion_owned_bare_metal() -> Self {
        Self {
            kind: EntryKind::BareMetalReset,
            implementation: EntryImplementationKind::FusionOwned,
        }
    }

    #[must_use]
    pub const fn ambient_hosted() -> Self {
        Self {
            kind: EntryKind::HostedProcess,
            implementation: EntryImplementationKind::AmbientProcess,
        }
    }

    #[must_use]
    pub const fn is_fusion_owned(self) -> bool {
        matches!(self.implementation, EntryImplementationKind::FusionOwned)
    }
}

/// Minimal PAL entry support contract.
pub trait EntryBaseContract {
    fn support(&self) -> EntrySupport;
}
