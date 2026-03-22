use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const AUTO_MANIFEST_NAME: &str = "fusion-std-fiber-task.generated";
const AUTO_REPORT_NAME: &str = "fusion-std-fiber-task.report";
const OUTPUT_NAME: &str = "fiber_task_generated.rs";
const MEMORY_LAYOUT_OUTPUT_NAME: &str = "memory.x";
const GENERATED_METADATA_ENV: &str = "FUSION_FIBER_TASK_METADATA";
const GENERATED_REPORT_ENV: &str = "FUSION_FIBER_TASK_REPORT";
const STRICT_CONTRACTS_FEATURE_ENV: &str = "CARGO_FEATURE_CRITICAL_SAFE_GENERATED_CONTRACTS";
const SYS_CORTEX_M_FEATURE_ENV: &str = "CARGO_FEATURE_SYS_CORTEX_M";
const SOC_RP2350_FEATURE_ENV: &str = "CARGO_FEATURE_SOC_RP2350";
const CORTEX_M_VECTOR_NONSECURE_WORLD_FEATURE_ENV: &str =
    "CARGO_FEATURE_CORTEX_M_VECTOR_NONSECURE_WORLD";
const MAIN_STACK_RESERVE_ENV: &str = "FUSION_CORTEX_M_MAIN_STACK_RESERVE";
const RP2350_FLASH_BOOT_METADATA_KIND: &str = "rp2350-image-def";
const RP2350_FLASH_BOOT_MIN_WINDOW_BYTES: usize = 4 * 1024;
const RP2350_BOOT_BLOCK_MARKER_START: u32 = 0xffff_ded3;
const RP2350_BOOT_BLOCK_MARKER_END: u32 = 0xab12_3579;
const RP2350_BOOT_ITEM_1BS_IMAGE_TYPE: u32 = 0x42;
const RP2350_BOOT_ITEM_2BS_LAST: u32 = 0xff;
const RP2350_BOOT_IMAGE_TYPE_EXE: u32 = 0x0001;
const RP2350_BOOT_IMAGE_TYPE_SECURITY_NS: u32 = 0x0010;
const RP2350_BOOT_IMAGE_TYPE_SECURITY_S: u32 = 0x0020;
const RP2350_BOOT_IMAGE_TYPE_CHIP_RP2350: u32 = 0x1000;

#[derive(Debug, Clone)]
struct GeneratedFiberTaskEntry {
    type_name: String,
    stack_bytes: usize,
    priority: i8,
}

