//! Temporary build-time bridge for generated Fiber/Async contract metadata.
//!
//! This file is not intended to be one permanent architectural center of gravity.
//! Its current job is narrower:
//! - emit generated fiber-task contract sidecars into `OUT_DIR`
//! - emit generated async poll-stack contract sidecars into `OUT_DIR`
//! - let `fusion-std` include those sidecars so generated exact contracts can participate in
//!   ordinary library builds today
//!
//! What this means in practice today:
//! - generated analyzer metadata is currently treated as required build input
//! - this build script will bootstrap the analyzer sidecars into the active build graph when they
//!   are missing so one clean checkout still builds
//! - missing or mismatched analyzer artifacts are hard build failures
//! - this is intentionally stricter than the long-term architecture because silent fallback here
//!   hides build-graph and metadata-pipeline mistakes
//!
//! Why this still smells:
//! - this is solution/build-pipeline work living in one library crate
//! - it discovers analyzer artifacts through environment variables and active-build-directory
//!   scanning
//! - it reconstructs compiler-owned truth from outside `rustc`/`cargo`
//! - it is therefore a compatibility bridge, not the final model
//!
//! What would be required for this to universally "Just Works":
//! - `rustc` must emit Fusion metadata as first-class compiler artifacts:
//!   - generated fiber task contracts
//!   - generated async poll-stack contracts
//!   - exact future/output layout truth where relevant
//! - `cargo` must understand and propagate those artifacts through the dependency graph
//! - downstream crates must not need:
//!   - `OUT_DIR` sidecar discovery
//!   - target-dir scavenging
//!   - environment-variable handoff
//!   - ad hoc build-script coordination
//! - strict/critical-safe builds must be able to demand exact generated contracts directly from
//!   compiler/toolchain outputs rather than from this build-script bridge
//!
//! Until that toolchain work exists, this file is tolerated debt. It should not grow new
//! responsibilities, and anything that is platform/board/linker truth belongs below this layer.
//! `memory.x` generation has already been moved down into `fusion-pal/build.rs` for exactly that
//! reason.

use std::collections::BTreeMap;
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::{
    Path,
    PathBuf,
};

const AUTO_MANIFEST_NAME: &str = "fusion-std-fiber-task.generated";
const AUTO_REPORT_NAME: &str = "fusion-std-fiber-task.report";
const AUTO_RUST_CONTRACTS_NAME: &str = "fusion-std-fiber-task.contracts.rs";
const AUTO_RED_INLINE_RUST_NAME: &str = "fusion-std-red-inline.contracts.rs";
const OUTPUT_NAME: &str = "fiber_task_generated.rs";
const ASYNC_POLL_STACK_OUTPUT_NAME: &str = "async_task_generated.rs";
const AUTO_ASYNC_POLL_STACK_MANIFEST_NAME: &str = "fusion-std-async-poll-stack.generated";
const AUTO_ASYNC_POLL_STACK_RUST_NAME: &str = "fusion-std-async-poll-stack.contracts.rs";
const GENERATED_METADATA_ENV: &str = "FUSION_FIBER_TASK_METADATA";
const GENERATED_REPORT_ENV: &str = "FUSION_FIBER_TASK_REPORT";
const GENERATED_ASYNC_POLL_STACK_METADATA_ENV: &str = "FUSION_ASYNC_POLL_STACK_METADATA";
const STRICT_CONTRACTS_FEATURE_ENV: &str = "CARGO_FEATURE_CRITICAL_SAFE";
const GENERATED_ASYNC_POLL_STACK_ANCHOR_TYPE_NAME: &str =
    "fusion_std::thread::executor::GeneratedAsyncPollStackMetadataAnchorFuture";
const GENERATED_ASYNC_POLL_STACK_ANCHOR_BYTES: usize = 1536;

#[derive(Debug, Clone)]
struct GeneratedFiberTaskEntry {
    type_name: String,
    stack_bytes: usize,
    priority: i8,
    execution: GeneratedFiberTaskExecution,
}

