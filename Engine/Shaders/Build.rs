use std::fs;
use std::fs::File;
use std::io::Write;
use std::process::Command;
use std::path::{Path, PathBuf};
use escargot::CargoBuild;

/**
 *  Helper functions to print to the standard build output.
 */
macro_rules! print_build {
    ($($tokens: tt)*) => {
        println!("cargo:warning={}", format!($($tokens)*))
    }
}

/**
 *  Functions to help canonicalize paths.
 */
#[cfg(not(target_os = "windows"))]
fn adjust_canonicalization<P: AsRef<Path>>(p: P) -> String {
    p.as_ref().display().to_string()
}

/**
 *  Functions to help canonicalize paths.
 */
#[cfg(target_os = "windows")]
fn adjust_canonicalization<P: AsRef<Path>>(p: P) -> String {
    const VERBATIM_PREFIX: &str = r#"\\?\"#;
    let p = p.as_ref().display().to_string();
    if p.starts_with(VERBATIM_PREFIX) {
        p[VERBATIM_PREFIX.len()..].to_string()
    } else {
        p
    }
}

/**
 * This project is a rendering/engine learning experiment.
 * Classically, shaders existed in HLSL or sometimes GLSL.
 * Instead, we are to use rust-gpu for integrating SPIR-V.
 * Rust-gpu must be built from source, and DLL accessible.
 */
fn main() {
/*
    // Signals cargo to rebuild this if the file(s) change.


    // For testing, this will cause this script to always run.
    println!("cargo:rerun-if-changed=NULL");
    // These three file signal project changes.
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=Cargo.lock");
    // If your git HEAD changes. Hack for not needing a hook.
    // But this will trigger for every commit.
    //println!("cargo:rerun-if-changed=.git/HEAD");
    // If rust-GPU changes.
    println!("cargo::rerun-if-changed=../Crates/ThirdParty/rust-gpu/rust-toolchain.toml");

    let sys_command;
    let dylib_prefix;
    let dylib_suffix;
    if cfg!(target_os = "windows") {
        sys_command = "cmd";
        dylib_prefix = "";
        dylib_suffix = ".dll";
    } else if cfg!(target_os = "linux") {
        sys_command = "sh";
        dylib_prefix = "lib";
        dylib_suffix = ".so";
    } else if cfg!(target_os = "macos") {
        sys_command = "sh";
        dylib_prefix = "lib";
        dylib_suffix = ".dylib";
    } else {
        panic!("Unsupported (for now) platform: {}", std::env::consts::OS);
    }
    // If the rust-gpu SPIR-V compiler changes.
    println!("cargo::rerun-if-changed=../Crates/ThirdParty/rust-gpu/target/release/{}rustc_codegen_spirv{}", dylib_prefix, dylib_suffix);

    // Build rust-gpu.
    let mut r_gpu_command = Command::new(sys_command);
    r_gpu_command.current_dir(Path::new("../Crates/ThirdParty/rust-gpu"));
    r_gpu_command.args(["cargo", "build", "--release"]);
    r_gpu_command.status().expect("Rust-gpu build failed!");

    let out_dir_raw = std::env::var("OUT_DIR").expect("Could not retrieve OUT_DIR");
    print_build!("OUT_DIR: {}", out_dir_raw);
    let out_dir = Path::new(&out_dir_raw).parent().unwrap().parent().unwrap().parent().unwrap().parent().unwrap();
    print_build!("Target DIR: {}", out_dir.display());


    let cwd_path = PathBuf::from(".").canonicalize().unwrap();
    let cwd = PathBuf::from(adjust_canonicalization(cwd_path));
    print_build!("cwd: {}", cwd.display());

    let spirv_specs_dir = PathBuf::from(adjust_canonicalization(cwd.clone().parent().unwrap().join("Crates/ThirdParty/rust-gpu/crates/spirv-builder/target-specs/").canonicalize().unwrap()));
    print_build!("spirv_dir: {}", spirv_specs_dir.display());

    let config_file_example = PathBuf::from(adjust_canonicalization(cwd.clone().join(".cargo/config.example.toml").canonicalize().unwrap()));

    //let config_file = PathBuf::from(adjust_canonicalization(cwd.clone().join(".cargo/config.toml").canonicalize().unwrap()));

    // Spirv-spec to use
    let _default_spec: String = PathBuf::from(adjust_canonicalization(spirv_specs_dir.clone().join("spirv-unknown-spv1.5.json").canonicalize().unwrap())).display().to_string();

    // Canonicalized dylib path.
    let rustc_codegen_dylib: String = dylib_prefix.to_string() + "rustc_codegen_spirv" + dylib_suffix;
    // Full path to dylib.
    let _codegen_dylib_dir: String = PathBuf::from(adjust_canonicalization(cwd.clone().parent().unwrap().join("Crates/ThirdParty/rust-gpu/target/release").join(rustc_codegen_dylib).canonicalize().unwrap())).display().to_string();

    let default_spec;
    let codegen_dylib_dir;

    if cfg!(target_os = "windows") {
        default_spec = _default_spec.replace("\\", "\\\\");
        codegen_dylib_dir = _codegen_dylib_dir.replace("\\", "\\\\");
    } else {
        default_spec = _default_spec;
        codegen_dylib_dir = _codegen_dylib_dir;
    }

    let cwd_path = PathBuf::from(".").canonicalize().unwrap();
    let cwd = PathBuf::from(adjust_canonicalization(cwd_path));
    print_build!("cwd: {}", cwd.display());
    let config_file_example = PathBuf::from(adjust_canonicalization(cwd.clone().join(".cargo/config.example.toml").canonicalize().unwrap()));

    //let default_spec = "../../ThirdParty/rust-gpu/crates/spirv-builder/target-specs/spirv-unknown-spv1.5.json";
    let rustc_codegen_dylib = dylib_prefix.to_string() + "rustc_codegen_spirv" + dylib_suffix;
    //let codegen_dylib = "../../ThirdParty/rust-gpu/target/release/".to_string() + &rustc_codegen_dylib;

    let example_contents = fs::read_to_string(&config_file_example).expect("Could not read example config.toml");
    // Replace with spir-v spec.
    let spec_config = example_contents.replace("<path_to_target_spec>", default_spec.as_str());
    // Replace with codegen dylib.
    let spec_config_with_spirv = spec_config.replace("<absolute_path_to_librustc_codegen_spirv>", codegen_dylib_dir.as_str());

    let mut config_file = fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .create(true)
        .open(".cargo/config.toml")
        .expect("Could not open config");
    config_file.write_all(spec_config_with_spirv.as_bytes()).expect("Could not write config.toml");
    */

    //println!("cargo:rustc-flags=-Zcodegen-backend={}");
}
