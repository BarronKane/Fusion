use std::env;
use std::fs;
use std::path::{
    Path,
    PathBuf,
};

const MEMORY_LAYOUT_OUTPUT_NAME: &str = "memory.x";
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

fn feature_enabled(name: &str) -> bool {
    env::var_os(name).is_some()
}

fn selected_lane() -> &'static str {
    let soc = feature_enabled("CARGO_FEATURE_SOC");
    let hosted = feature_enabled("CARGO_FEATURE_HOSTED");

    // `hosted` is the soft default for ordinary root-workspace consumers. When a more specific
    // hardware lane is selected explicitly, let it win instead of turning default convenience into
    // a fake conflict.
    if soc {
        return "soc";
    }

    if hosted {
        return "hosted";
    }

    if feature_enabled("CARGO_FEATURE_SYS_CORTEX_M") || feature_enabled("CARGO_FEATURE_SOC_RP2350")
    {
        return "soc";
    }

    if feature_enabled("CARGO_FEATURE_SYS_FUSION_KN") {
        return "hosted";
    }

    match env::var("CARGO_CFG_TARGET_OS").as_deref() {
        Ok("linux" | "macos" | "windows" | "ios") => "hosted",
        Ok("none") => panic!(
            "fusion-pal no longer exposes the generic `hal` lane; use `fusion-firmware` for dynamic firmware and hardware-discovery targets"
        ),
        Ok(other) => panic!(
            "fusion-pal could not infer PAL lane for target_os={other:?}; enable one of `soc` or `hosted`"
        ),
        Err(_) => panic!(
            "fusion-pal could not infer PAL lane because CARGO_CFG_TARGET_OS was unavailable"
        ),
    }
}