#[derive(Debug, Clone)]
struct GeneratedAsyncPollStackEntry {
    type_name: String,
    poll_stack_bytes: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GeneratedFiberTaskExecution {
    Fiber,
    InlineNoYield,
}

impl GeneratedFiberTaskExecution {
    const fn render(self) -> &'static str {
        match self {
            Self::Fiber => "FiberTaskExecution::Fiber",
            Self::InlineNoYield => "FiberTaskExecution::InlineNoYield",
        }
    }
}

fn parse_generated_execution(
    raw: &str,
    line_no: usize,
) -> Result<GeneratedFiberTaskExecution, String> {
    match raw {
        "" | "fiber" => Ok(GeneratedFiberTaskExecution::Fiber),
        "inline-no-yield" => Ok(GeneratedFiberTaskExecution::InlineNoYield),
        other => Err(format!(
            "line {} has unsupported execution kind `{other}`",
            line_no + 1
        )),
    }
}

fn main() {
    let (auto_manifest_candidates, auto_report_candidates, auto_async_poll_stack_candidates) =
        setup_build_inputs();
    let output_path =
        PathBuf::from(env::var("OUT_DIR").expect("Cargo should provide OUT_DIR")).join(OUTPUT_NAME);
    let generated =
        generate_fiber_task_metadata(&auto_manifest_candidates, &auto_report_candidates);
    fs::write(&output_path, generated).unwrap_or_else(|error| {
        panic!(
            "fusion-std: failed to write {}: {error}",
            output_path.display()
        )
    });
    let async_output_path =
        PathBuf::from(env::var("OUT_DIR").expect("Cargo should provide OUT_DIR"))
            .join(ASYNC_POLL_STACK_OUTPUT_NAME);
    let generated_async_poll_stack =
        generate_async_poll_stack_metadata(&auto_async_poll_stack_candidates);
    fs::write(&async_output_path, generated_async_poll_stack).unwrap_or_else(|error| {
        panic!(
            "fusion-std: failed to write {}: {error}",
            async_output_path.display()
        )
    });
}

