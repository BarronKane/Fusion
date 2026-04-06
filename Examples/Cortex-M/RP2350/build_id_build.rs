use std::collections::BTreeSet;
use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    emit_rerun_triggers();
    emit_build_identity_env();
}

fn emit_rerun_triggers() {
    println!("cargo:rerun-if-env-changed=PROFILE");
    println!("cargo:rerun-if-env-changed=TARGET");
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
    let mut features = BTreeSet::new();
    for (key, _) in env::vars() {
        if let Some(feature) = key.strip_prefix("CARGO_FEATURE_") {
            features.insert(feature.to_owned());
        }
    }

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
