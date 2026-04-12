//! PAL-facing PCU contract surface.
//!
//! Canonical pure PCU law now lives in `fusion-pcu`. `fusion-pal` re-exports that law here so
//! existing platform backends and higher layers can keep importing through the PAL contract tree
//! while the ownership inversion settles.

pub use fusion_pcu::contract::*;
pub use fusion_pcu::dispatch::*;

pub mod caps {
    pub use fusion_pcu::contract::caps::*;
}

pub mod device {
    pub use fusion_pcu::contract::device::*;
}

pub mod error {
    pub use fusion_pcu::contract::error::*;
}

pub mod invocation {
    pub use fusion_pcu::contract::invocation::*;
}

pub mod dispatch {
    pub use fusion_pcu::dispatch::*;
}

pub mod ir {
    pub use fusion_pcu::contract::ir::*;
}

pub mod unsupported {
    pub use fusion_pcu::contract::unsupported::*;
}

pub mod protocol;
pub use protocol::*;