fn setup_build_inputs() -> (Vec<PathBuf>, Vec<PathBuf>, Vec<PathBuf>) {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed={GENERATED_METADATA_ENV}");
    println!("cargo:rerun-if-env-changed={GENERATED_REPORT_ENV}");
    println!("cargo:rerun-if-env-changed={GENERATED_ASYNC_POLL_STACK_METADATA_ENV}");
    println!("cargo:rerun-if-env-changed={STRICT_CONTRACTS_FEATURE_ENV}");
    println!("cargo:rerun-if-env-changed=CARGO_BUILD_TARGET_DIR");
    println!("cargo:rerun-if-env-changed=CARGO_TARGET_DIR");

    let manifest_dir = PathBuf::from(
        env::var("CARGO_MANIFEST_DIR").expect("Cargo should always provide CARGO_MANIFEST_DIR"),
    );
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("Cargo should provide OUT_DIR"));
    let auto_manifest_candidates =
        candidate_auto_artifact_paths(&manifest_dir, &out_dir, AUTO_MANIFEST_NAME);
    let auto_report_candidates =
        candidate_auto_artifact_paths(&manifest_dir, &out_dir, AUTO_REPORT_NAME);
    let auto_async_poll_stack_candidates =
        candidate_auto_artifact_paths(&manifest_dir, &out_dir, AUTO_ASYNC_POLL_STACK_MANIFEST_NAME);
    ensure_generated_artifacts(
        &manifest_dir,
        &out_dir,
        &auto_manifest_candidates,
        &auto_report_candidates,
        &auto_async_poll_stack_candidates,
    );
    let analyzer_metadata = env::var_os(GENERATED_METADATA_ENV).map(PathBuf::from);
    let analyzer_report = env::var_os(GENERATED_REPORT_ENV).map(PathBuf::from);
    let async_poll_stack_metadata =
        env::var_os(GENERATED_ASYNC_POLL_STACK_METADATA_ENV).map(PathBuf::from);
    if let Some(path) = analyzer_metadata.as_ref()
        && path.is_file()
    {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    if let Some(path) = analyzer_report.as_ref()
        && path.is_file()
    {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    if let Some(path) = async_poll_stack_metadata.as_ref()
        && path.is_file()
    {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    for path in &auto_manifest_candidates {
        if path.is_file() {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
    for path in &auto_report_candidates {
        if path.is_file() {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
    for path in &auto_async_poll_stack_candidates {
        if path.is_file() {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
    (
        auto_manifest_candidates,
        auto_report_candidates,
        auto_async_poll_stack_candidates,
    )
}

fn parse_linker_scalar(raw: &str) -> Result<usize, String> {
    let raw = raw.trim();
    if let Some(hex) = raw.strip_prefix("0x") {
        return usize::from_str_radix(hex, 16).map_err(|error| error.to_string());
    }

    let mut multiplier = 1_usize;
    let digits = if let Some(value) = raw.strip_suffix(['K', 'k']) {
        multiplier = 1024;
        value
    } else if let Some(value) = raw.strip_suffix(['M', 'm']) {
        multiplier = 1024 * 1024;
        value
    } else if let Some(value) = raw.strip_suffix(['G', 'g']) {
        multiplier = 1024 * 1024 * 1024;
        value
    } else {
        raw
    };

    let base = digits.parse::<usize>().map_err(|error| error.to_string())?;
    base.checked_mul(multiplier)
        .ok_or_else(|| format!("value `{raw}` exceeds usize"))
}

fn generate_fiber_task_metadata(
    auto_manifest_candidates: &[PathBuf],
    auto_report_candidates: &[PathBuf],
) -> String {
    let analyzer_metadata = env::var_os(GENERATED_METADATA_ENV).map(PathBuf::from);
    let analyzer_report = env::var_os(GENERATED_REPORT_ENV).map(PathBuf::from);
    let strict_generated_contracts = env::var_os(STRICT_CONTRACTS_FEATURE_ENV).is_some();
    let metadata_source = required_generated_artifact_source(
        analyzer_metadata.as_deref(),
        auto_manifest_candidates,
        GENERATED_METADATA_ENV,
        AUTO_MANIFEST_NAME,
        "fiber-task metadata",
    );
    let report_source = required_generated_artifact_source(
        analyzer_report.as_deref(),
        auto_report_candidates,
        GENERATED_REPORT_ENV,
        AUTO_REPORT_NAME,
        "fiber-task analyzer report",
    );
    assert_matching_generated_artifact_roots(
        "fiber-task metadata",
        metadata_source,
        "fiber-task analyzer report",
        report_source,
    );
    let mut entries = match load_generated_entries(metadata_source) {
        Ok(entries) => entries,
        Err(error) => panic!(
            "fusion-std: failed to load generated fiber-task metadata from {}: {error}",
            metadata_source.display()
        ),
    };
    if strict_generated_contracts {
        if let Err(error) = assert_report_has_no_unresolved_symbols(report_source) {
            panic!(
                "fusion-std: strict generated-task contracts rejected analyzer report {}: \
                 {error}",
                report_source.display()
            );
        }
    }

    entries.sort_by(|left, right| left.type_name.cmp(&right.type_name));
    render_generated_entries(&entries)
}

fn generate_async_poll_stack_metadata(auto_manifest_candidates: &[PathBuf]) -> String {
    let explicit_source = env::var_os(GENERATED_ASYNC_POLL_STACK_METADATA_ENV).map(PathBuf::from);
    let metadata_source = required_generated_artifact_source(
        explicit_source.as_deref(),
        auto_manifest_candidates,
        GENERATED_ASYNC_POLL_STACK_METADATA_ENV,
        AUTO_ASYNC_POLL_STACK_MANIFEST_NAME,
        "async poll-stack metadata",
    );
    let mut entries =
        load_generated_async_poll_stack_entries(metadata_source).unwrap_or_else(|error| {
            panic!(
                "fusion-std: failed to load generated async poll-stack metadata from {}: {error}",
                metadata_source.display()
            )
        });
    entries = merge_async_poll_stack_entries(entries);
    if !entries
        .iter()
        .any(|entry| entry.type_name == GENERATED_ASYNC_POLL_STACK_ANCHOR_TYPE_NAME)
    {
        entries.push(GeneratedAsyncPollStackEntry {
            type_name: GENERATED_ASYNC_POLL_STACK_ANCHOR_TYPE_NAME.to_owned(),
            poll_stack_bytes: GENERATED_ASYNC_POLL_STACK_ANCHOR_BYTES,
        });
    }
    entries.sort_by(|left, right| left.type_name.cmp(&right.type_name));
    render_generated_async_poll_stack_entries(&entries)
}

fn merge_async_poll_stack_entries(
    entries: Vec<GeneratedAsyncPollStackEntry>,
) -> Vec<GeneratedAsyncPollStackEntry> {
    let mut merged = BTreeMap::<String, usize>::new();
    for entry in entries {
        let budget = merged.entry(entry.type_name).or_insert(0);
        *budget = (*budget).max(entry.poll_stack_bytes);
    }
    merged
        .into_iter()
        .map(
            |(type_name, poll_stack_bytes)| GeneratedAsyncPollStackEntry {
                type_name,
                poll_stack_bytes,
            },
        )
        .collect()
}

fn candidate_auto_artifact_paths(
    manifest_dir: &Path,
    out_dir: &Path,
    artifact_name: &str,
) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    roots.extend(active_target_artifact_roots(out_dir));
    roots.extend(configured_target_artifact_roots(manifest_dir));

    if let Some(workspace_root) = workspace_root(manifest_dir) {
        roots.push(workspace_root.join("target"));
    }
    if let Some(parent) = manifest_dir.parent() {
        roots.push(parent.join("target"));
    }
    roots.push(PathBuf::from("target"));

    let mut candidates = Vec::new();
    for root in roots {
        if !candidates.contains(&root) {
            candidates.push(root.join(artifact_name));
        }
    }
    candidates
}

fn active_target_artifact_roots(out_dir: &Path) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let profile = env::var_os("PROFILE");
    let target = env::var_os("TARGET");
    // Cargo always gives us OUT_DIR, so use it to recover the active build graph before falling
    // back to broader target-dir guesses. This keeps dependency builds with their own target dir
    // from depending on workspace-root folklore.
    let active_profile_dir = profile
        .as_deref()
        .and_then(|profile_name| {
            out_dir
                .ancestors()
                .find(|ancestor| ancestor.file_name() == Some(profile_name))
        })
        .map(Path::to_path_buf)
        .or_else(|| out_dir.ancestors().nth(3).map(Path::to_path_buf));

    if let Some(profile_dir) = active_profile_dir {
        roots.push(profile_dir.clone());
        if let Some(parent) = profile_dir.parent() {
            if target_component_matches(parent.file_name(), target.as_deref()) {
                roots.push(parent.to_path_buf());
                if let Some(target_root) = parent.parent() {
                    roots.push(target_root.to_path_buf());
                }
            } else {
                roots.push(parent.to_path_buf());
            }
        }
    }

    roots
}

fn configured_target_artifact_roots(manifest_dir: &Path) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for env_name in ["CARGO_BUILD_TARGET_DIR", "CARGO_TARGET_DIR"] {
        let Some(target_dir) = env::var_os(env_name).map(PathBuf::from) else {
            continue;
        };
        if target_dir.is_absolute() {
            roots.push(target_dir);
        } else {
            roots.push(manifest_dir.join(&target_dir));
            if let Some(workspace_root) = workspace_root(manifest_dir) {
                roots.push(workspace_root.join(&target_dir));
            }
        }
    }
    roots
}

fn target_component_matches(component: Option<&OsStr>, target: Option<&OsStr>) -> bool {
    component.is_some() && component == target
}

fn workspace_root(manifest_dir: &Path) -> Option<&Path> {
    manifest_dir.parent().and_then(Path::parent)
}

fn first_existing_path(candidates: &[PathBuf]) -> Option<&PathBuf> {
    candidates.iter().find(|path| path.is_file())
}

fn ensure_generated_artifacts(
    _manifest_dir: &Path,
    _out_dir: &Path,
    auto_manifest_candidates: &[PathBuf],
    auto_report_candidates: &[PathBuf],
    auto_async_poll_stack_candidates: &[PathBuf],
) {
    let explicit_metadata = env::var_os(GENERATED_METADATA_ENV).map(PathBuf::from);
    let explicit_report = env::var_os(GENERATED_REPORT_ENV).map(PathBuf::from);
    let explicit_async = env::var_os(GENERATED_ASYNC_POLL_STACK_METADATA_ENV).map(PathBuf::from);
    let preferred_metadata =
        preferred_generated_artifact_path(auto_manifest_candidates, AUTO_MANIFEST_NAME);
    let preferred_report =
        preferred_generated_artifact_path(auto_report_candidates, AUTO_REPORT_NAME);
    let preferred_async = preferred_generated_artifact_path(
        auto_async_poll_stack_candidates,
        AUTO_ASYNC_POLL_STACK_MANIFEST_NAME,
    );

    if preferred_metadata.is_file() && preferred_report.is_file() && preferred_async.is_file() {
        return;
    }
    if explicit_metadata.is_some() || explicit_report.is_some() || explicit_async.is_some() {
        return;
    }

    bootstrap_generated_artifacts(preferred_metadata, preferred_report, preferred_async);
}

fn preferred_generated_artifact_path<'a>(
    candidates: &'a [PathBuf],
    artifact_name: &str,
) -> &'a Path {
    candidates.first().map(PathBuf::as_path).unwrap_or_else(|| {
        panic!("fusion-std: no candidate path available for generated artifact {artifact_name}")
    })
}

fn bootstrap_generated_artifacts(metadata_path: &Path, report_path: &Path, async_path: &Path) {
    let metadata_dir = metadata_path.parent().unwrap_or_else(|| {
        panic!(
            "fusion-std: metadata path {} has no parent",
            metadata_path.display()
        )
    });
    let report_dir = report_path.parent().unwrap_or_else(|| {
        panic!(
            "fusion-std: report path {} has no parent",
            report_path.display()
        )
    });
    let async_dir = async_path.parent().unwrap_or_else(|| {
        panic!(
            "fusion-std: async metadata path {} has no parent",
            async_path.display()
        )
    });
    if !(same_parent_directory(metadata_path, report_path)
        && same_parent_directory(metadata_path, async_path))
    {
        panic!(
            "fusion-std: generated artifact bootstrap expected metadata/report/async outputs to \
             share one directory, got {}, {}, {}",
            metadata_dir.display(),
            report_dir.display(),
            async_dir.display()
        );
    }
    fs::create_dir_all(metadata_dir).unwrap_or_else(|error| {
        panic!(
            "fusion-std: failed to create generated metadata directory {}: {error}",
            metadata_dir.display()
        )
    });
    write_bootstrap_file_if_missing(
        metadata_path,
        b"fusion_std::thread::fiber::GeneratedFiberTaskMetadataAnchorTask=8,5,inline-no-yield\n",
        "fiber-task metadata",
    );
    write_bootstrap_file_if_missing(
        report_path,
        b"# Generated by fusion-std build.rs bootstrap\n",
        "fiber-task analyzer report",
    );
    write_bootstrap_file_if_missing(
        async_path,
        format!(
            "{GENERATED_ASYNC_POLL_STACK_ANCHOR_TYPE_NAME}={GENERATED_ASYNC_POLL_STACK_ANCHOR_BYTES}\n"
        )
        .as_bytes(),
        "async poll-stack metadata",
    );
    write_bootstrap_file_if_missing(
        &metadata_dir.join(AUTO_RUST_CONTRACTS_NAME),
        br#"// Generated by fusion-std build.rs bootstrap
fusion_std::declare_generated_fiber_task_contract!(
    crate::thread::fiber::GeneratedFiberTaskMetadataAnchorTask,
    core::num::NonZeroUsize::new(8).unwrap(),
    fusion_std::thread::FiberTaskPriority::new(5),
    fusion_std::thread::FiberTaskExecution::InlineNoYield,
);
"#,
        "fiber-task Rust sidecar",
    );
    write_bootstrap_file_if_missing(
        &metadata_dir.join(AUTO_RED_INLINE_RUST_NAME),
        b"// Generated by fusion-std build.rs bootstrap\n",
        "red-inline Rust sidecar",
    );
    write_bootstrap_file_if_missing(
        &metadata_dir.join(AUTO_ASYNC_POLL_STACK_RUST_NAME),
        br#"// Generated by fusion-std build.rs bootstrap
fusion_std::declare_generated_async_poll_stack_contract!(
    crate::thread::executor::GeneratedAsyncPollStackMetadataAnchorFuture,
    1536,
);
"#,
        "async poll-stack Rust sidecar",
    );
}

fn write_bootstrap_file_if_missing(path: &Path, contents: &[u8], label: &str) {
    if path.is_file() {
        return;
    }
    fs::write(path, contents).unwrap_or_else(|error| {
        panic!(
            "fusion-std: failed to write bootstrap {label} {}: {error}",
            path.display()
        )
    });
}

/// TODO: Remove this hard-fail bridge once Fusion metadata is emitted and propagated directly by
/// `rustc`/`cargo`. Until then, silently falling back on missing or mismatched analyzer artifacts
/// just launders build-graph bugs into runtime guesswork.
fn required_generated_artifact_source<'a>(
    explicit_source: Option<&'a Path>,
    auto_candidates: &'a [PathBuf],
    env_name: &str,
    auto_name: &str,
    artifact_kind: &str,
) -> &'a Path {
    let auto_source = first_existing_path(auto_candidates).map(PathBuf::as_path);
    match explicit_source {
        Some(path) if !path.is_file() => {
            panic!(
                "fusion-std: {artifact_kind} was explicitly set via {env_name}={}, but that file \
                 does not exist",
                path.display()
            );
        }
        Some(path) => {
            if let Some(auto_path) = auto_source
                && !same_existing_file(path, auto_path)
            {
                panic!(
                    "fusion-std: explicit {artifact_kind} {} does not match active-build \
                     artifact {}; fix {env_name} or the analyzer output location",
                    path.display(),
                    auto_path.display()
                );
            }
            path
        }
        None => auto_source.unwrap_or_else(|| {
            panic!(
                "fusion-std: missing required {artifact_kind}; run `fusion_std_fiber_task_pipeline` \
                 or set {env_name}. Expected auto artifact named {auto_name} under the active \
                 build target directory."
            )
        }),
    }
}

fn assert_matching_generated_artifact_roots(
    left_kind: &str,
    left: &Path,
    right_kind: &str,
    right: &Path,
) {
    if !same_parent_directory(left, right) {
        panic!(
            "fusion-std: {left_kind} {} and {right_kind} {} do not come from the same generated \
             artifact directory",
            left.display(),
            right.display()
        );
    }
}

fn same_parent_directory(left: &Path, right: &Path) -> bool {
    match (left.parent(), right.parent()) {
        (Some(left_parent), Some(right_parent)) => same_existing_file(left_parent, right_parent),
        (None, None) => true,
        _ => false,
    }
}

fn same_existing_file(left: &Path, right: &Path) -> bool {
    match (fs::canonicalize(left), fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

fn load_generated_entries(path: &Path) -> Result<Vec<GeneratedFiberTaskEntry>, String> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.to_string()),
    };

    let mut entries = Vec::new();
    for (line_no, raw_line) in contents.lines().enumerate() {
        let line = raw_line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }

        let (type_name, rest) = line
            .split_once('=')
            .ok_or_else(|| format!("line {} is missing '='", line_no + 1))?;
        let type_name = type_name.trim();
        if type_name.is_empty() {
            return Err(format!("line {} has an empty type name", line_no + 1));
        }

        let mut parts = rest.split(',').map(str::trim);
        let stack_bytes = parts
            .next()
            .ok_or_else(|| format!("line {} is missing stack bytes", line_no + 1))?
            .parse::<usize>()
            .map_err(|error| format!("line {} stack bytes parse failed: {error}", line_no + 1))?
            .max(1);

        let mut priority = 0;
        let mut execution = GeneratedFiberTaskExecution::Fiber;
        match parts.next() {
            Some("") | None => {}
            Some(raw) => match raw.parse::<i8>() {
                Ok(parsed) => priority = parsed,
                Err(_) => execution = parse_generated_execution(raw, line_no)?,
            },
        }
        if priority != 0 || execution == GeneratedFiberTaskExecution::Fiber {
            if let Some(raw) = parts.next() {
                execution = parse_generated_execution(raw, line_no)?;
            }
        } else if let Some(raw) = parts.next() {
            return Err(format!(
                "line {} has too many comma-separated fields after execution `{raw}`",
                line_no + 1
            ));
        }
        if parts.next().is_some() {
            return Err(format!(
                "line {} has too many comma-separated fields",
                line_no + 1
            ));
        }

        entries.push(GeneratedFiberTaskEntry {
            type_name: type_name.to_owned(),
            stack_bytes,
            priority,
            execution,
        });
    }

    Ok(entries)
}

