use std::env;
use std::fs;
use std::path::PathBuf;

fn feature_enabled(name: &str) -> bool {
    env::var_os(name).is_some()
}

fn selected_lane() -> &'static str {
    let soc = feature_enabled("CARGO_FEATURE_SOC");
    let hosted = feature_enabled("CARGO_FEATURE_HOSTED");
    let hal = feature_enabled("CARGO_FEATURE_HAL");

    if soc && hal {
        panic!(
            "fusion-pal requires at most one explicit hardware PAL lane; `soc` and `hal` are mutually exclusive"
        );
    }

    // `hosted` is the soft default for ordinary root-workspace consumers. When a more specific
    // hardware lane is selected explicitly, let it win instead of turning default convenience into
    // a fake conflict.
    if soc {
        return "soc";
    }

    if hal {
        return "hal";
    }

    if hosted {
        return "hosted";
    }

    if feature_enabled("CARGO_FEATURE_SYS_CORTEX_M") || feature_enabled("CARGO_FEATURE_SOC_RP2350")
    {
        return "soc";
    }

    if feature_enabled("CARGO_FEATURE_SYS_FUSION_KN") {
        return "hosted";
    }

    match env::var("CARGO_CFG_TARGET_OS").as_deref() {
        Ok("linux" | "macos" | "windows" | "ios") => "hosted",
        Ok("none") => "hal",
        Ok(other) => panic!(
            "fusion-pal could not infer PAL lane for target_os={other:?}; enable one of `soc`, `hosted`, or `hal`"
        ),
        Err(_) => panic!(
            "fusion-pal could not infer PAL lane because CARGO_CFG_TARGET_OS was unavailable"
        ),
    }
}

fn selected_pal_glue(lane: &str) -> String {
    match lane {
        "soc" => "pub use super::soc::SelectedPalLane;\n\
                  pub const PAL_LANE_NAME: &str = super::soc::PAL_LANE_NAME;\n\
                  #[cfg(all(target_os = \"none\", feature = \"sys-cortex-m\"))]\n\
                  pub use crate::pal::soc::cortex_m as platform;\n\
                  #[cfg(all(not(target_os = \"none\"), target_os = \"linux\"))]\n\
                  pub use crate::pal::hosted::linux as platform;\n\
                  #[cfg(target_os = \"macos\")]\n\
                  pub use crate::pal::hosted::macos as platform;\n\
                  #[cfg(target_os = \"windows\")]\n\
                  pub use crate::pal::hosted::windows as platform;\n\
                  #[cfg(target_os = \"ios\")]\n\
                  pub use crate::pal::hosted::ios as platform;\n"
            .to_owned(),
        "hosted" => {
            if feature_enabled("CARGO_FEATURE_SYS_FUSION_KN") {
                return "pub use super::hosted::SelectedPalLane;\n\
                        pub const PAL_LANE_NAME: &str = super::hosted::PAL_LANE_NAME;\n\
                        pub use crate::pal::hosted::fusion_kn as platform;\n"
                    .to_owned();
            }

            match env::var("CARGO_CFG_TARGET_OS").as_deref() {
                Ok("ios") => "pub use super::hosted::SelectedPalLane;\n\
                              pub const PAL_LANE_NAME: &str = super::hosted::PAL_LANE_NAME;\n\
                              pub use crate::pal::hosted::ios as platform;\n"
                    .to_owned(),
                Ok("linux") => "pub use super::hosted::SelectedPalLane;\n\
                                pub const PAL_LANE_NAME: &str = super::hosted::PAL_LANE_NAME;\n\
                                pub use crate::pal::hosted::linux as platform;\n"
                    .to_owned(),
                Ok("macos") => "pub use super::hosted::SelectedPalLane;\n\
                                pub const PAL_LANE_NAME: &str = super::hosted::PAL_LANE_NAME;\n\
                                pub use crate::pal::hosted::macos as platform;\n"
                    .to_owned(),
                Ok("windows") => "pub use super::hosted::SelectedPalLane;\n\
                                  pub const PAL_LANE_NAME: &str = super::hosted::PAL_LANE_NAME;\n\
                                  pub use crate::pal::hosted::windows as platform;\n"
                    .to_owned(),
                Ok(other) => panic!(
                    "fusion-pal could not select hosted PAL platform glue for target_os={other:?}"
                ),
                Err(_) => panic!(
                    "fusion-pal could not select hosted PAL platform glue because CARGO_CFG_TARGET_OS was unavailable"
                ),
            }
        }
        "hal" => "pub use super::hal::SelectedPalLane;\n\
                  pub const PAL_LANE_NAME: &str = super::hal::PAL_LANE_NAME;\n\
                  pub use crate::pal::hal as platform;\n"
            .to_owned(),
        other => panic!("unsupported fusion-pal lane selection {other:?}"),
    }
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_SOC");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_HOSTED");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_HAL");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_SYS_CORTEX_M");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_SOC_RP2350");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_SYS_FUSION_KN");
    println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_OS");

    let lane = selected_lane();
    let selected_rs = selected_pal_glue(lane);

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("cargo should provide OUT_DIR"));
    fs::write(out_dir.join("selected_pal.rs"), selected_rs)
        .expect("fusion-pal build should emit selected PAL glue");
}
