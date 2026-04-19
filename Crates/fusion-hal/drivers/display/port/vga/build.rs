use std::env;
use std::fs;
use std::path::PathBuf;

fn selected_target_name() -> String {
    let target = env::var("TARGET").expect("cargo target triple");

    if env::var_os("CARGO_FEATURE_SOC_RP2350").is_some() {
        return format!("soc-rp2350:{target}");
    }

    if env::var_os("CARGO_FEATURE_SYS_CORTEX_M").is_some() {
        return format!("sys-cortex-m:{target}");
    }

    if env::var_os("CARGO_FEATURE_SYS_FUSION_KN").is_some() {
        return format!("sys-fusion-kn:{target}");
    }

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_else(|_| "unknown".to_owned());
    format!("{target_os}:{target}")
}

fn main() {
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let shared = manifest_dir.join("../../../../fdxe/shared.rs");
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("out dir")).join("fdxe_shared.rs");

    println!("cargo:rerun-if-changed={}", shared.display());
    println!("cargo:rerun-if-env-changed=TARGET");
    println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_OS");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_SYS_CORTEX_M");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_SOC_RP2350");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_SYS_FUSION_KN");
    println!(
        "cargo:rustc-env=FUSION_FDXE_TARGET_NAME={}",
        selected_target_name()
    );

    let body = fs::read_to_string(&shared).expect("read shared FDXE ABI");
    fs::write(&out, body).expect("write staged FDXE ABI");
}
