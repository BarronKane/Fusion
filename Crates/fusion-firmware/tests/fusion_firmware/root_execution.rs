use std::path::PathBuf;
use std::process::Command;

use crate::lock_fusion_firmware_tests;

#[test]
fn root_execution_probe_uses_canonical_fusion_main_harness() {
    let _guard = lock_fusion_firmware_tests();
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir
        .parent()
        .and_then(|path| path.parent())
        .expect("fusion-firmware crate should live under the workspace root");
    let fixture_manifest = manifest_dir.join("tests/fixtures/root_execution_probe/Cargo.toml");
    let target_dir = repo_root.join("target/test-fixtures/root_execution_probe");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let output = Command::new(cargo)
        .current_dir(&repo_root)
        .arg("run")
        .arg("--manifest-path")
        .arg(&fixture_manifest)
        .arg("--quiet")
        .env("CARGO_TARGET_DIR", &target_dir)
        .output()
        .expect("root-execution probe should run");
    assert!(
        output.status.success(),
        "root-execution probe should run successfully:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
