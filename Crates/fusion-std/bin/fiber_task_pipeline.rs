use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

const DEFAULT_OUTPUT_NAME: &str = "fusion-std-fiber-task.generated";
const DEFAULT_GENERATED_ROOTS_NAME: &str = "fusion-std-fiber-task.roots";
const DEFAULT_REPORT_NAME: &str = "fusion-std-fiber-task.report";
const DEFAULT_RUST_CONTRACTS_NAME: &str = "fusion-std-fiber-task.contracts.rs";
const DEFAULT_CONTRACTS_NAME: &str = "fiber-task.contracts";
const DEFAULT_RED_INLINE_CONTRACTS_NAME: &str = "red-inline.contracts";
const DEFAULT_RED_INLINE_RUST_NAME: &str = "fusion-std-red-inline.contracts.rs";

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
    let target_dir = workspace_root.join("target").join("fiber-task-pipeline");
    let roots_path = materialize_roots(&workspace_root, &target_dir, &config)?;

    build_fusion_std_artifact(&workspace_root, &target_dir, &config)?;
    let artifact = find_fusion_std_object(&target_dir, config.profile, config.target.as_deref())?;
    run_analyzer(&workspace_root, &config, &roots_path, &artifact)?;
    Ok(())
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

#[derive(Debug)]
struct PipelineConfig {
    roots_path: Option<PathBuf>,
    contracts_path: Option<PathBuf>,
    red_inline_contracts_path: Option<PathBuf>,
    report_path: PathBuf,
    rust_contracts_path: PathBuf,
    red_inline_rust_path: PathBuf,
    output_path: PathBuf,
    toolchain: String,
    crate_name: String,
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
        let mut contracts_path = manifest_dir
            .join(DEFAULT_CONTRACTS_NAME)
            .is_file()
            .then(|| manifest_dir.join(DEFAULT_CONTRACTS_NAME));
        let mut red_inline_contracts_path = manifest_dir
            .join(DEFAULT_RED_INLINE_CONTRACTS_NAME)
            .is_file()
            .then(|| manifest_dir.join(DEFAULT_RED_INLINE_CONTRACTS_NAME));
        let mut report_path = workspace_root.join("target").join(DEFAULT_REPORT_NAME);
        let mut rust_contracts_path = workspace_root
            .join("target")
            .join(DEFAULT_RUST_CONTRACTS_NAME);
        let mut red_inline_rust_path = workspace_root
            .join("target")
            .join(DEFAULT_RED_INLINE_RUST_NAME);
        let mut output_path = workspace_root.join("target").join(DEFAULT_OUTPUT_NAME);
        let mut toolchain = load_workspace_toolchain(workspace_root)?;
        let mut crate_name = "fusion_std".to_owned();
        let mut profile = BuildProfile::Dev;
        let mut target = None;
        let mut features = None;
        let mut no_default_features = false;

        while let Some(arg) = args.next() {
            apply_cli_arg(
                current_dir,
                &mut args,
                &arg.to_string_lossy(),
                &mut roots_path,
                &mut contracts_path,
                &mut red_inline_contracts_path,
                &mut report_path,
                &mut rust_contracts_path,
                &mut red_inline_rust_path,
                &mut output_path,
                &mut toolchain,
                &mut crate_name,
                &mut profile,
                &mut target,
                &mut features,
                &mut no_default_features,
            )?;
        }

        Ok(Self {
            roots_path,
            contracts_path,
            red_inline_contracts_path,
            report_path,
            rust_contracts_path,
            red_inline_rust_path,
            output_path,
            toolchain,
            crate_name,
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
    roots_path: &mut Option<PathBuf>,
    contracts_path: &mut Option<PathBuf>,
    red_inline_contracts_path: &mut Option<PathBuf>,
    report_path: &mut PathBuf,
    rust_contracts_path: &mut PathBuf,
    red_inline_rust_path: &mut PathBuf,
    output_path: &mut PathBuf,
    toolchain: &mut String,
    crate_name: &mut String,
    profile: &mut BuildProfile,
    target: &mut Option<String>,
    features: &mut Option<String>,
    no_default_features: &mut bool,
) -> Result<(), String> {
    match arg {
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
        "--crate-name" => {
            *crate_name = args
                .next()
                .ok_or_else(|| usage("missing value for --crate-name"))?
                .to_string_lossy()
                .into_owned();
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

fn build_fusion_std_artifact(
    workspace_root: &Path,
    target_dir: &Path,
    config: &PipelineConfig,
) -> Result<(), String> {
    let mut command = cargo_command(&config.toolchain);
    command
        .current_dir(workspace_root)
        .env("CARGO_INCREMENTAL", "0")
        .arg("rustc")
        .arg("-p")
        .arg("fusion-std")
        .arg("--lib")
        .arg("--target-dir")
        .arg(target_dir);

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

    command
        .arg("--")
        .arg("-Z")
        .arg("emit-stack-sizes")
        .arg("--emit=obj");
    run_command(command, "cargo rustc")
}

fn materialize_roots(
    workspace_root: &Path,
    target_dir: &Path,
    config: &PipelineConfig,
) -> Result<PathBuf, String> {
    if let Some(path) = config.roots_path.as_ref() {
        return Ok(path.clone());
    }

    let output_path = target_dir.join(DEFAULT_GENERATED_ROOTS_NAME);
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
    }

    let mut command = cargo_command(&config.toolchain);
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
    fs::write(&output_path, &output.stdout)
        .map_err(|error| format!("failed to write {}: {error}", output_path.display()))?;
    Ok(output_path)
}

fn find_fusion_std_object(
    target_dir: &Path,
    profile: BuildProfile,
    target: Option<&str>,
) -> Result<PathBuf, String> {
    let deps_dir = target.map_or_else(
        || target_dir.join(profile.dir_name()).join("deps"),
        |triple| {
            target_dir
                .join(triple)
                .join(profile.dir_name())
                .join("deps")
        },
    );
    let entries = fs::read_dir(&deps_dir)
        .map_err(|error| format!("failed to read {}: {error}", deps_dir.display()))?;
    let mut newest = None::<(SystemTime, PathBuf)>;

    for entry in entries {
        let entry =
            entry.map_err(|error| format!("failed to scan {}: {error}", deps_dir.display()))?;
        let path = entry.path();
        let is_fusion_std_object = path.extension().is_some_and(|ext| ext == "o")
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("fusion_std-"));
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
            "failed to locate fusion-std object artifact under {}",
            deps_dir.display()
        )
    })
}

fn run_analyzer(
    workspace_root: &Path,
    config: &PipelineConfig,
    roots_path: &Path,
    artifact: &Path,
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
    if let Some(contracts_path) = config.contracts_path.as_ref() {
        command.arg("--contracts").arg(contracts_path);
    }
    if let Some(red_inline_contracts_path) = config.red_inline_contracts_path.as_ref() {
        command
            .arg("--red-inline-contracts")
            .arg(red_inline_contracts_path);
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
        "{reason}\nusage: cargo run -p fusion-std --bin fusion_std_fiber_task_pipeline -- [--roots <path>] [--contracts <path>] [--red-inline-contracts <path>] [--report <path>] [--rust-contracts <path>] [--red-inline-rust <path>] [--output <path>] [--toolchain <channel>] [--crate-name <name>] [--profile <dev|release>] [--target <triple>] [--features <csv>] [--no-default-features]\n\nWhen --roots is omitted, the pipeline derives analyzer roots from fusion-std's hidden generated-task root registry."
    )
}
