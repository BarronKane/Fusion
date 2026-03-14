// SPDX-License-Identifier: GPL-2.0

//! Minimal Rust-for-Linux module entrypoint for `fusion-kn`.
//!
//! This intentionally exposes no user-facing device surface yet. The point is to keep the
//! kernel entry seam real while the Cargo-visible crate hardens the boundary contract.

use kernel::prelude::*;

module! {
    type: FusionKnModule,
    name: "fusion_kn",
    authors: ["Lance Wallis"],
    description: "Fusion kernel boundary blueprint",
    license: "GPL",
}

struct FusionKnModule;

impl kernel::Module for FusionKnModule {
    fn init(_module: &'static ThisModule) -> Result<Self> {
        pr_info!("fusion-kn: module loaded with no public kernel surface\n");
        Ok(Self)
    }
}

impl Drop for FusionKnModule {
    fn drop(&mut self) {
        pr_info!("fusion-kn: module unloaded\n");
    }
}
