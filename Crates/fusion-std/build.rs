use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const MANIFEST_NAME: &str = "fiber-task.generated";
const AUTO_MANIFEST_NAME: &str = "fusion-std-fiber-task.generated";
const OUTPUT_NAME: &str = "fiber_task_generated.rs";
const GENERATED_METADATA_ENV: &str = "FUSION_FIBER_TASK_METADATA";

#[derive(Debug)]
struct GeneratedFiberTaskEntry {
    type_name: String,
    stack_bytes: usize,
    priority: i8,
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={MANIFEST_NAME}");
    println!("cargo:rerun-if-env-changed={GENERATED_METADATA_ENV}");

    let manifest_dir = PathBuf::from(
        env::var("CARGO_MANIFEST_DIR").expect("Cargo should always provide CARGO_MANIFEST_DIR"),
    );
    let manifest_path = manifest_dir.join(MANIFEST_NAME);
    let auto_manifest_path = default_auto_manifest_path(&manifest_dir);
    let analyzer_metadata = env::var_os(GENERATED_METADATA_ENV).map(PathBuf::from);
    if let Some(path) = analyzer_metadata.as_ref()
        && path.is_file()
    {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    if auto_manifest_path.is_file() {
        println!("cargo:rerun-if-changed={}", auto_manifest_path.display());
    }
    let output_path =
        PathBuf::from(env::var("OUT_DIR").expect("Cargo should provide OUT_DIR")).join(OUTPUT_NAME);

    let metadata_source = analyzer_metadata
        .as_deref()
        .filter(|path| path.is_file())
        .or_else(|| {
            auto_manifest_path
                .is_file()
                .then_some(auto_manifest_path.as_path())
        })
        .unwrap_or(&manifest_path);
    let entries = match load_generated_entries(metadata_source) {
        Ok(entries) => entries,
        Err(error) => panic!(
            "fusion-std: failed to load generated fiber-task metadata from {}: {error}",
            metadata_source.display()
        ),
    };

    let generated = render_generated_entries(&entries);
    fs::write(&output_path, generated).unwrap_or_else(|error| {
        panic!(
            "fusion-std: failed to write {}: {error}",
            output_path.display()
        )
    });
}

fn default_auto_manifest_path(manifest_dir: &Path) -> PathBuf {
    if let Some(target_dir) = env::var_os("CARGO_TARGET_DIR").map(PathBuf::from) {
        return target_dir.join(AUTO_MANIFEST_NAME);
    }

    manifest_dir
        .parent()
        .and_then(Path::parent)
        .map_or_else(|| PathBuf::from("target"), PathBuf::from)
        .join(AUTO_MANIFEST_NAME)
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
        "const GENERATED_EXPLICIT_FIBER_TASKS: &[GeneratedExplicitFiberTaskMetadata] = &[\n",
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
