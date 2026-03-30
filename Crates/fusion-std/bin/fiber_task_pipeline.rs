use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

const DEFAULT_OUTPUT_NAME: &str = "fusion-std-fiber-task.generated";
const DEFAULT_ASYNC_POLL_STACK_OUTPUT_NAME: &str = "fusion-std-async-poll-stack.generated";
const DEFAULT_ASYNC_POLL_STACK_RUST_NAME: &str = "fusion-std-async-poll-stack.contracts.rs";
const DEFAULT_GENERATED_ROOTS_NAME: &str = "fusion-std-fiber-task.roots";
const DEFAULT_ASYNC_POLL_STACK_ROOTS_NAME: &str = "async-poll-stack.roots";
const DEFAULT_REPORT_NAME: &str = "fusion-std-fiber-task.report";
const DEFAULT_RUST_CONTRACTS_NAME: &str = "fusion-std-fiber-task.contracts.rs";
const DEFAULT_CONTRACTS_NAME: &str = "fiber-task.contracts";
const DEFAULT_RED_INLINE_CONTRACTS_NAME: &str = "red-inline.contracts";
const DEFAULT_RED_INLINE_RUST_NAME: &str = "fusion-std-red-inline.contracts.rs";
const GENERATED_CLOSURE_ROOT_SYMBOL_PREFIX: &str =
    "fusion_std::thread::fiber::generated_closure_task_root";
const GENERATED_ASYNC_POLL_STACK_ROOT_SYMBOL_PREFIX: &str =
    "fusion_std::thread::executor::generated_async_poll_stack_root";

