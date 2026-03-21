use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const AUTO_MANIFEST_NAME: &str = "fusion-std-fiber-task.generated";
const AUTO_REPORT_NAME: &str = "fusion-std-fiber-task.report";
const OUTPUT_NAME: &str = "fiber_task_generated.rs";
const GENERATED_METADATA_ENV: &str = "FUSION_FIBER_TASK_METADATA";
const GENERATED_REPORT_ENV: &str = "FUSION_FIBER_TASK_REPORT";
const STRICT_CONTRACTS_FEATURE_ENV: &str = "CARGO_FEATURE_CRITICAL_SAFE_GENERATED_CONTRACTS";

#[derive(Debug, Clone)]
struct GeneratedFiberTaskEntry {
    type_name: String,
    stack_bytes: usize,
    priority: i8,
}

fn main() {
    let (auto_manifest_candidates, auto_report_candidates) = setup_build_inputs();
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
}

fn setup_build_inputs() -> (Vec<PathBuf>, Vec<PathBuf>) {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed={GENERATED_METADATA_ENV}");
    println!("cargo:rerun-if-env-changed={GENERATED_REPORT_ENV}");
    println!("cargo:rerun-if-env-changed={STRICT_CONTRACTS_FEATURE_ENV}");

    let manifest_dir = PathBuf::from(
        env::var("CARGO_MANIFEST_DIR").expect("Cargo should always provide CARGO_MANIFEST_DIR"),
    );
    let auto_manifest_candidates = candidate_auto_artifact_paths(&manifest_dir, AUTO_MANIFEST_NAME);
    let auto_report_candidates = candidate_auto_artifact_paths(&manifest_dir, AUTO_REPORT_NAME);
    let analyzer_metadata = env::var_os(GENERATED_METADATA_ENV).map(PathBuf::from);
    let analyzer_report = env::var_os(GENERATED_REPORT_ENV).map(PathBuf::from);
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
    (auto_manifest_candidates, auto_report_candidates)
}

fn generate_fiber_task_metadata(
    auto_manifest_candidates: &[PathBuf],
    auto_report_candidates: &[PathBuf],
) -> String {
    let analyzer_metadata = env::var_os(GENERATED_METADATA_ENV).map(PathBuf::from);
    let analyzer_report = env::var_os(GENERATED_REPORT_ENV).map(PathBuf::from);
    let strict_generated_contracts = env::var_os(STRICT_CONTRACTS_FEATURE_ENV).is_some();
    let explicit_metadata_source = analyzer_metadata.as_deref().filter(|path| path.is_file());
    let explicit_report_source = analyzer_report.as_deref().filter(|path| path.is_file());
    let auto_metadata_source = first_existing_path(auto_manifest_candidates).map(PathBuf::as_path);
    let metadata_source = if strict_generated_contracts {
        Some(
            explicit_metadata_source
                .or(auto_metadata_source)
                .unwrap_or_else(|| {
                    panic!(
                        "fusion-std: strict generated-task contracts require analyzer output; run \
                     `fusion_std_fiber_task_pipeline` or set {GENERATED_METADATA_ENV}"
                    )
                }),
        )
    } else {
        explicit_metadata_source
    };
    let report_source = explicit_report_source;
    let mut entries =
        metadata_source.map_or_else(Vec::new, |path| match load_generated_entries(path) {
            Ok(entries) => entries,
            Err(error) => panic!(
                "fusion-std: failed to load generated fiber-task metadata from {}: {error}",
                path.display()
            ),
        });
    if strict_generated_contracts {
        let Some(report_source) = report_source
            .or_else(|| first_existing_path(auto_report_candidates).map(PathBuf::as_path))
        else {
            panic!(
                "fusion-std: strict generated-task contracts require an analyzer report; \
                 set {GENERATED_REPORT_ENV} or place {AUTO_REPORT_NAME} under target/"
            )
        };
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

fn candidate_auto_artifact_paths(manifest_dir: &Path, artifact_name: &str) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(target_dir) = env::var_os("CARGO_TARGET_DIR").map(PathBuf::from) {
        if target_dir.is_absolute() {
            roots.push(target_dir);
        } else {
            roots.push(manifest_dir.join(&target_dir));
            if let Some(workspace_root) = workspace_root(manifest_dir) {
                roots.push(workspace_root.join(&target_dir));
            }
        }
    }

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

fn workspace_root(manifest_dir: &Path) -> Option<&Path> {
    manifest_dir.parent().and_then(Path::parent)
}

fn first_existing_path(candidates: &[PathBuf]) -> Option<&PathBuf> {
    candidates.iter().find(|path| path.is_file())
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
            .map_err(|error| format!("line {} stack bytes parse failed: {error}", line_no + 1))?;
        if stack_bytes == 0 {
            return Err(format!("line {} stack bytes must be non-zero", line_no + 1));
        }

        let priority = match parts.next() {
            Some(raw) if !raw.is_empty() => raw
                .parse::<i8>()
                .map_err(|error| format!("line {} priority parse failed: {error}", line_no + 1))?,
            _ => 0,
        };
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
        rendered.push_str("    },\n");
    }
    rendered.push_str("];\n\n");

    for entry in entries {
        rendered.push_str("impl GeneratedExplicitFiberTaskContract for ");
        rendered.push_str(&render_type_path(&entry.type_name));
        rendered.push_str(" {\n");
        rendered.push_str("    const ATTRIBUTES: FiberTaskAttributes = match FiberTaskAttributes::from_stack_bytes(\n");
        rendered.push_str("        NonZeroUsize::new(");
        rendered.push_str(&entry.stack_bytes.to_string());
        rendered.push_str(").unwrap(),\n");
        rendered.push_str("        FiberTaskPriority::new(");
        rendered.push_str(&entry.priority.to_string());
        rendered.push_str("),\n");
        rendered.push_str("    ) {\n");
        rendered.push_str("        Ok(attributes) => attributes,\n");
        rendered.push_str(
            "        Err(_) => panic!(\"invalid generated explicit fiber task contract\"),\n",
        );
        rendered.push_str("    };\n");
        rendered.push_str("}\n\n");
    }

    rendered
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
