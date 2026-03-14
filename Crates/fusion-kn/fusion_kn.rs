// SPDX-License-Identifier: GPL-2.0

//! Future Rust-for-Linux out-of-tree module entrypoint for `fusion-kn`.
//!
//! This file intentionally mirrors the upstream out-of-tree module shape, but it is not yet
//! part of the Cargo build. `fusion-kn` currently compiles as a normal Rust crate while the
//! actual kernel-facing entrypoint remains a blueprint seam for future work.
//!
//! When the Linux kernel integration becomes real, this file is where the eventual
//! `module! { ... }` registration and `kernel::Module` implementation will live.
