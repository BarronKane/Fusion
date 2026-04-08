use std::env;
use std::fs;
use std::path::{
    Path,
    PathBuf,
};
use std::process::Command;

const ROOT_TASK_PIPELINE_SKIP_ENV: &str = "FUSION_SKIP_FIBER_TASK_PIPELINE";
const ROOT_TASK_CONTRACTS_ENV: &str = "FUSION_FIRMWARE_GENERATED_FIBER_TASK_CONTRACTS_RS";

fn main() {
    emit_rerun_triggers();
    emit_root_fiber_contracts_env();
}

fn emit_rerun_triggers() {
    println!("cargo:rerun-if-env-changed=PROFILE");
    println!("cargo:rerun-if-env-changed=TARGET");
    println!("cargo:rerun-if-env-changed={ROOT_TASK_PIPELINE_SKIP_ENV}");

    let manifest_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("Cargo should provide CARGO_MANIFEST_DIR"));
    let manifest_path = manifest_dir.join("Cargo.toml");
    println!("cargo:rerun-if-changed={}", manifest_path.display());
    println!("cargo:rerun-if-changed={}", manifest_dir.join("src/main.rs").display());
}

fn emit_root_fiber_contracts_env() {
    println!("cargo:rustc-check-cfg=cfg(fusion_firmware_root_task_bootstrap)");
    if env::var_os(ROOT_TASK_PIPELINE_SKIP_ENV).is_some() {
        println!("cargo:rustc-cfg=fusion_firmware_root_task_bootstrap");
        return;
    }

    let manifest_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("Cargo should provide CARGO_MANIFEST_DIR"));
    let manifest_path = manifest_dir.join("Cargo.toml");
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("Cargo should provide OUT_DIR"));
    let workspace_root = workspace_root(&manifest_dir)
        .unwrap_or_else(|error| panic!("failed to locate workspace root for probe contracts: {error}"));
    let fusion_std_manifest = workspace_root.join("Crates/fusion-std/Cargo.toml");
    let contracts_path = out_dir.join("fusion-firmware-main.contracts.rs");
    let metadata_path = out_dir.join("fusion-firmware-main.generated");
    let report_path = out_dir.join("fusion-firmware-main.report");
    let red_inline_rust_path = out_dir.join("fusion-firmware-main.red-inline.contracts.rs");
    let async_output_path = out_dir.join("fusion-firmware-main.async.generated");
    let async_rust_path = out_dir.join("fusion-firmware-main.async.contracts.rs");
    fs::create_dir_all(&out_dir).unwrap_or_else(|error| {
        panic!(
            "failed to create root-task contract output directory {}: {error}",
            out_dir.display()
        )
    });

    let output = Command::new("cargo")
        .current_dir(&workspace_root)
        .env("CARGO_TARGET_DIR", workspace_root.join("target").join("build-tools"))
        .env(ROOT_TASK_PIPELINE_SKIP_ENV, "1")
        .arg("run")
        .arg("--manifest-path")
        .arg(&fusion_std_manifest)
        .arg("--bin")
        .arg("fusion_std_fiber_task_pipeline")
        .arg("--")
        .arg("--manifest-path")
        .arg(&manifest_path)
        .arg("--package")
        .arg(env::var("CARGO_PKG_NAME").expect("Cargo should provide CARGO_PKG_NAME"))
        .arg("--bin")
        .arg("root_execution_probe")
        .arg("--no-closure-roots")
        .arg("--profile")
        .arg(env::var("PROFILE").unwrap_or_else(|_| "debug".to_owned()))
        .arg("--target")
        .arg(env::var("TARGET").expect("Cargo should provide TARGET"))
        .arg("--rust-contracts")
        .arg(&contracts_path)
        .arg("--output")
        .arg(&metadata_path)
        .arg("--report")
        .arg(&report_path)
        .arg("--red-inline-rust")
        .arg(&red_inline_rust_path)
        .arg("--async-poll-stack-output")
        .arg(&async_output_path)
        .arg("--async-poll-stack-rust")
        .arg(&async_rust_path)
        .output()
        .unwrap_or_else(|error| {
            panic!("failed to run fusion_std_fiber_task_pipeline for root task contracts: {error}")
        });
    if !output.status.success() {
        panic!(
            "fusion_std_fiber_task_pipeline failed for root task contracts with status {}:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    println!(
        "cargo:rustc-env={ROOT_TASK_CONTRACTS_ENV}={}",
        contracts_path.display()
    );
}

fn workspace_root(start: &Path) -> Result<PathBuf, String> {
    let mut candidate = None;
    for ancestor in start.ancestors() {
        if ancestor.join("Cargo.toml").is_file() {
            candidate = Some(ancestor.to_path_buf());
        }
    }
    candidate.ok_or_else(|| format!("no Cargo workspace root found above {}", start.display()))
}