#[derive(Debug, Clone)]
struct CortexMMemoryLayoutSpec {
    board_name: String,
    flash_origin: usize,
    flash_length: usize,
    flash_boot_metadata: CortexMFlashBootMetadata,
    ram_origin: usize,
    ram_length: usize,
    default_main_stack_reserve: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CortexMFlashBootMetadata {
    None,
    Rp2350ImageDef { window_bytes: usize },
}

fn main() {
    let (auto_manifest_candidates, auto_report_candidates) = setup_build_inputs();
    emit_platform_memory_layout();
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
    println!("cargo:rerun-if-env-changed={SYS_CORTEX_M_FEATURE_ENV}");
    println!("cargo:rerun-if-env-changed={SOC_RP2350_FEATURE_ENV}");
    println!("cargo:rerun-if-env-changed={MAIN_STACK_RESERVE_ENV}");

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

fn emit_platform_memory_layout() {
    if env::var_os(SYS_CORTEX_M_FEATURE_ENV).is_none() {
        return;
    }

    let Some(source_path) = selected_platform_memory_layout_spec_path() else {
        return;
    };

    println!("cargo:rerun-if-changed={}", source_path.display());
    let spec = load_platform_memory_layout_spec(&source_path).unwrap_or_else(|error| {
        panic!(
            "fusion-std: failed to load platform memory layout spec from {}: {error}",
            source_path.display()
        )
    });
    let main_stack_reserve = selected_main_stack_reserve(&spec)
        .unwrap_or_else(|error| panic!("fusion-std: invalid {MAIN_STACK_RESERVE_ENV}: {error}"));
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("Cargo should provide OUT_DIR"));
    let output_path = out_dir.join(MEMORY_LAYOUT_OUTPUT_NAME);
    let rendered = render_platform_memory_layout(&spec, main_stack_reserve);
    fs::write(&output_path, rendered).unwrap_or_else(|error| {
        panic!(
            "fusion-std: failed to write generated platform memory layout to {}: {error}",
            output_path.display()
        )
    });
    println!("cargo:rustc-link-search={}", out_dir.display());
}

fn selected_platform_memory_layout_spec_path() -> Option<PathBuf> {
    if env::var_os(SOC_RP2350_FEATURE_ENV).is_some() {
        return Some(
            PathBuf::from(
                env::var("CARGO_MANIFEST_DIR").expect("Cargo should provide CARGO_MANIFEST_DIR"),
            )
            .join(
                "../fusion-sys/fusion-pal/sys/cortex_m/hal/soc/board/rp2350-pico2w.memory.layout",
            ),
        );
    }
    None
}

fn load_platform_memory_layout_spec(path: &Path) -> Result<CortexMMemoryLayoutSpec, String> {
    let contents = fs::read_to_string(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    let mut board_name = None;
    let mut flash_origin = None;
    let mut flash_length = None;
    let mut flash_boot_metadata_kind = None;
    let mut flash_boot_window = None;
    let mut ram_origin = None;
    let mut ram_length = None;
    let mut default_main_stack_reserve = None;

    for (line_no, raw_line) in contents.lines().enumerate() {
        let line = raw_line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| format!("line {} is missing '='", line_no + 1))?;
        let key = key.trim();
        let value = value.trim();
        match key {
            "board_name" => board_name = Some(value.trim_matches('"').to_owned()),
            "flash_origin" => flash_origin = Some(parse_linker_scalar(value)?),
            "flash_length" => flash_length = Some(parse_linker_scalar(value)?),
            "flash_boot_metadata_kind" => {
                flash_boot_metadata_kind = Some(value.trim_matches('"').to_owned());
            }
            "flash_boot_window" => flash_boot_window = Some(parse_linker_scalar(value)?),
            "ram_origin" => ram_origin = Some(parse_linker_scalar(value)?),
            "ram_length" => ram_length = Some(parse_linker_scalar(value)?),
            "default_main_stack_reserve" => {
                default_main_stack_reserve = Some(parse_linker_scalar(value)?);
            }
            other => {
                return Err(format!(
                    "unsupported memory layout key `{other}` at line {}",
                    line_no + 1
                ));
            }
        }
    }

    let flash_length = flash_length.ok_or_else(|| "missing flash_length".to_owned())?;
    let flash_boot_metadata = match flash_boot_metadata_kind.as_deref() {
        None => CortexMFlashBootMetadata::None,
        Some(RP2350_FLASH_BOOT_METADATA_KIND) => {
            let window_bytes =
                flash_boot_window.ok_or_else(|| "missing flash_boot_window".to_owned())?;
            if window_bytes < RP2350_FLASH_BOOT_MIN_WINDOW_BYTES {
                return Err(format!(
                    "flash_boot_window {window_bytes:#x} is smaller than the RP2350 first-4-KiB boot requirement"
                ));
            }
            if window_bytes >= flash_length {
                return Err(format!(
                    "flash_boot_window {window_bytes:#x} leaves no flash space beyond the reserved boot window"
                ));
            }
            CortexMFlashBootMetadata::Rp2350ImageDef { window_bytes }
        }
        Some(other) => {
            return Err(format!("unsupported flash boot metadata kind `{other}`"));
        }
    };
    if matches!(flash_boot_metadata, CortexMFlashBootMetadata::None) && flash_boot_window.is_some()
    {
        return Err(
            "flash_boot_window is only valid when flash_boot_metadata_kind is configured"
                .to_owned(),
        );
    }

    Ok(CortexMMemoryLayoutSpec {
        board_name: board_name.ok_or_else(|| "missing board_name".to_owned())?,
        flash_origin: flash_origin.ok_or_else(|| "missing flash_origin".to_owned())?,
        flash_length,
        flash_boot_metadata,
        ram_origin: ram_origin.ok_or_else(|| "missing ram_origin".to_owned())?,
        ram_length: ram_length.ok_or_else(|| "missing ram_length".to_owned())?,
        default_main_stack_reserve: default_main_stack_reserve
            .ok_or_else(|| "missing default_main_stack_reserve".to_owned())?,
    })
}

fn selected_main_stack_reserve(spec: &CortexMMemoryLayoutSpec) -> Result<usize, String> {
    let reserve = match env::var_os(MAIN_STACK_RESERVE_ENV) {
        Some(value) => parse_linker_scalar(&value.to_string_lossy())?,
        None => spec.default_main_stack_reserve,
    };
    if reserve == 0 {
        return Err("main stack reserve must be non-zero".to_owned());
    }
    if reserve >= spec.ram_length {
        return Err(format!(
            "main stack reserve {reserve:#x} leaves no RAM below the stack boundary"
        ));
    }
    Ok(reserve)
}

fn render_platform_memory_layout(
    spec: &CortexMMemoryLayoutSpec,
    main_stack_reserve: usize,
) -> String {
    let flash_boot_metadata = render_flash_boot_metadata(spec);
    format!(
        "/* Generated by fusion-std build.rs from the owning Cortex-M board layout spec.\n\
* Board: {board_name}\n\
* Main/exception stack reserve: 0x{main_stack_reserve:x} bytes\n\
*/\n\
MEMORY\n\
{{\n\
    /* XIP flash -- program code executes in place */\n\
    FLASH : ORIGIN = 0x{flash_origin:08x}, LENGTH = 0x{flash_length:x}\n\
    /* SRAM -- all board-visible application RAM */\n\
    RAM   : ORIGIN = 0x{ram_origin:08x}, LENGTH = 0x{ram_length:x}\n\
}}\n\n\
{flash_boot_metadata}\
/* Reserve the configured main/exception stack window at the top of RAM.\n\
 * The gap between `__sheap` (after .bss/.uninit) and `_stack_end`\n\
 * becomes board-owned free SRAM for allocator and fiber backing.\n\
 */\n\
_stack_end = ORIGIN(RAM) + LENGTH(RAM) - 0x{main_stack_reserve:x};\n",
        board_name = spec.board_name,
        flash_origin = spec.flash_origin,
        flash_length = spec.flash_length,
        flash_boot_metadata = flash_boot_metadata,
        ram_origin = spec.ram_origin,
        ram_length = spec.ram_length,
        main_stack_reserve = main_stack_reserve,
    )
}

fn render_flash_boot_metadata(spec: &CortexMMemoryLayoutSpec) -> String {
    match spec.flash_boot_metadata {
        CortexMFlashBootMetadata::None => String::new(),
        CortexMFlashBootMetadata::Rp2350ImageDef { window_bytes } => format!(
            "/* RP2350 flash boot metadata.\n\
* Keep the Arm vector table at flash base, reserve the rest of the first boot window for boot\n\
* metadata, and emit one board-owned IMAGE_DEF block directly into `.start_block` so the ROM gets\n\
* facts instead of our feelings.\n\
*/\n\
_stext = ORIGIN(FLASH) + 0x{window_bytes:x};\n\
SECTIONS\n\
{{\n\
    .start_block : ALIGN(4)\n\
    {{\n\
        LONG(0x{marker_start:08x})\n\
        LONG(0x{image_type_item:08x})\n\
        LONG(0x{block_last:08x})\n\
        LONG(0x00000000)\n\
        LONG(0x{marker_end:08x})\n\
    }} > FLASH\n\
}}\n\
INSERT AFTER .vector_table;\n\
ASSERT(ADDR(.start_block) + SIZEOF(.start_block) <= ORIGIN(FLASH) + 0x{window_bytes:x},\n\
       \"RP2350 boot metadata must fit inside the configured first-flash boot window\");\n\n",
            marker_start = RP2350_BOOT_BLOCK_MARKER_START,
            image_type_item = rp2350_flash_boot_image_type_item(),
            block_last = rp2350_flash_boot_block_last_item(),
            marker_end = RP2350_BOOT_BLOCK_MARKER_END,
            window_bytes = window_bytes,
        ),
    }
}

fn rp2350_flash_boot_image_type_item() -> u32 {
    let security_bits = if env::var_os(CORTEX_M_VECTOR_NONSECURE_WORLD_FEATURE_ENV).is_some() {
        RP2350_BOOT_IMAGE_TYPE_SECURITY_NS
    } else {
        RP2350_BOOT_IMAGE_TYPE_SECURITY_S
    };
    let image_type =
        RP2350_BOOT_IMAGE_TYPE_EXE | RP2350_BOOT_IMAGE_TYPE_CHIP_RP2350 | security_bits;
    (image_type << 16) | (1 << 8) | RP2350_BOOT_ITEM_1BS_IMAGE_TYPE
}

const fn rp2350_flash_boot_block_last_item() -> u32 {
    (1 << 8) | RP2350_BOOT_ITEM_2BS_LAST
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