fn main() {
    if let Err(error) = run() {
        eprintln!("fusion_std_fiber_task_pipeline: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| "failed to determine workspace root".to_owned())?
        .to_path_buf();
    let current_dir =
        env::current_dir().map_err(|error| format!("failed to read current directory: {error}"))?;
    let config = PipelineConfig::parse(&current_dir, &manifest_dir, &workspace_root)?;
    let target_dir = pipeline_target_dir(&workspace_root, &config);
    build_target_artifact(&workspace_root, &target_dir, &config)?;
    let artifact = find_target_artifact(&target_dir, &config)?;
    let auxiliary_artifacts = collect_auxiliary_runtime_artifacts(&target_dir, &config, &artifact)?;
    let roots_path = materialize_roots(&workspace_root, &target_dir, &config, &artifact)?;
    let async_poll_stack_roots_path =
        materialize_async_poll_stack_roots(&target_dir, &config, &artifact)?;
    if async_poll_stack_roots_path.is_none() {
        write_empty_async_poll_stack_sidecars(&config)?;
    }
    run_analyzer(
        &workspace_root,
        &config,
        &roots_path,
        async_poll_stack_roots_path.as_deref(),
        &artifact,
        &auxiliary_artifacts,
    )?;
    Ok(())
}

fn write_empty_async_poll_stack_sidecars(config: &PipelineConfig) -> Result<(), String> {
    fs::write(&config.async_poll_stack_output_path, "").map_err(|error| {
        format!(
            "failed to write empty async poll-stack metadata {}: {error}",
            config.async_poll_stack_output_path.display()
        )
    })?;
    fs::write(
        &config.async_poll_stack_rust_path,
        "#[allow(dead_code)]\nconst GENERATED_ASYNC_POLL_STACK_TASKS: &[GeneratedAsyncPollStackMetadataEntry] = &[];\n",
    )
    .map_err(|error| {
        format!(
            "failed to write empty async poll-stack Rust sidecar {}: {error}",
            config.async_poll_stack_rust_path.display()
        )
    })?;
    Ok(())
}

fn pipeline_target_dir(workspace_root: &Path, config: &PipelineConfig) -> PathBuf {
    let mut dir = workspace_root
        .join("target")
        .join("fiber-task-pipeline")
        .join(sanitize_path_component(&config.package))
        .join(match &config.target_artifact {
            TargetArtifact::Lib => "lib".to_owned(),
            TargetArtifact::Bin(name) => sanitize_path_component(name),
        })
        .join(config.profile.dir_name());
    if let Some(target) = config.target.as_ref() {
        dir = dir.join(sanitize_path_component(target));
    } else {
        dir = dir.join("host");
    }
    if config.no_default_features {
        dir = dir.join("no-default-features");
    } else {
        dir = dir.join("default-features");
    }
    if let Some(features) = config.features.as_ref() {
        dir.join(sanitize_path_component(features))
    } else {
        dir.join("default")
    }
}

fn sanitize_path_component(raw: &str) -> String {
    let mut sanitized = String::with_capacity(raw.len());
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
            sanitized.push(ch);
        } else {
            sanitized.push('_');
        }
    }
    if sanitized.is_empty() {
        "_".to_owned()
    } else {
        sanitized
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BuildProfile {
    Dev,
    Release,
}

impl BuildProfile {
    const fn dir_name(self) -> &'static str {
        match self {
            Self::Dev => "debug",
            Self::Release => "release",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TargetArtifact {
    Lib,
    Bin(String),
}

#[derive(Debug)]
struct PipelineConfig {
    manifest_path: Option<PathBuf>,
    roots_path: Option<PathBuf>,
    contracts_path: Option<PathBuf>,
    red_inline_contracts_path: Option<PathBuf>,
    async_poll_stack_roots_path: Option<PathBuf>,
    report_path: PathBuf,
    rust_contracts_path: PathBuf,
    red_inline_rust_path: PathBuf,
    output_path: PathBuf,
    async_poll_stack_output_path: PathBuf,
    async_poll_stack_rust_path: PathBuf,
    toolchain: String,
    package: String,
    crate_name: String,
    target_artifact: TargetArtifact,
    profile: BuildProfile,
    target: Option<String>,
    features: Option<String>,
    no_default_features: bool,
}

impl PipelineConfig {
    fn parse(
        current_dir: &Path,
        manifest_dir: &Path,
        workspace_root: &Path,
    ) -> Result<Self, String> {
        let mut args = env::args_os();
        let _program = args.next();

        let mut roots_path = None;
        let mut manifest_path = None;
        let mut contracts_path = manifest_dir
            .join(DEFAULT_CONTRACTS_NAME)
            .is_file()
            .then(|| manifest_dir.join(DEFAULT_CONTRACTS_NAME));
        let mut red_inline_contracts_path = manifest_dir
            .join(DEFAULT_RED_INLINE_CONTRACTS_NAME)
            .is_file()
            .then(|| manifest_dir.join(DEFAULT_RED_INLINE_CONTRACTS_NAME));
        let mut async_poll_stack_roots_path = manifest_dir
            .join(DEFAULT_ASYNC_POLL_STACK_ROOTS_NAME)
            .is_file()
            .then(|| manifest_dir.join(DEFAULT_ASYNC_POLL_STACK_ROOTS_NAME));
        let mut report_path = workspace_root.join("target").join(DEFAULT_REPORT_NAME);
        let mut rust_contracts_path = workspace_root
            .join("target")
            .join(DEFAULT_RUST_CONTRACTS_NAME);
        let mut red_inline_rust_path = workspace_root
            .join("target")
            .join(DEFAULT_RED_INLINE_RUST_NAME);
        let mut output_path = workspace_root.join("target").join(DEFAULT_OUTPUT_NAME);
        let mut async_poll_stack_output_path = workspace_root
            .join("target")
            .join(DEFAULT_ASYNC_POLL_STACK_OUTPUT_NAME);
        let mut async_poll_stack_rust_path = workspace_root
            .join("target")
            .join(DEFAULT_ASYNC_POLL_STACK_RUST_NAME);
        let mut toolchain = load_workspace_toolchain(workspace_root)?;
        let mut package = "fusion-std".to_owned();
        let mut crate_name = "fusion_std".to_owned();
        let mut target_artifact = TargetArtifact::Lib;
        let mut profile = BuildProfile::Dev;
        let mut target = None;
        let mut features = None;
        let mut no_default_features = false;

        while let Some(arg) = args.next() {
            apply_cli_arg(
                current_dir,
                &mut args,
                &arg.to_string_lossy(),
                &mut manifest_path,
                &mut roots_path,
                &mut contracts_path,
                &mut red_inline_contracts_path,
                &mut async_poll_stack_roots_path,
                &mut report_path,
                &mut rust_contracts_path,
                &mut red_inline_rust_path,
                &mut output_path,
                &mut async_poll_stack_output_path,
                &mut async_poll_stack_rust_path,
                &mut toolchain,
                &mut package,
                &mut crate_name,
                &mut target_artifact,
                &mut profile,
                &mut target,
                &mut features,
                &mut no_default_features,
            )?;
        }

        if async_poll_stack_roots_path.is_none()
            && manifest_dir
                .join(DEFAULT_ASYNC_POLL_STACK_ROOTS_NAME)
                .is_file()
        {
            async_poll_stack_roots_path =
                Some(manifest_dir.join(DEFAULT_ASYNC_POLL_STACK_ROOTS_NAME));
        }

        Ok(Self {
            manifest_path,
            roots_path,
            contracts_path,
            red_inline_contracts_path,
            async_poll_stack_roots_path,
            report_path,
            rust_contracts_path,
            red_inline_rust_path,
            output_path,
            async_poll_stack_output_path,
            async_poll_stack_rust_path,
            toolchain,
            package,
            crate_name,
            target_artifact,
            profile,
            target,
            features,
            no_default_features,
        })
    }
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn apply_cli_arg(
    current_dir: &Path,
    args: &mut impl Iterator<Item = std::ffi::OsString>,
    arg: &str,
    manifest_path: &mut Option<PathBuf>,
    roots_path: &mut Option<PathBuf>,
    contracts_path: &mut Option<PathBuf>,
    red_inline_contracts_path: &mut Option<PathBuf>,
    async_poll_stack_roots_path: &mut Option<PathBuf>,
    report_path: &mut PathBuf,
    rust_contracts_path: &mut PathBuf,
    red_inline_rust_path: &mut PathBuf,
    output_path: &mut PathBuf,
    async_poll_stack_output_path: &mut PathBuf,
    async_poll_stack_rust_path: &mut PathBuf,
    toolchain: &mut String,
    package: &mut String,
    crate_name: &mut String,
    target_artifact: &mut TargetArtifact,
    profile: &mut BuildProfile,
    target: &mut Option<String>,
    features: &mut Option<String>,
    no_default_features: &mut bool,
) -> Result<(), String> {
    match arg {
        "--manifest-path" => {
            *manifest_path = Some(resolve_cli_path(
                current_dir,
                &PathBuf::from(
                    args.next()
                        .ok_or_else(|| usage("missing value for --manifest-path"))?,
                ),
            ));
        }
        "--roots" => {
            *roots_path = Some(resolve_cli_path(
                current_dir,
                &PathBuf::from(
                    args.next()
                        .ok_or_else(|| usage("missing value for --roots"))?,
                ),
            ));
        }
        "--output" => {
            *output_path = resolve_cli_path(
                current_dir,
                &PathBuf::from(
                    args.next()
                        .ok_or_else(|| usage("missing value for --output"))?,
                ),
            );
        }
        "--async-poll-stack-output" => {
            *async_poll_stack_output_path = resolve_cli_path(
                current_dir,
                &PathBuf::from(
                    args.next()
                        .ok_or_else(|| usage("missing value for --async-poll-stack-output"))?,
                ),
            );
        }
        "--async-poll-stack-rust" => {
            *async_poll_stack_rust_path = resolve_cli_path(
                current_dir,
                &PathBuf::from(
                    args.next()
                        .ok_or_else(|| usage("missing value for --async-poll-stack-rust"))?,
                ),
            );
        }
        "--contracts" => {
            *contracts_path = Some(resolve_cli_path(
                current_dir,
                &PathBuf::from(
                    args.next()
                        .ok_or_else(|| usage("missing value for --contracts"))?,
                ),
            ));
        }
        "--red-inline-contracts" => {
            *red_inline_contracts_path = Some(resolve_cli_path(
                current_dir,
                &PathBuf::from(
                    args.next()
                        .ok_or_else(|| usage("missing value for --red-inline-contracts"))?,
                ),
            ));
        }
        "--async-poll-stack-roots" => {
            *async_poll_stack_roots_path = Some(resolve_cli_path(
                current_dir,
                &PathBuf::from(
                    args.next()
                        .ok_or_else(|| usage("missing value for --async-poll-stack-roots"))?,
                ),
            ));
        }
        "--report" => {
            *report_path = resolve_cli_path(
                current_dir,
                &PathBuf::from(
                    args.next()
                        .ok_or_else(|| usage("missing value for --report"))?,
                ),
            );
        }
        "--rust-contracts" => {
            *rust_contracts_path = resolve_cli_path(
                current_dir,
                &PathBuf::from(
                    args.next()
                        .ok_or_else(|| usage("missing value for --rust-contracts"))?,
                ),
            );
        }
        "--red-inline-rust" => {
            *red_inline_rust_path = resolve_cli_path(
                current_dir,
                &PathBuf::from(
                    args.next()
                        .ok_or_else(|| usage("missing value for --red-inline-rust"))?,
                ),
            );
        }
        "--toolchain" => {
            *toolchain = args
                .next()
                .ok_or_else(|| usage("missing value for --toolchain"))?
                .to_string_lossy()
                .into_owned();
        }
        "--package" => {
            *package = args
                .next()
                .ok_or_else(|| usage("missing value for --package"))?
                .to_string_lossy()
                .into_owned();
            *crate_name = sanitize_crate_name(package);
        }
        "--crate-name" => {
            *crate_name = args
                .next()
                .ok_or_else(|| usage("missing value for --crate-name"))?
                .to_string_lossy()
                .into_owned();
        }
        "--bin" => {
            let name = args
                .next()
                .ok_or_else(|| usage("missing value for --bin"))?
                .to_string_lossy()
                .into_owned();
            *crate_name = sanitize_crate_name(&name);
            *target_artifact = TargetArtifact::Bin(name);
        }
        "--lib" => {
            *target_artifact = TargetArtifact::Lib;
        }
        "--profile" => {
            *profile = match args
                .next()
                .ok_or_else(|| usage("missing value for --profile"))?
                .to_string_lossy()
                .as_ref()
            {
                "dev" | "debug" => BuildProfile::Dev,
                "release" => BuildProfile::Release,
                other => return Err(usage(&format!("unsupported profile `{other}`"))),
            };
        }
        "--target" => {
            *target = Some(
                args.next()
                    .ok_or_else(|| usage("missing value for --target"))?
                    .to_string_lossy()
                    .into_owned(),
            );
        }
        "--features" => {
            *features = Some(
                args.next()
                    .ok_or_else(|| usage("missing value for --features"))?
                    .to_string_lossy()
                    .into_owned(),
            );
        }
        "--no-default-features" => {
            *no_default_features = true;
        }
        other => return Err(usage(&format!("unexpected argument `{other}`"))),
    }
    Ok(())
}

fn sanitize_crate_name(package: &str) -> String {
    package.replace('-', "_")
}

fn resolve_cli_path(current_dir: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        current_dir.join(path)
    }
}

fn load_workspace_toolchain(workspace_root: &Path) -> Result<String, String> {
    let toolchain_path = workspace_root.join("rust-toolchain.toml");
    let contents = fs::read_to_string(&toolchain_path)
        .map_err(|error| format!("failed to read {}: {error}", toolchain_path.display()))?;
    for raw_line in contents.lines() {
        let line = raw_line.split('#').next().unwrap_or("").trim();
        if let Some(rest) = line.strip_prefix("channel")
            && let Some((_, value)) = rest.split_once('=')
        {
            let channel = value.trim().trim_matches('"');
            if !channel.is_empty() {
                return Ok(channel.to_owned());
            }
        }
    }
    Err(format!(
        "failed to locate `channel = ...` in {}",
        toolchain_path.display()
    ))
}

fn build_target_artifact(
    workspace_root: &Path,
    target_dir: &Path,
    config: &PipelineConfig,
) -> Result<(), String> {
    let mut command = cargo_command(&config.toolchain);
    append_rustflags_env(&mut command, "-Z emit-stack-sizes");
    command
        .current_dir(workspace_root)
        .env("CARGO_INCREMENTAL", "0")
        .arg("rustc")
        .arg("--target-dir")
        .arg(target_dir);

    if let Some(manifest_path) = config.manifest_path.as_ref() {
        command.arg("--manifest-path").arg(manifest_path);
    }

    command.arg("-p").arg(&config.package);

    match &config.target_artifact {
        TargetArtifact::Lib => {
            command.arg("--lib");
        }
        TargetArtifact::Bin(name) => {
            command.arg("--bin").arg(name);
        }
    }

    if config.profile == BuildProfile::Release {
        command.arg("--release");
    }
    if let Some(target) = config.target.as_ref() {
        command.arg("--target").arg(target);
    }
    if let Some(features) = config.features.as_ref() {
        command.arg("--features").arg(features);
    }
    if config.no_default_features {
        command.arg("--no-default-features");
    }

    command.arg("--").arg("--emit=obj");
    run_command(command, "cargo rustc")
}

fn append_rustflags_env(command: &mut Command, extra_flags: &str) {
    let existing = env::var("RUSTFLAGS").unwrap_or_default();
    let combined = if existing.trim().is_empty() {
        extra_flags.to_owned()
    } else {
        format!("{existing} {extra_flags}")
    };
    command.env("RUSTFLAGS", combined);
}

fn materialize_roots(
    workspace_root: &Path,
    target_dir: &Path,
    config: &PipelineConfig,
    artifact: &Path,
) -> Result<PathBuf, String> {
    if let Some(path) = config.roots_path.as_ref() {
        return Ok(path.clone());
    }

    let output_path = target_dir.join(DEFAULT_GENERATED_ROOTS_NAME);
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
    }

    let mut rendered = String::new();
    if config.package == "fusion-std" && matches!(config.target_artifact, TargetArtifact::Lib) {
        rendered.push_str(&collect_named_roots_from_emitter(
            workspace_root,
            &config.toolchain,
        )?);
    }
    if should_collect_discovered_closure_roots(config) {
        rendered.push_str(&collect_generated_closure_roots(
            artifact,
            &config.crate_name,
        )?);
    }
    fs::write(&output_path, rendered)
        .map_err(|error| format!("failed to write {}: {error}", output_path.display()))?;
    Ok(output_path)
}

fn should_collect_discovered_closure_roots(config: &PipelineConfig) -> bool {
    !matches!(config.target_artifact, TargetArtifact::Lib) || config.package != "fusion-std"
}

fn materialize_async_poll_stack_roots(
    target_dir: &Path,
    config: &PipelineConfig,
    artifact: &Path,
) -> Result<Option<PathBuf>, String> {
    let explicit = config.async_poll_stack_roots_path.as_ref().cloned();
    let discovered = collect_generated_async_poll_stack_roots(artifact)?;
    if explicit.is_none() && discovered.is_empty() {
        return Ok(None);
    }

    let output_path = target_dir.join(DEFAULT_ASYNC_POLL_STACK_ROOTS_NAME);
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
    }

    let mut rendered = String::new();
    if let Some(path) = explicit {
        rendered.push_str(
            &fs::read_to_string(&path)
                .map_err(|error| format!("failed to read {}: {error}", path.display()))?,
        );
        if !rendered.ends_with('\n') && !rendered.is_empty() {
            rendered.push('\n');
        }
    }
    rendered.push_str(&discovered);
    fs::write(&output_path, rendered)
        .map_err(|error| format!("failed to write {}: {error}", output_path.display()))?;
    Ok(Some(output_path))
}

fn collect_named_roots_from_emitter(
    workspace_root: &Path,
    toolchain: &str,
) -> Result<String, String> {
    let mut command = cargo_command(toolchain);
    command
        .current_dir(workspace_root)
        .arg("run")
        .arg("-p")
        .arg("fusion-std")
        .arg("--bin")
        .arg("fusion_std_fiber_task_roots")
        .arg("--quiet");
    let output = command
        .output()
        .map_err(|error| format!("failed to run roots emitter: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "roots emitter exited with status {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    String::from_utf8(output.stdout)
        .map_err(|error| format!("roots emitter produced non-utf8 output: {error}"))
}

fn find_target_artifact(target_dir: &Path, config: &PipelineConfig) -> Result<PathBuf, String> {
    match &config.target_artifact {
        TargetArtifact::Lib | TargetArtifact::Bin(_) => find_library_object(
            target_dir,
            config.profile,
            config.target.as_deref(),
            &config.crate_name,
        ),
    }
}

fn collect_auxiliary_runtime_artifacts(
    target_dir: &Path,
    config: &PipelineConfig,
    primary_artifact: &Path,
) -> Result<Vec<PathBuf>, String> {
    if !matches!(config.target_artifact, TargetArtifact::Bin(_)) {
        return Ok(Vec::new());
    }
    let deps_dir = artifact_deps_dir(target_dir, config.profile, config.target.as_deref());
    let entries = fs::read_dir(&deps_dir)
        .map_err(|error| format!("failed to read {}: {error}", deps_dir.display()))?;
    let mut artifacts = Vec::new();
    for entry in entries {
        let entry =
            entry.map_err(|error| format!("failed to scan {}: {error}", deps_dir.display()))?;
        let path = entry.path();
        let is_runtime_archive = path.extension().is_some_and(|ext| ext == "rlib")
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("libfusion_"));
        if !is_runtime_archive || path == primary_artifact {
            continue;
        }
        artifacts.push(path);
    }
    artifacts.extend(collect_target_sysroot_artifacts(config)?);
    artifacts.sort();
    artifacts.dedup();
    Ok(artifacts)
}

fn collect_target_sysroot_artifacts(config: &PipelineConfig) -> Result<Vec<PathBuf>, String> {
    let Some(target) = config.target.as_ref() else {
        return Ok(Vec::new());
    };
    let sysroot = rustc_sysroot(&config.toolchain)?;
    let target_lib_dir = sysroot.join("lib").join("rustlib").join(target).join("lib");
    let entries = fs::read_dir(&target_lib_dir)
        .map_err(|error| format!("failed to read {}: {error}", target_lib_dir.display()))?;
    let mut artifacts = Vec::new();
    for entry in entries {
        let entry = entry
            .map_err(|error| format!("failed to scan {}: {error}", target_lib_dir.display()))?;
        let path = entry.path();
        let is_runtime_archive = path.extension().is_some_and(|ext| ext == "rlib")
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    name.starts_with("libcore-")
                        || name.starts_with("liballoc-")
                        || name.starts_with("libcompiler_builtins-")
                        || name.starts_with("libpanic_abort-")
                });
        if is_runtime_archive {
            artifacts.push(path);
        }
    }
    Ok(artifacts)
}

fn rustc_sysroot(toolchain: &str) -> Result<PathBuf, String> {
    let output = Command::new("rustup")
        .arg("run")
        .arg(toolchain)
        .arg("rustc")
        .arg("--print")
        .arg("sysroot")
        .output()
        .map_err(|error| format!("failed to run rustc --print sysroot: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "rustc --print sysroot exited with status {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let sysroot = String::from_utf8(output.stdout)
        .map_err(|error| format!("rustc --print sysroot produced non-utf8 output: {error}"))?;
    Ok(PathBuf::from(sysroot.trim()))
}

fn artifact_deps_dir(target_dir: &Path, profile: BuildProfile, target: Option<&str>) -> PathBuf {
    target.map_or_else(
        || target_dir.join(profile.dir_name()).join("deps"),
        |triple| {
            target_dir
                .join(triple)
                .join(profile.dir_name())
                .join("deps")
        },
    )
}

fn find_library_object(
    target_dir: &Path,
    profile: BuildProfile,
    target: Option<&str>,
    crate_name: &str,
) -> Result<PathBuf, String> {
    let deps_dir = artifact_deps_dir(target_dir, profile, target);
    let entries = fs::read_dir(&deps_dir)
        .map_err(|error| format!("failed to read {}: {error}", deps_dir.display()))?;
    let mut newest = None::<(SystemTime, PathBuf)>;
    let object_prefix = format!("{crate_name}-");

    for entry in entries {
        let entry =
            entry.map_err(|error| format!("failed to scan {}: {error}", deps_dir.display()))?;
        let path = entry.path();
        let is_fusion_std_object = path.extension().is_some_and(|ext| ext == "o")
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(&object_prefix));
        if !is_fusion_std_object {
            continue;
        }

        let modified = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .map_err(|error| format!("failed to read metadata for {}: {error}", path.display()))?;
        match newest {
            Some((best_modified, _)) if modified <= best_modified => {}
            _ => newest = Some((modified, path)),
        }
    }

    newest.map(|(_, path)| path).ok_or_else(|| {
        format!(
            "failed to locate `{crate_name}` object artifact under {}",
            deps_dir.display(),
        )
    })
}

fn collect_generated_closure_roots(artifact: &Path, crate_name: &str) -> Result<String, String> {
    let symbol_index = load_artifact_symbol_index(artifact)?;
    let call_graph = load_artifact_call_graph(artifact)?;
    let mut roots = Vec::<(String, String)>::new();
    let crate_prefix = format!("{crate_name}::");
    for caller in symbol_index.entries.iter().filter(|entry| {
        entry
            .normalized_demangled
            .contains(GENERATED_CLOSURE_ROOT_SYMBOL_PREFIX)
    }) {
        let Some(callees) = call_graph.get(&caller.raw) else {
            roots.push((
                synthetic_generated_closure_root_type_name(crate_name, &caller.raw),
                caller.raw.clone(),
            ));
            continue;
        };
        let mut discovered = false;
        for callee in callees {
            let Some(metadata) = symbol_index.metadata_for_raw(callee) else {
                continue;
            };
            if !metadata.normalized_demangled.contains("{{closure}}") {
                continue;
            }
            if !metadata.normalized_demangled.starts_with(&crate_prefix) {
                continue;
            }
            roots.push((metadata.normalized_demangled.clone(), metadata.raw.clone()));
            discovered = true;
        }
        if !discovered {
            roots.push((
                synthetic_generated_closure_root_type_name(crate_name, &caller.raw),
                caller.raw.clone(),
            ));
        }
    }
    roots.sort();
    roots.dedup();

    let mut rendered = String::new();
    for (type_name, symbol) in roots {
        rendered.push_str(&type_name);
        rendered.push_str(" = ");
        rendered.push_str(&symbol);
        rendered.push('\n');
    }
    Ok(rendered)
}

fn synthetic_generated_closure_root_type_name(crate_name: &str, raw_symbol: &str) -> String {
    let suffix = raw_symbol
        .rsplit_once("::h")
        .map(|(_, hash)| hash)
        .or_else(|| raw_symbol.rsplit_once("17h").map(|(_, hash)| hash))
        .map(|hash| hash.trim_end_matches('E'))
        .filter(|hash| !hash.is_empty())
        .unwrap_or("unknown");
    format!("{crate_name}::__generated_closure_root__{suffix}::{{{{closure}}}}")
}

fn collect_generated_async_poll_stack_roots(artifact: &Path) -> Result<String, String> {
    let symbol_index = load_artifact_symbol_index(artifact)?;
    let call_graph = load_artifact_call_graph(artifact)?;
    let roots = collect_generated_async_poll_stack_root_entries(&symbol_index, &call_graph);

    let mut rendered = String::new();
    for (type_name, symbol) in roots {
        rendered.push_str(&type_name);
        rendered.push_str(" = ");
        rendered.push_str(&symbol);
        rendered.push('\n');
    }
    Ok(rendered)
}

fn extract_async_future_type_from_poll_symbol(symbol: &str) -> Option<&str> {
    let symbol = symbol.strip_prefix('<')?;
    let symbol = symbol.strip_suffix(" as core::future::future::Future>::poll")?;
    Some(symbol)
}

fn collect_generated_async_poll_stack_root_entries(
    symbol_index: &ArtifactSymbolIndex,
    call_graph: &BTreeMap<String, Vec<String>>,
) -> Vec<(String, String)> {
    let mut roots = Vec::<(String, String)>::new();
    for caller in symbol_index.entries.iter().filter(|entry| {
        entry
            .normalized_demangled
            .contains(GENERATED_ASYNC_POLL_STACK_ROOT_SYMBOL_PREFIX)
    }) {
        let Some(callees) = call_graph.get(&caller.raw) else {
            continue;
        };
        for callee in callees {
            let Some(metadata) = symbol_index.metadata_for_raw(callee) else {
                continue;
            };
            let Some(type_name) =
                extract_async_future_type_from_poll_symbol(&metadata.normalized_demangled)
            else {
                continue;
            };
            roots.push((type_name.to_owned(), metadata.raw.clone()));
        }
    }
    roots.sort();
    roots.dedup();
    roots
}

#[derive(Debug)]
struct ArtifactSymbolIndex {
    entries: Vec<ArtifactSymbolEntry>,
}

#[derive(Debug)]
struct ArtifactSymbolEntry {
    raw: String,
    normalized_demangled: String,
}

fn load_artifact_symbol_index(path: &Path) -> Result<ArtifactSymbolIndex, String> {
    let raw_output = run_tool("llvm-nm", ["--defined-only", "--format=just-symbols"], path)?;
    let demangled_output = run_tool(
        "llvm-nm",
        ["-C", "--defined-only", "--format=just-symbols"],
        path,
    )?;
    parse_llvm_nm_symbol_index(&raw_output, &demangled_output)
}

fn load_artifact_call_graph(path: &Path) -> Result<BTreeMap<String, Vec<String>>, String> {
    let output = run_tool("llvm-objdump", ["-dr", "--no-show-raw-insn"], path)?;
    Ok(parse_llvm_objdump_call_graph(&output))
}

fn run_tool<const N: usize>(tool: &str, args: [&str; N], path: &Path) -> Result<String, String> {
    let output = Command::new(tool)
        .args(args)
        .arg(path)
        .output()
        .map_err(|error| format!("failed to run {tool} on {}: {error}", path.display()))?;
    if !output.status.success() {
        return Err(format!(
            "{tool} exited with status {} for {}: {}",
            output.status,
            path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    String::from_utf8(output.stdout).map_err(|error| {
        format!(
            "{tool} produced non-utf8 output for {}: {error}",
            path.display()
        )
    })
}

fn parse_llvm_nm_symbol_index(
    raw_output: &str,
    demangled_output: &str,
) -> Result<ArtifactSymbolIndex, String> {
    let raw_symbols = raw_output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    let demangled_symbols = demangled_output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();

    if raw_symbols.len() != demangled_symbols.len() {
        return Err(format!(
            "llvm-nm symbol streams differ in length (raw={}, demangled={})",
            raw_symbols.len(),
            demangled_symbols.len()
        ));
    }

    let entries = raw_symbols
        .into_iter()
        .zip(demangled_symbols)
        .map(|(raw, demangled)| ArtifactSymbolEntry {
            raw: raw.to_owned(),
            normalized_demangled: normalize_demangled_symbol(demangled),
        })
        .collect::<Vec<_>>();
    Ok(ArtifactSymbolIndex { entries })
}

fn normalize_demangled_symbol(symbol: &str) -> String {
    let trimmed = if let Some((prefix, suffix)) = symbol.rsplit_once("::h") {
        if suffix.len() == 16 && suffix.chars().all(|ch| ch.is_ascii_hexdigit()) {
            prefix
        } else {
            symbol
        }
    } else {
        symbol
    };
    trimmed.replace("_$u7b$$u7b$closure$u7d$$u7d$", "{{closure}}")
}

impl ArtifactSymbolIndex {
    fn metadata_for_raw<'a>(&'a self, raw: &str) -> Option<&'a ArtifactSymbolEntry> {
        self.entries.iter().find(|entry| entry.raw == raw)
    }
}

fn parse_llvm_objdump_call_graph(contents: &str) -> BTreeMap<String, Vec<String>> {
    let mut call_graph = BTreeMap::<String, Vec<String>>::new();
    let mut current_function: Option<String> = None;
    let mut last_instruction_was_call = false;
    let mut pending_direct_target: Option<String> = None;

    for raw_line in contents.lines() {
        let line = raw_line.trim_end();
        if let Some(function) = parse_objdump_function_header(line) {
            flush_pending_direct_call(
                &mut call_graph,
                current_function.as_deref(),
                pending_direct_target.take(),
            );
            current_function = Some(function.to_owned());
            last_instruction_was_call = false;
            continue;
        }

        if let Some(target) = parse_objdump_relocation_target(line) {
            if last_instruction_was_call && let Some(caller) = current_function.as_ref() {
                record_call_edge(&mut call_graph, caller, target);
            }
            pending_direct_target = None;
            last_instruction_was_call = false;
            continue;
        }

        if let Some(mnemonic) = parse_objdump_instruction_mnemonic(line) {
            flush_pending_direct_call(
                &mut call_graph,
                current_function.as_deref(),
                pending_direct_target.take(),
            );
            if instruction_maybe_calls(mnemonic) {
                pending_direct_target =
                    parse_objdump_inline_call_target(line).map(ToOwned::to_owned);
                last_instruction_was_call = true;
            } else {
                pending_direct_target = None;
                last_instruction_was_call = false;
            }
            continue;
        }

        if line.trim().is_empty() {
            flush_pending_direct_call(
                &mut call_graph,
                current_function.as_deref(),
                pending_direct_target.take(),
            );
            last_instruction_was_call = false;
        }
    }

    flush_pending_direct_call(
        &mut call_graph,
        current_function.as_deref(),
        pending_direct_target,
    );
    call_graph
}

fn record_call_edge(call_graph: &mut BTreeMap<String, Vec<String>>, caller: &str, target: &str) {
    let entry = call_graph.entry(caller.to_owned()).or_default();
    if !entry.iter().any(|existing| existing == target) {
        entry.push(target.to_owned());
    }
}

fn flush_pending_direct_call(
    call_graph: &mut BTreeMap<String, Vec<String>>,
    caller: Option<&str>,
    target: Option<String>,
) {
    if let (Some(caller), Some(target)) = (caller, target) {
        record_call_edge(call_graph, caller, &target);
    }
}

fn parse_objdump_function_header(line: &str) -> Option<&str> {
    let (_, rest) = line.split_once('<')?;
    rest.strip_suffix(">:")
}

fn parse_objdump_instruction_mnemonic(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    if trimmed.is_empty()
        || trimmed.starts_with('/')
        || trimmed.starts_with("Disassembly of section ")
        || trimmed.starts_with('<')
    {
        return None;
    }

    let (address, rest) = trimmed.split_once(':')?;
    if address.is_empty() || !address.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return None;
    }
    rest.split_whitespace().next()
}

fn instruction_maybe_calls(mnemonic: &str) -> bool {
    mnemonic.starts_with("call") || matches!(mnemonic, "bl" | "blx" | "jal" | "jalr")
}

fn parse_objdump_relocation_target(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    if !trimmed.starts_with(|ch: char| ch.is_ascii_hexdigit()) || !trimmed.contains("R_") {
        return None;
    }
    let target = trimmed.split_whitespace().last()?;
    normalize_relocation_target(target)
}

fn parse_objdump_inline_call_target(line: &str) -> Option<&str> {
    let (_, target) = line.rsplit_once('<')?;
    let target = target.strip_suffix('>')?;
    normalize_relocation_target(target)
}

fn normalize_relocation_target(target: &str) -> Option<&str> {
    let mut target = target.trim();
    if target.is_empty() {
        return None;
    }
    if let Some(stripped) = target.strip_prefix(".text.") {
        target = stripped;
    }
    let end = target.find(['@', '+', '-']).unwrap_or(target.len());
    let normalized = &target[..end];
    (!normalized.is_empty()).then_some(normalized)
}

fn run_analyzer(
    workspace_root: &Path,
    config: &PipelineConfig,
    roots_path: &Path,
    async_poll_stack_roots_path: Option<&Path>,
    artifact: &Path,
    auxiliary_artifacts: &[PathBuf],
) -> Result<(), String> {
    let mut command = cargo_command(&config.toolchain);
    command
        .current_dir(workspace_root)
        .arg("run")
        .arg("-p")
        .arg("fusion-std")
        .arg("--bin")
        .arg("fusion_std_fiber_task_analyzer")
        .arg("--")
        .arg(roots_path)
        .arg(artifact)
        .arg(&config.output_path)
        .arg("--report")
        .arg(&config.report_path)
        .arg("--rust-contracts")
        .arg(&config.rust_contracts_path)
        .arg("--red-inline-rust")
        .arg(&config.red_inline_rust_path)
        .arg("--crate-name")
        .arg(&config.crate_name);
    for path in auxiliary_artifacts {
        command.arg("--aux-artifact").arg(path);
    }
    if let Some(contracts_path) = config.contracts_path.as_ref() {
        command.arg("--contracts").arg(contracts_path);
    }
    if let Some(red_inline_contracts_path) = config.red_inline_contracts_path.as_ref() {
        command
            .arg("--red-inline-contracts")
            .arg(red_inline_contracts_path);
    }
    if let Some(async_poll_stack_roots_path) = async_poll_stack_roots_path {
        command
            .arg("--async-poll-stack-roots")
            .arg(async_poll_stack_roots_path)
            .arg("--async-poll-stack-output")
            .arg(&config.async_poll_stack_output_path)
            .arg("--async-poll-stack-rust")
            .arg(&config.async_poll_stack_rust_path);
    }
    run_command(command, "cargo run analyzer")
}

fn cargo_command(toolchain: &str) -> Command {
    let mut command = Command::new("rustup");
    command.arg("run").arg(toolchain).arg("cargo");
    command
}

fn run_command(mut command: Command, label: &str) -> Result<(), String> {
    let output = command
        .output()
        .map_err(|error| format!("failed to run {label}: {error}"))?;
    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(format!(
        "{label} exited with status {}: {}",
        output.status,
        stderr.trim()
    ))
}

fn usage(reason: &str) -> String {
    format!(
        "{reason}\nusage: cargo run -p fusion-std --bin fusion_std_fiber_task_pipeline -- [--manifest-path <path>] [--roots <path>] [--contracts <path>] [--red-inline-contracts <path>] [--async-poll-stack-roots <path>] [--report <path>] [--rust-contracts <path>] [--red-inline-rust <path>] [--output <path>] [--async-poll-stack-output <path>] [--async-poll-stack-rust <path>] [--toolchain <channel>] [--package <name>] [--crate-name <name>] [--bin <name> | --lib] [--profile <dev|release>] [--target <triple>] [--features <csv>] [--no-default-features]\n\nWhen --roots is omitted, the pipeline merges fusion-std's hidden generated-task root registry (for the fusion-std lib target) with generated closure roots discovered from the analyzed artifact."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_closure_marker_from_llvm_nm_output() {
        let symbol = "fusion_std::thread::fiber::generated_closure_task_metadata_anchor::_$u7b$$u7b$closure$u7d$$u7d$::h4dec999d945aad9d";
        assert_eq!(
            normalize_demangled_symbol(symbol),
            "fusion_std::thread::fiber::generated_closure_task_metadata_anchor::{{closure}}"
        );
    }

    #[test]
    fn parses_objdump_direct_call_graph_for_generated_closure_root() {
        let graph = parse_llvm_objdump_call_graph(
            "00000000 <_ROOT_>:\n   0:       bl      <_CLOSURE_>\n\n00000004 <_CLOSURE_>:\n   4:       bx      lr\n",
        );
        assert_eq!(graph.get("_ROOT_"), Some(&vec!["_CLOSURE_".to_owned()]));
    }

    #[test]
    fn collects_generated_closure_root_callee_symbols() {
        let symbol_index = ArtifactSymbolIndex {
            entries: vec![
                ArtifactSymbolEntry {
                    raw: "_ROOT_".to_owned(),
                    normalized_demangled: "fusion_std::thread::fiber::generated_closure_task_root"
                        .to_owned(),
                },
                ArtifactSymbolEntry {
                    raw: "_CLOSURE_".to_owned(),
                    normalized_demangled: "fusion_example_pico::main::{{closure}}".to_owned(),
                },
            ],
        };
        let mut call_graph = BTreeMap::new();
        call_graph.insert("_ROOT_".to_owned(), vec!["_CLOSURE_".to_owned()]);

        let mut roots = Vec::<(String, String)>::new();
        for caller in symbol_index.entries.iter().filter(|entry| {
            entry
                .normalized_demangled
                .contains(GENERATED_CLOSURE_ROOT_SYMBOL_PREFIX)
        }) {
            let Some(callees) = call_graph.get(&caller.raw) else {
                continue;
            };
            for callee in callees {
                let Some(metadata) = symbol_index.metadata_for_raw(callee) else {
                    continue;
                };
                if !metadata.normalized_demangled.contains("{{closure}}") {
                    continue;
                }
                roots.push((metadata.normalized_demangled.clone(), metadata.raw.clone()));
            }
        }

        assert_eq!(
            roots,
            vec![(
                "fusion_example_pico::main::{{closure}}".to_owned(),
                "_CLOSURE_".to_owned(),
            )]
        );
    }

    #[test]
    fn extracts_async_future_type_from_poll_symbol() {
        assert_eq!(
            extract_async_future_type_from_poll_symbol(
                "<fusion_example_pico::main::{{closure}} as core::future::future::Future>::poll"
            ),
            Some("fusion_example_pico::main::{{closure}}")
        );
    }

    #[test]
    fn collects_generated_async_poll_stack_root_callee_symbols() {
        let symbol_index = ArtifactSymbolIndex {
            entries: vec![
                ArtifactSymbolEntry {
                    raw: "_ROOT_".to_owned(),
                    normalized_demangled:
                        "fusion_std::thread::executor::generated_async_poll_stack_root".to_owned(),
                },
                ArtifactSymbolEntry {
                    raw: "_POLL_".to_owned(),
                    normalized_demangled:
                        "<fusion_example_pico::main::{{closure}} as core::future::future::Future>::poll"
                            .to_owned(),
                },
            ],
        };
        let mut call_graph = BTreeMap::new();
        call_graph.insert("_ROOT_".to_owned(), vec!["_POLL_".to_owned()]);

        let roots = collect_generated_async_poll_stack_root_entries(&symbol_index, &call_graph);

        assert_eq!(
            roots,
            vec![(
                "fusion_example_pico::main::{{closure}}".to_owned(),
                "_POLL_".to_owned(),
            )]
        );
    }

    #[test]
    fn collects_generated_async_poll_stack_roots_for_external_future_types() {
        let symbol_index = ArtifactSymbolIndex {
            entries: vec![
                ArtifactSymbolEntry {
                    raw: "_ROOT_".to_owned(),
                    normalized_demangled:
                        "fusion_std::thread::executor::generated_async_poll_stack_root".to_owned(),
                },
                ArtifactSymbolEntry {
                    raw: "_EXTERNAL_POLL_".to_owned(),
                    normalized_demangled:
                        "<external_crate::task::ExternalFuture as core::future::future::Future>::poll"
                            .to_owned(),
                },
            ],
        };
        let mut call_graph = BTreeMap::new();
        call_graph.insert("_ROOT_".to_owned(), vec!["_EXTERNAL_POLL_".to_owned()]);

        let roots = collect_generated_async_poll_stack_root_entries(&symbol_index, &call_graph);

        assert_eq!(
            roots,
            vec![(
                "external_crate::task::ExternalFuture".to_owned(),
                "_EXTERNAL_POLL_".to_owned(),
            )]
        );
    }
}
