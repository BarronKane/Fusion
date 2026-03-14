use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=Kbuild");
    println!("cargo:rerun-if-changed=Makefile");
    println!("cargo:rerun-if-changed=fusion_kn.rs");
    println!("cargo:rerun-if-env-changed=KDIR");
    println!("cargo:rerun-if-env-changed=FUSION_KN_SKIP_KERNEL_BUILD");

    if env::var_os("FUSION_KN_SKIP_KERNEL_BUILD").is_some() {
        println!(
            "cargo:warning=fusion-kn: skipping kernel module build because FUSION_KN_SKIP_KERNEL_BUILD is set"
        );
        return;
    }

    if env::var("CARGO_CFG_TARGET_OS").ok().as_deref() != Some("linux") {
        println!(
            "cargo:warning=fusion-kn: skipping kernel module build because target OS is not Linux"
        );
        return;
    }

    let manifest_dir = PathBuf::from(
        env::var("CARGO_MANIFEST_DIR").expect("Cargo should always provide CARGO_MANIFEST_DIR"),
    );
    let Some(kdir) = kernel_build_dir() else {
        println!(
            "cargo:warning=fusion-kn: skipping kernel module build because no kernel build tree was found"
        );
        return;
    };

    let Some(kernel_rust) = kernel_rust_status(&kdir) else {
        println!(
            "cargo:warning=fusion-kn: skipping kernel module build because kernel Rust support is unavailable in {}",
            kdir.display()
        );
        return;
    };

    let Some(host_rustc_version) = current_rustc_version() else {
        panic!("fusion-kn: unable to determine current rustc version");
    };

    let system_rustc_version = rustc_version_at(Path::new("/usr/bin/rustc"));

    let mut make = Command::new("make");
    let out_dir = kernel_output_dir(&manifest_dir);
    fs::create_dir_all(&out_dir).expect("failed to create kernel module output directory");
    make.arg("-C")
        .arg(&kdir)
        .arg(format!("M={}", manifest_dir.display()))
        .arg(format!("MO={}", out_dir.display()));

    if let Some(kernel_rustc_version) = kernel_rust.rustc_version
        && kernel_rustc_version != host_rustc_version
    {
        match system_rustc_version {
            Some(version) if version == kernel_rustc_version => {
                make.arg("RUSTC=/usr/bin/rustc")
                    .arg("HOSTRUSTC=/usr/bin/rustc");

                if Path::new("/usr/bin/rustfmt").is_file() {
                    make.arg("RUSTFMT=/usr/bin/rustfmt");
                }
                if Path::new("/usr/bin/clippy-driver").is_file() {
                    make.arg("CLIPPY_DRIVER=/usr/bin/clippy-driver");
                }

                println!(
                    "cargo:warning=fusion-kn: using /usr/bin/rustc to match kernel Rust metadata {} instead of active rustc {}",
                    format_rustc_version(kernel_rustc_version),
                    format_rustc_version(host_rustc_version)
                );
            }
            Some(system_version) => {
                panic!(
                    "fusion-kn: kernel Rust metadata expects rustc {}, Cargo is using {}, and /usr/bin/rustc is {}. Use the distribution Rust toolchain that built the kernel.",
                    format_rustc_version(kernel_rustc_version),
                    format_rustc_version(host_rustc_version),
                    format_rustc_version(system_version)
                );
            }
            None => {
                panic!(
                    "fusion-kn: kernel Rust metadata expects rustc {} but Cargo is using {}, and /usr/bin/rustc could not be queried. Use the distribution Rust toolchain that built the kernel.",
                    format_rustc_version(kernel_rustc_version),
                    format_rustc_version(host_rustc_version)
                );
            }
        }
    }

    let status = make
        .current_dir(&manifest_dir)
        .status()
        .expect("failed to invoke kernel module build via make");

    assert!(
        status.success(),
        "fusion-kn kernel module build failed against {}",
        kdir.display()
    );
}

fn kernel_build_dir() -> Option<PathBuf> {
    if let Some(explicit) = env::var_os("KDIR") {
        let path = PathBuf::from(explicit);
        if path.is_dir() {
            return Some(path);
        }
    }

    let release = kernel_release()?;
    let path = PathBuf::from(format!("/lib/modules/{release}/build"));
    path.is_dir().then_some(path)
}

fn kernel_release() -> Option<String> {
    let output = Command::new("uname").arg("-r").output().ok()?;
    if !output.status.success() {
        return None;
    }

    let release = String::from_utf8(output.stdout).ok()?;
    let trimmed = release.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

fn kernel_output_dir(manifest_dir: &Path) -> PathBuf {
    manifest_dir.join("../../target/kernel")
}

#[derive(Debug, Clone, Copy)]
struct KernelRustStatus {
    rustc_version: Option<u32>,
}

fn kernel_rust_status(kdir: &Path) -> Option<KernelRustStatus> {
    if !kdir.join("rust/libkernel.rmeta").is_file() {
        return None;
    }

    let config = fs::read_to_string(kdir.join(".config")).ok()?;
    if !config.lines().any(|line| line.trim() == "CONFIG_RUST=y") {
        return None;
    }

    Some(KernelRustStatus {
        rustc_version: parse_kernel_rustc_version(&config),
    })
}

fn parse_kernel_rustc_version(config: &str) -> Option<u32> {
    let value = config
        .lines()
        .find_map(|line| line.strip_prefix("CONFIG_RUSTC_VERSION="))?;
    value.trim().parse().ok()
}

fn current_rustc_version() -> Option<u32> {
    let mut command = Command::new("rustc");
    rustc_version_command(&mut command)
}

fn rustc_version_at(path: &Path) -> Option<u32> {
    if !path.is_file() {
        return None;
    }

    let mut command = Command::new(path);
    rustc_version_command(&mut command)
}

fn rustc_version_command(command: &mut Command) -> Option<u32> {
    let output = command.arg("--version").output().ok()?;
    if !output.status.success() {
        return None;
    }

    let version = String::from_utf8(output.stdout).ok()?;
    parse_rustc_semver(&version)
}

fn parse_rustc_semver(version: &str) -> Option<u32> {
    let semver = version.split_whitespace().nth(1)?;
    let mut parts = semver.split('.');
    let major: u32 = parts.next()?.parse().ok()?;
    let minor: u32 = parts.next()?.parse().ok()?;
    let patch_digits: String = parts
        .next()?
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    let patch: u32 = patch_digits.parse().ok()?;
    Some(major * 100_000 + minor * 100 + patch)
}

fn format_rustc_version(version: u32) -> String {
    let major = version / 100_000;
    let minor = (version / 100) % 1_000;
    let patch = version % 100;
    format!("{major}.{minor}.{patch}")
}
