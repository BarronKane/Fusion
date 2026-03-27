#![allow(clippy::module_inception)]

pub const PAL_LANE_NAME: &str = "soc";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SelectedPalLane;

#[cfg(all(target_os = "none", feature = "sys-cortex-m"))]
#[path = "cortex_m/cortex_m.rs"]
/// Cortex-M SoC implementation family.
pub mod cortex_m;