fn load_generated_async_poll_stack_entries(
    path: &Path,
) -> Result<Vec<GeneratedAsyncPollStackEntry>, String> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.to_string()),
    };

    let mut entries = Vec::new();
    for (line_no, raw_line) in contents.lines().enumerate() {
        let line = raw_line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }

        let (type_name, raw_bytes) = line
            .split_once('=')
            .ok_or_else(|| format!("line {} is missing '='", line_no + 1))?;
        let type_name = type_name.trim();
        if type_name.is_empty() {
            return Err(format!("line {} has an empty type name", line_no + 1));
        }

        let poll_stack_bytes = parse_linker_scalar(raw_bytes.trim())
            .map_err(|error| format!("line {} poll stack parse failed: {error}", line_no + 1))?;
        if poll_stack_bytes == 0 {
            return Err(format!(
                "line {} poll stack bytes must be non-zero",
                line_no + 1
            ));
        }

        entries.push(GeneratedAsyncPollStackEntry {
            type_name: type_name.to_owned(),
            poll_stack_bytes,
        });
    }

    Ok(entries)
}

fn render_generated_entries(entries: &[GeneratedFiberTaskEntry]) -> String {
    let mut rendered = String::from(
        "#[allow(dead_code)]\nconst GENERATED_EXPLICIT_FIBER_TASKS: &[GeneratedExplicitFiberTaskMetadata] = &[\n",
    );
    for entry in entries {
        rendered.push_str("    GeneratedExplicitFiberTaskMetadata {\n");
        rendered.push_str("        type_name: \"");
        rendered.push_str(&escape_rust_string(&entry.type_name));
        rendered.push_str("\",\n");
        rendered.push_str("        stack_bytes: ");
        rendered.push_str(&entry.stack_bytes.to_string());
        rendered.push_str(",\n");
        rendered.push_str("        priority: ");
        rendered.push_str(&entry.priority.to_string());
        rendered.push_str(",\n");
        rendered.push_str("        execution: ");
        rendered.push_str("crate::thread::");
        rendered.push_str(entry.execution.render());
        rendered.push_str(",\n");
        rendered.push_str("    },\n");
    }
    rendered.push_str("];\n\n");

    for entry in entries {
        if !generated_contract_type_is_nameable(&entry.type_name) {
            continue;
        }
        rendered.push_str("impl GeneratedExplicitFiberTaskContract for ");
        rendered.push_str(&render_type_path(&entry.type_name));
        rendered.push_str(" {\n");
        rendered.push_str(
            "    const ATTRIBUTES: FiberTaskAttributes = match admit_generated_fiber_task_stack_bytes(\n",
        );
        rendered.push_str("        NonZeroUsize::new(");
        rendered.push_str(&entry.stack_bytes.to_string());
        rendered.push_str(").unwrap(),\n");
        rendered.push_str("    ) {\n");
        rendered
            .push_str("        Ok(stack_bytes) => match FiberTaskAttributes::from_stack_bytes(\n");
        rendered.push_str("            stack_bytes,\n");
        rendered.push_str("            FiberTaskPriority::new(");
        rendered.push_str(&entry.priority.to_string());
        rendered.push_str("),\n");
        rendered.push_str("        ) {\n");
        rendered
            .push_str("            Ok(attributes) => attributes.with_execution(crate::thread::");
        rendered.push_str(entry.execution.render());
        rendered.push_str("),\n");
        rendered.push_str(
            "            Err(_) => panic!(\"invalid generated explicit fiber task contract\"),\n",
        );
        rendered.push_str("        },\n");
        rendered.push_str(
            "        Err(_) => panic!(\"invalid generated explicit fiber task contract\"),\n",
        );
        rendered.push_str("    };\n");
        rendered.push_str("}\n\n");
    }

    rendered
}

