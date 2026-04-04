//! Dynamic bare-metal hardware-enumeration lane.
//!
//! This lane is reserved for machines where Fusion must discover reachable hardware and
//! firmware-described devices at runtime rather than consuming a closed-world SoC composition.

#![allow(clippy::module_inception)]

pub const PAL_LANE_NAME: &str = "hal";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SelectedPalLane;

#[path = "acpi/acpi.rs"]
pub mod acpi;
pub mod devicetree;
pub mod hardware;
pub mod runtime;
