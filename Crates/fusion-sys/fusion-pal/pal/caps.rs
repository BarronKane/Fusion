//! Shared support-vocabulary types reused across fusion-pal contract domains.
//!
//! These types intentionally model the cross-cutting “how strong is this claim?” and
//! “where does this claim come from?” questions that recur across thread, hardware,
//! context, and event contracts. Domain-specific capability flags still live with their
//! own modules; this file only centralizes the common guarantee vocabulary so future
//! cross-domain composition does not quietly grow five nearly-identical type families.

use bitflags::bitflags;

/// Indicates whether a capability is native, emulated, or unavailable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImplementationKind {
    /// The backend uses native platform support directly.
    Native,
    /// The backend synthesizes the capability from lower-level facilities.
    Emulated,
    /// The backend does not support the capability honestly.
    Unsupported,
}

/// Strength of the guarantee a backend can honestly claim for a capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Guarantee {
    /// The capability is not supported at all.
    Unsupported,
    /// The backend cannot determine the effective guarantee honestly.
    Unknown,
    /// The capability is best-effort or advisory only.
    Advisory,
    /// The capability can be requested and controlled, but the backend cannot prove strict
    /// enforcement under all relevant authorities.
    Controllable,
    /// The backend can enforce the capability across the relevant authorities.
    Enforced,
    /// The backend can both enforce and directly verify the effective state.
    Verified,
}

bitflags! {
    /// Authorities that may contribute evidence to an effective capability record.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct AuthoritySet: u32 {
        /// Operating-system or runtime mechanism truth.
        const OPERATING_SYSTEM = 1 << 0;
        /// ISA or microarchitectural truth.
        const ISA              = 1 << 1;
        /// Machine topology discovery truth.
        const TOPOLOGY         = 1 << 2;
        /// Firmware- or platform-fabric-provided truth.
        const FIRMWARE         = 1 << 3;
        /// Hypervisor or virtual-machine mediation truth.
        const HYPERVISOR       = 1 << 4;
    }
}