fn selected_pal_glue(lane: &str) -> String {
    match lane {
        "soc" => "pub use super::soc::SelectedPalLane;\n\
                  pub const PAL_LANE_NAME: &str = super::soc::PAL_LANE_NAME;\n\
                  #[cfg(all(target_os = \"none\", feature = \"sys-cortex-m\"))]\n\
                  pub use crate::pal::soc::cortex_m as platform;\n\
                  #[cfg(all(not(target_os = \"none\"), target_os = \"linux\"))]\n\
                  pub use crate::pal::hosted::linux as platform;\n\
                  #[cfg(target_os = \"macos\")]\n\
                  pub use crate::pal::hosted::macos as platform;\n\
                  #[cfg(target_os = \"windows\")]\n\
                  pub use crate::pal::hosted::windows as platform;\n\
                  #[cfg(target_os = \"ios\")]\n\
                  pub use crate::pal::hosted::ios as platform;\n"
            .to_owned(),
        "hosted" => {
            if feature_enabled("CARGO_FEATURE_SYS_FUSION_KN") {
                return "pub use super::hosted::SelectedPalLane;\n\
                        pub const PAL_LANE_NAME: &str = super::hosted::PAL_LANE_NAME;\n\
                        pub use crate::pal::hosted::fusion_kn as platform;\n"
                    .to_owned();
            }

            match env::var("CARGO_CFG_TARGET_OS").as_deref() {
                Ok("ios") => "pub use super::hosted::SelectedPalLane;\n\
                              pub const PAL_LANE_NAME: &str = super::hosted::PAL_LANE_NAME;\n\
                              pub use crate::pal::hosted::ios as platform;\n"
                    .to_owned(),
                Ok("linux") => "pub use super::hosted::SelectedPalLane;\n\
                                pub const PAL_LANE_NAME: &str = super::hosted::PAL_LANE_NAME;\n\
                                pub use crate::pal::hosted::linux as platform;\n"
                    .to_owned(),
                Ok("macos") => "pub use super::hosted::SelectedPalLane;\n\
                                pub const PAL_LANE_NAME: &str = super::hosted::PAL_LANE_NAME;\n\
                                pub use crate::pal::hosted::macos as platform;\n"
                    .to_owned(),
                Ok("windows") => "pub use super::hosted::SelectedPalLane;\n\
                                  pub const PAL_LANE_NAME: &str = super::hosted::PAL_LANE_NAME;\n\
                                  pub use crate::pal::hosted::windows as platform;\n"
                    .to_owned(),
                Ok(other) => panic!(
                    "fusion-pal could not select hosted PAL platform glue for target_os={other:?}"
                ),
                Err(_) => panic!(
                    "fusion-pal could not select hosted PAL platform glue because CARGO_CFG_TARGET_OS was unavailable"
                ),
            }
        }
        other => panic!("unsupported fusion-pal lane selection {other:?}"),
    }
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_SOC");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_HOSTED");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_SYS_CORTEX_M");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_SOC_RP2350");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_SYS_FUSION_KN");
    println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_OS");
    println!("cargo:rerun-if-env-changed={MAIN_STACK_RESERVE_ENV}");

    emit_platform_memory_layout();

    let lane = selected_lane();
    let selected_rs = selected_pal_glue(lane);

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("cargo should provide OUT_DIR"));
    fs::write(out_dir.join("selected_pal.rs"), selected_rs)
        .expect("fusion-pal build should emit selected PAL glue");
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
            "fusion-pal: failed to load platform memory layout spec from {}: {error}",
            source_path.display()
        )
    });
    let main_stack_reserve = selected_main_stack_reserve(&spec)
        .unwrap_or_else(|error| panic!("fusion-pal: invalid {MAIN_STACK_RESERVE_ENV}: {error}"));
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("Cargo should provide OUT_DIR"));
    let output_path = out_dir.join(MEMORY_LAYOUT_OUTPUT_NAME);
    let rendered = render_platform_memory_layout(&spec, main_stack_reserve);
    fs::write(&output_path, rendered).unwrap_or_else(|error| {
        panic!(
            "fusion-pal: failed to write generated platform memory layout to {}: {error}",
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
            .join("pal/soc/cortex_m/hal/soc/board/rp2350-pico2w.memory.layout"),
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
    let fdxe_static_modules = render_fdxe_static_module_section();
    let build_id_section = render_build_id_section();
    format!(
        "/* Generated by fusion-pal build.rs from the owning Cortex-M board layout spec.\n\
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
{fdxe_static_modules}\
{build_id_section}\
/* Reserve the configured main/exception stack window at the top of RAM.\n\
 * The gap between `__sheap` (after .bss/.uninit) and `_stack_end`\n\
 * becomes board-owned free SRAM for allocator and fiber backing.\n\
 */\n\
_stack_end = ORIGIN(RAM) + LENGTH(RAM) - 0x{main_stack_reserve:x};\n",
        board_name = spec.board_name,
        flash_origin = spec.flash_origin,
        flash_length = spec.flash_length,
        flash_boot_metadata = flash_boot_metadata,
        fdxe_static_modules = fdxe_static_modules,
        build_id_section = build_id_section,
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

fn render_fdxe_static_module_section() -> String {
    "/* Statically embedded Fusion driver-module records.\n\
* Bare-metal firmware images cannot dlopen their way out of the void, so keep the FDXE module\n\
* records in flash and let `fusion-firmware` walk them directly at boot.\n\
*/\n\
SECTIONS\n\
{\n\
    .fdxe_modules : ALIGN(4)\n\
    {\n\
        __fusion_fdxe_modules_start = .;\n\
        KEEP(*(.fdxe.modules));\n\
        __fusion_fdxe_modules_end = .;\n\
    } > FLASH\n\
}\n\
INSERT AFTER .rodata;\n\n"
        .to_owned()
}

fn render_build_id_section() -> String {
    "/* Embedded firmware build identity.\n\
* Keep the exported build-id payload in its own retained flash section so release stripping cannot\n\
* gaslight the post-flash verification path into forgetting what image it just wrote.\n\
*/\n\
SECTIONS\n\
{\n\
    .fusion_build_id : ALIGN(4)\n\
    {\n\
        __fusion_build_id_start = .;\n\
        KEEP(*(.fusion.build_id));\n\
        __fusion_build_id_end = .;\n\
    } > FLASH\n\
}\n\
INSERT AFTER .rodata;\n\n"
        .to_owned()
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
