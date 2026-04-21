//! Canonical public ACPI driver module crate.

#![cfg_attr(not(feature = "std"), no_std)]

pub mod contract {
    pub mod drivers {
        pub use fusion_hal::contract::drivers::*;
    }
}

#[path = "public.rs"]
pub mod public_impl;

pub mod drivers {
    pub mod acpi {
        pub use crate::public_impl as public;
    }
}

pub use public_impl::*;

#[cfg(any(target_os = "none", feature = "fdxe-module"))]
mod fdxe;
