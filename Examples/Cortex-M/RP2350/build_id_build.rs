use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

const ROOT_TASK_PIPELINE_SKIP_ENV: &str = "FUSION_SKIP_FIBER_TASK_PIPELINE";
const ROOT_TASK_CONTRACTS_ENV: &str = "FUSION_FIRMWARE_GENERATED_FIBER_TASK_CONTRACTS_RS";

fn main() {
    emit_rerun_triggers();
    emit_build_identity_env();
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
    if let Ok(bin) = package_binary_descriptor(&manifest_path) {
        println!("cargo:rerun-if-changed={}", bin.path.display());
    }

    for (key, _) in env::vars() {
        if key.starts_with("CARGO_FEATURE_") {
            println!("cargo:rerun-if-env-changed={key}");
        }
    }

    if let Some(git_root) = git_root() {
        let head_path = git_root.join(".git").join("HEAD");
        println!("cargo:rerun-if-changed={}", head_path.display());
    }
}

fn emit_build_identity_env() {
    println!(
        "cargo:rustc-env=FUSION_RP2350_BUILD_PROFILE={}",
        env::var("PROFILE").unwrap_or_else(|_| "unknown".to_owned())
    );
    println!(
        "cargo:rustc-env=FUSION_RP2350_BUILD_TARGET={}",
        env::var("TARGET").unwrap_or_else(|_| "unknown".to_owned())
    );
    println!(
        "cargo:rustc-env=FUSION_RP2350_BUILD_GIT_SHA={}",
        git_short_sha().unwrap_or_else(|| "nogit".to_owned())
    );
    println!(
        "cargo:rustc-env=FUSION_RP2350_BUILD_DIRTY={}",
        if git_dirty().unwrap_or(false) { "1" } else { "0" }
    );
    println!(
        "cargo:rustc-env=FUSION_RP2350_BUILD_FEATURES_HASH={}",
        active_features_hash_hex()
    );
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
    let bin = package_binary_descriptor(&manifest_path)
        .unwrap_or_else(|error| panic!("failed to determine package binary for root task contracts: {error}"));
    let workspace_root = workspace_root(&manifest_dir)
        .unwrap_or_else(|error| panic!("failed to locate workspace root for root task contracts: {error}"));
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

    let mut command = Command::new("cargo");
    command
        .current_dir(&workspace_root)
        .env(
            "CARGO_TARGET_DIR",
            workspace_root.join("target").join("build-tools"),
        )
        .env(ROOT_TASK_PIPELINE_SKIP_ENV, "1")
        .arg("run")
        .arg("-p")
        .arg("fusion-std")
        .arg("--bin")
        .arg("fusion_std_fiber_task_pipeline")
        .arg("--")
        .arg("--manifest-path")
        .arg(&manifest_path)
        .arg("--package")
        .arg(env::var("CARGO_PKG_NAME").expect("Cargo should provide CARGO_PKG_NAME"))
        .arg("--bin")
        .arg(&bin.name)
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
        .arg(&async_rust_path);

    let active_features = active_feature_names();
    if !active_features.is_empty() {
        command.arg("--features").arg(active_features.join(","));
    }

    let output = command.output().unwrap_or_else(|error| {
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

fn git_root() -> Option<PathBuf> {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").ok()?);
    let output = Command::new("git")
        .arg("-C")
        .arg(&manifest_dir)
        .arg("rev-parse")
        .arg("--show-toplevel")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let root = String::from_utf8(output.stdout).ok()?;
    let root = root.trim();
    if root.is_empty() {
        None
    } else {
        Some(PathBuf::from(root))
    }
}

fn git_short_sha() -> Option<String> {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").ok()?);
    let output = Command::new("git")
        .arg("-C")
        .arg(&manifest_dir)
        .arg("rev-parse")
        .arg("--short=12")
        .arg("HEAD")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let sha = String::from_utf8(output.stdout).ok()?;
    let sha = sha.trim();
    if sha.is_empty() {
        None
    } else {
        Some(sha.to_owned())
    }
}

fn git_dirty() -> Option<bool> {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").ok()?);
    let output = Command::new("git")
        .arg("-C")
        .arg(&manifest_dir)
        .arg("status")
        .arg("--porcelain")
        .arg("--untracked-files=no")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let status = String::from_utf8(output.stdout).ok()?;
    Some(!status.trim().is_empty())
}

fn active_features_hash_hex() -> String {
    let features = active_feature_names().into_iter().collect::<BTreeSet<_>>();

    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for feature in features {
        for byte in feature.bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        hash ^= u64::from(b',');
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

fn active_feature_names() -> Vec<String> {
    let mut features = BTreeSet::new();
    for (key, _) in env::vars() {
        let Some(feature) = key.strip_prefix("CARGO_FEATURE_") else {
            continue;
        };
        features.insert(feature.to_ascii_lowercase().replace('_', "-"));
    }
    features.into_iter().collect()
}

#[derive(Debug, Clone)]
struct PackageBinaryDescriptor {
    name: String,
    path: PathBuf,
}

fn package_binary_descriptor(manifest_path: &Path) -> Result<PackageBinaryDescriptor, String> {
    let contents = fs::read_to_string(manifest_path)
        .map_err(|error| format!("failed to read {}: {error}", manifest_path.display()))?;
    let manifest_dir = manifest_path
        .parent()
        .ok_or_else(|| format!("manifest path {} has no parent", manifest_path.display()))?;
    let mut in_bin = false;
    let mut current_name = None::<String>;
    let mut current_path = None::<PathBuf>;

    for raw_line in contents.lines() {
        let line = raw_line.split('#').next().unwrap_or("").trim();
        if line == "[[bin]]" {
            if in_bin && current_name.is_some() {
                break;
            }
            in_bin = true;
            current_name = None;
            current_path = None;
            continue;
        }
        if !in_bin {
            continue;
        }
        if line.starts_with('[') && line != "[[bin]]" {
            if current_name.is_some() {
                break;
            }
            in_bin = false;
            continue;
        }
        if let Some(name) = parse_toml_string_assignment(line, "name") {
            current_name = Some(name.to_owned());
            continue;
        }
        if let Some(path) = parse_toml_string_assignment(line, "path") {
            current_path = Some(manifest_dir.join(path));
        }
    }

    let name = current_name.ok_or_else(|| {
        format!(
            "failed to locate [[bin]] name in {}",
            manifest_path.display()
        )
    })?;
    let path = current_path.unwrap_or_else(|| manifest_dir.join("main.rs"));
    Ok(PackageBinaryDescriptor { name, path })
}

fn parse_toml_string_assignment<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let (left, right) = line.split_once('=')?;
    if left.trim() != key {
        return None;
    }
    let value = right.trim();
    value.strip_prefix('"')?.strip_suffix('"')
}

fn workspace_root(manifest_dir: &Path) -> Result<PathBuf, String> {
    for ancestor in manifest_dir.ancestors() {
        let cargo_toml = ancestor.join("Cargo.toml");
        let Ok(contents) = fs::read_to_string(&cargo_toml) else {
            continue;
        };
        if contents
            .lines()
            .map(|line| line.split('#').next().unwrap_or("").trim())
            .any(|line| line == "[workspace]")
        {
            return Ok(ancestor.to_path_buf());
        }
    }
    Err(format!(
        "failed to locate workspace root above {}",
        manifest_dir.display()
    ))
}
