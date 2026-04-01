#![cfg_attr(target_os = "none", no_std)]

//! fusion-kernel library surfaces.
//!
//! This crate is the policy/enforcement consumer over the composable claims, courier, channel,
//! and inspection primitives exposed by `fusion-pal`, `fusion-sys`, and `fusion-std`.

pub mod claims;