fn render_generated_async_poll_stack_entries(entries: &[GeneratedAsyncPollStackEntry]) -> String {
    let mut rendered = String::from(
        "#[allow(dead_code)]\n\
         const GENERATED_ASYNC_POLL_STACK_TASKS: &[GeneratedAsyncPollStackMetadataEntry] = &[\n",
    );
    for entry in entries {
        rendered.push_str("    GeneratedAsyncPollStackMetadataEntry {\n");
        rendered.push_str("        type_name: \"");
        rendered.push_str(&escape_rust_string(&entry.type_name));
        rendered.push_str("\",\n");
        rendered.push_str("        poll_stack_bytes: ");
        rendered.push_str(&entry.poll_stack_bytes.to_string());
        rendered.push_str(",\n");
        rendered.push_str("    },\n");
    }
    rendered.push_str("];\n\n");

    for entry in entries {
        if !generated_contract_type_is_nameable(&entry.type_name) {
            continue;
        }
        rendered.push_str("impl GeneratedExplicitAsyncPollStackContract for ");
        rendered.push_str(&render_type_path(&entry.type_name));
        rendered.push_str(" {\n");
        rendered.push_str("    const POLL_STACK_BYTES: usize = ");
        rendered.push_str(&entry.poll_stack_bytes.to_string());
        rendered.push_str(";\n");
        rendered.push_str("}\n\n");
    }
    rendered
}

