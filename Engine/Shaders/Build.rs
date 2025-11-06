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
    // Get the current directory (workspace root)
    let cwd_path = PathBuf::from(".").canonicalize().unwrap();
    let cwd = PathBuf::from(adjust_canonicalization(cwd_path));
    print_build!("Workspace directory: {}", cwd.display());

    // Determine dylib extension
    let dylib_suffix = if cfg!(target_os = "windows") {
        ".dll"
    } else if cfg!(target_os = "linux") {
        ".so"
    } else if cfg!(target_os = "macos") {
        ".dylib"
    } else {
        panic!("Unsupported platform: {}", std::env::consts::OS);
    };

    let dylib_prefix = if cfg!(target_os = "windows") { "" } else { "lib" };
    let rustc_codegen_dylib = format!("{}rustc_codegen_spirv{}", dylib_prefix, dylib_suffix);

    // Build the absolute path to the codegen backend
    let codegen_dylib_path = cwd
        .parent()
        .unwrap()
        .join("ThirdParty/rust-gpu/target/release")
        .join(&rustc_codegen_dylib)
        .canonicalize()
        .expect("Could not find rustc_codegen_spirv dylib");

    let codegen_dylib_str = adjust_canonicalization(&codegen_dylib_path);
    print_build!("Codegen backend: {}", codegen_dylib_str);

    // Build the path to the target spec
    let target_spec_path = cwd
        .parent()
        .unwrap()
        .join("ThirdParty/rust-gpu/crates/rustc_codegen_spirv-target-specs/target-specs/spirv-unknown-spv1.6.json")
        .canonicalize()
        .expect("Could not find target spec");

    let target_spec_str = adjust_canonicalization(&target_spec_path);
    print_build!("Target spec: {}", target_spec_str);

    // Generate config.toml content
    let config_content = format!(
        r#"[build]
target = "{}"
rustflags = [
    "-Zcodegen-backend={}",
    "-Zbinary-dep-depinfo",
    "-Csymbol-mangling-version=v0",
    "-Zcrate-attr=feature(register_tool)",
    "-Zcrate-attr=register_tool(rust_gpu)"
]

[unstable]
build-std=["core"]
build-std-features=["compiler-builtins-mem"]
"#,
        target_spec_str.replace("\\", "\\\\"),
        codegen_dylib_str.replace("\\", "\\\\")
    );

    // Write the config file
    let config_path = cwd.join(".cargo/config.toml");
    fs::write(&config_path, config_content)
        .expect("Could not write config.toml");

    print_build!("Generated config.toml at: {}", config_path.display());

    // Set up rebuild triggers
    println!("cargo:rerun-if-changed=Build.rs");
    println!("cargo:rerun-if-changed=../ThirdParty/rust-gpu/target/release/{}", rustc_codegen_dylib);
}
