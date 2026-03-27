#[path = "hosted/hosted.rs"]
pub mod hosted;

#[path = "soc/soc.rs"]
pub mod soc;

#[path = "hal/hal.rs"]
pub mod hal;

pub mod cpu;

pub mod selected {
    include!(concat!(env!("OUT_DIR"), "/selected_pal.rs"));
}

pub use selected::SelectedPalLane;
pub const PAL_LANE_NAME: &str = selected::PAL_LANE_NAME;