fn generated_contract_type_is_nameable(type_name: &str) -> bool {
    !type_name.contains("{{closure}}")
}

fn assert_report_has_no_unresolved_symbols(path: &Path) -> Result<(), String> {
    let contents = fs::read_to_string(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    for (line_no, raw_line) in contents.lines().enumerate() {
        let line = raw_line.trim();
        if line.starts_with("# TODO: ") {
            return Err(format!(
                "unresolved analyzer contract at line {}: {}",
                line_no + 1,
                line
            ));
        }
    }
    Ok(())
}

fn render_type_path(type_name: &str) -> String {
    type_name.strip_prefix("fusion_std::").map_or_else(
        || format!("::{type_name}"),
        |suffix| format!("crate::{suffix}"),
    )
}

fn escape_rust_string(input: &str) -> String {
    let mut escaped = String::new();
    for ch in input.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            other => escaped.push(other),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_async_poll_stack_entries_keeps_worst_case_budget_per_type() {
        let merged = merge_async_poll_stack_entries(vec![
            GeneratedAsyncPollStackEntry {
                type_name: "crate::future::A".to_owned(),
                poll_stack_bytes: 512,
            },
            GeneratedAsyncPollStackEntry {
                type_name: "crate::future::A".to_owned(),
                poll_stack_bytes: 1024,
            },
            GeneratedAsyncPollStackEntry {
                type_name: "crate::future::B".to_owned(),
                poll_stack_bytes: 768,
            },
        ]);

        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].type_name, "crate::future::A");
        assert_eq!(merged[0].poll_stack_bytes, 1024);
        assert_eq!(merged[1].type_name, "crate::future::B");
        assert_eq!(merged[1].poll_stack_bytes, 768);
    }

    #[test]
    fn render_generated_async_poll_stack_entries_emits_trait_impls_for_nameable_types() {
        let rendered = render_generated_async_poll_stack_entries(&[
            GeneratedAsyncPollStackEntry {
                type_name:
                    "fusion_std::thread::executor::GeneratedAsyncPollStackMetadataAnchorFuture"
                        .to_owned(),
                poll_stack_bytes: 1536,
            },
            GeneratedAsyncPollStackEntry {
                type_name: "fusion_example_pico::main::{{closure}}".to_owned(),
                poll_stack_bytes: 1024,
            },
        ]);

        assert!(rendered.contains("const GENERATED_ASYNC_POLL_STACK_TASKS"));
        assert!(rendered.contains(
            "impl GeneratedExplicitAsyncPollStackContract for crate::thread::executor::GeneratedAsyncPollStackMetadataAnchorFuture"
        ));
        assert!(rendered.contains("const POLL_STACK_BYTES: usize = 1536;"));
        assert!(!rendered.contains(
            "impl GeneratedExplicitAsyncPollStackContract for ::fusion_example_pico::main::{{closure}}"
        ));
    }
}
