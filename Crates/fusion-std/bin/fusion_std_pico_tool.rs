use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

use object::{Object, ObjectSegment};

const DEFAULT_MANIFEST_PATH: &str = "Examples/PicoTarget/Cargo.toml";
const DEFAULT_TARGET: &str = "thumbv8m.main-none-eabihf";
const DEFAULT_BIN: &str = "pico";
const RP2350_FLASH_BASE: u64 = 0x1000_0000;
const RP2350_FLASH_LEN: usize = 4 * 1024 * 1024;
const UF2_MAGIC_START0: u32 = 0x0a32_4655;
const UF2_MAGIC_START1: u32 = 0x9e5d_5157;
const UF2_MAGIC_END: u32 = 0x0ab1_6f30;
const UF2_FLAG_FAMILY_ID_PRESENT: u32 = 0x0000_2000;
const UF2_PAYLOAD_SIZE: usize = 256;
const UF2_FAMILY_RP2XXX_ABSOLUTE: u32 = 0xe48b_ff57;
const UF2_FAMILY_RP2350_ARM_S: u32 = 0xe48b_ff59;
const DEFAULT_PROBE_CHIP: &str = "RP235x";
const PROBE_CHIP_ENV: &str = "FUSION_PICO_PROBE_CHIP";
const PROBE_SELECTOR_ENV: &str = "FUSION_PICO_PROBE_SELECTOR";

fn main() {
    if let Err(error) = try_main() {
        eprintln!("fusion_std_pico_tool: {error}");
        std::process::exit(1);
    }
}

fn try_main() -> Result<(), String> {
    let mut args = env::args_os();
    let _ = args.next();
    let Some(command) = args.next() else {
        return Err(usage());
    };

    match command.to_string_lossy().as_ref() {
        "uf2" => {
            let options = CommandOptions::parse(args)?;
            run_uf2(&options)?;
        }
        "probe-flash" => {
            let options = CommandOptions::parse(args)?;
            run_probe_flash(&options)?;
        }
        "probe-run" => {
            let options = CommandOptions::parse(args)?;
            run_probe_rs("run", &options)?;
        }
        "probe-attach" => {
            let options = CommandOptions::parse(args)?;
            run_probe_rs("attach", &options)?;
        }
        "help" | "--help" | "-h" => {
            println!("{}", usage());
        }
        other => return Err(format!("unknown subcommand `{other}`\n\n{}", usage())),
    }

    Ok(())
}

fn usage() -> String {
    format!(
        "usage:\n  \
         cargo pico-uf2 -- [--manifest-path PATH] [--target TRIPLE] [--bin NAME] [--release] [--family FAMILY] [--output-dir DIR]\n  \
         cargo pico-flash -- [--manifest-path PATH] [--target TRIPLE] [--bin NAME] [--release] --chip CHIP [--probe SELECTOR]\n  \
         cargo pico-run -- [--manifest-path PATH] [--target TRIPLE] [--bin NAME] [--release] --chip CHIP [--probe SELECTOR]\n  \
         cargo pico-attach -- [--manifest-path PATH] [--target TRIPLE] [--bin NAME] [--release] --chip CHIP [--probe SELECTOR]\n\n\
         defaults:\n  \
         manifest-path = {DEFAULT_MANIFEST_PATH}\n  \
         target = {DEFAULT_TARGET}\n  \
         bin = {DEFAULT_BIN}\n  \
         family = rp2xxx-absolute\n\n\
         environment:\n  \
         {PROBE_CHIP_ENV}=chip-name for probe-rs wrappers (defaults to {DEFAULT_PROBE_CHIP})\n  \
         {PROBE_SELECTOR_ENV}=probe selector passed through to probe-rs"
    )
}

#[derive(Debug, Clone, Copy)]
enum Uf2Family {
    Rp2xxxAbsolute,
    Rp2350ArmSecure,
    Raw(u32),
}

impl Uf2Family {
    fn from_str(raw: &str) -> Result<Self, String> {
        match raw {
            "rp2xxx-absolute" => Ok(Self::Rp2xxxAbsolute),
            "rp2350-arm-s" => Ok(Self::Rp2350ArmSecure),
            value if value.starts_with("0x") || value.starts_with("0X") => {
                u32::from_str_radix(value.trim_start_matches("0x").trim_start_matches("0X"), 16)
                    .map(Self::Raw)
                    .map_err(|error| format!("invalid UF2 family id `{value}`: {error}"))
            }
            other => Err(format!(
                "unsupported UF2 family `{other}`; use `rp2xxx-absolute`, `rp2350-arm-s`, or a hex u32"
            )),
        }
    }

    const fn id(self) -> u32 {
        match self {
            Self::Rp2xxxAbsolute => UF2_FAMILY_RP2XXX_ABSOLUTE,
            Self::Rp2350ArmSecure => UF2_FAMILY_RP2350_ARM_S,
            Self::Raw(value) => value,
        }
    }
}

#[derive(Debug, Clone)]
struct CommandOptions {
    manifest_path: PathBuf,
    target: String,
    bin_name: String,
    release: bool,
    family: Uf2Family,
    output_dir: Option<PathBuf>,
    chip: Option<String>,
    probe: Option<String>,
}

impl Default for CommandOptions {
    fn default() -> Self {
        Self {
            manifest_path: PathBuf::from(DEFAULT_MANIFEST_PATH),
            target: DEFAULT_TARGET.to_owned(),
            bin_name: DEFAULT_BIN.to_owned(),
            release: false,
            family: Uf2Family::Rp2xxxAbsolute,
            output_dir: None,
            chip: None,
            probe: None,
        }
    }
}

impl CommandOptions {
    fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Self, String> {
        let mut options = Self::default();
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            let arg = arg
                .into_string()
                .map_err(|_| "arguments must be valid UTF-8".to_owned())?;
            match arg.as_str() {
                "--" => {}
                "--manifest-path" => {
                    options.manifest_path =
                        PathBuf::from(next_value(&mut args, "--manifest-path")?);
                }
                "--target" => options.target = next_value(&mut args, "--target")?,
                "--bin" => options.bin_name = next_value(&mut args, "--bin")?,
                "--release" => options.release = true,
                "--family" => {
                    options.family = Uf2Family::from_str(&next_value(&mut args, "--family")?)?;
                }
                "--output-dir" => {
                    options.output_dir =
                        Some(PathBuf::from(next_value(&mut args, "--output-dir")?));
                }
                "--chip" => options.chip = Some(next_value(&mut args, "--chip")?),
                "--probe" => options.probe = Some(next_value(&mut args, "--probe")?),
                "--help" | "-h" => return Err(usage()),
                other => return Err(format!("unsupported option `{other}`\n\n{}", usage())),
            }
        }
        Ok(options)
    }

    fn manifest_path(&self) -> Result<PathBuf, String> {
        let manifest = absolute_path(&self.manifest_path)?;
        if !manifest.is_file() {
            return Err(format!(
                "manifest path {} does not exist",
                manifest.display()
            ));
        }
        Ok(manifest)
    }

    fn project_dir(&self) -> Result<PathBuf, String> {
        let manifest = self.manifest_path()?;
        manifest.parent().map(Path::to_path_buf).ok_or_else(|| {
            format!(
                "manifest path {} has no parent directory",
                manifest.display()
            )
        })
    }

    fn workspace_root(&self) -> Result<PathBuf, String> {
        let mut current = self.project_dir()?;
        loop {
            let candidate = current.join("Cargo.toml");
            if candidate.is_file() {
                let contents = fs::read_to_string(&candidate)
                    .map_err(|error| format!("failed to read {}: {error}", candidate.display()))?;
                if contents.contains("[workspace]") {
                    return Ok(current);
                }
            }
            if !current.pop() {
                break;
            }
        }
        Err(format!(
            "failed to locate workspace root above {}",
            self.project_dir()?.display()
        ))
    }

    const fn build_profile(&self) -> &'static str {
        if self.release { "release" } else { "debug" }
    }

    fn elf_path(&self) -> Result<PathBuf, String> {
        let root = self.workspace_root()?;
        Ok(root
            .join("target")
            .join(&self.target)
            .join(self.build_profile())
            .join(&self.bin_name))
    }

    fn output_dir(&self) -> Result<PathBuf, String> {
        if let Some(path) = &self.output_dir {
            return absolute_path(path);
        }
        let elf = self.elf_path()?;
        elf.parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| format!("ELF path {} has no parent directory", elf.display()))
    }

    fn resolve_chip(&self) -> String {
        self.chip
            .clone()
            .or_else(|| env::var(PROBE_CHIP_ENV).ok())
            .unwrap_or_else(|| DEFAULT_PROBE_CHIP.to_owned())
    }

    fn resolve_probe_selector(&self) -> Option<String> {
        self.probe
            .clone()
            .or_else(|| env::var(PROBE_SELECTOR_ENV).ok())
    }
}

fn next_value(args: &mut impl Iterator<Item = OsString>, flag: &str) -> Result<String, String> {
    args.next()
        .ok_or_else(|| format!("missing value after `{flag}`"))
        .and_then(|value| {
            value
                .into_string()
                .map_err(|_| format!("value for `{flag}` must be valid UTF-8"))
        })
}

fn absolute_path(path: &Path) -> Result<PathBuf, String> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        env::current_dir()
            .map(|cwd| cwd.join(path))
            .map_err(|error| {
                format!(
                    "failed to resolve {} against current directory: {error}",
                    path.display()
                )
            })
    }
}

fn run_uf2(options: &CommandOptions) -> Result<(), String> {
    build_example_elf(options)?;
    let elf_path = options.elf_path()?;
    let output_dir = options.output_dir()?;
    fs::create_dir_all(&output_dir)
        .map_err(|error| format!("failed to create {}: {error}", output_dir.display()))?;
    let image = extract_flash_image(&elf_path, RP2350_FLASH_BASE, RP2350_FLASH_LEN)?;
    let bin_path = output_dir.join(format!("{}.bin", options.bin_name));
    let uf2_path = output_dir.join(format!("{}.uf2", options.bin_name));
    fs::write(&bin_path, &image.bytes)
        .map_err(|error| format!("failed to write {}: {error}", bin_path.display()))?;
    write_uf2(&uf2_path, &image, options.family.id())?;
    println!("ELF : {}", elf_path.display());
    println!("BIN : {}", bin_path.display());
    println!("UF2 : {}", uf2_path.display());
    Ok(())
}

fn run_probe_flash(options: &CommandOptions) -> Result<(), String> {
    let manifest_path = options.manifest_path()?;
    let project_dir = options.project_dir()?;
    let chip = options.resolve_chip();
    let mut command = Command::new("cargo-flash");
    command.current_dir(project_dir);
    command.arg("--manifest-path").arg(manifest_path);
    command.arg("--target").arg(&options.target);
    command.arg("--bin").arg(&options.bin_name);
    command.arg("--chip").arg(chip);
    if options.release {
        command.arg("--release");
    }
    if let Some(probe) = options.resolve_probe_selector() {
        command.arg("--probe").arg(probe);
    }
    run_command(&mut command, "cargo-flash")?;
    Ok(())
}

fn run_probe_rs(subcommand: &str, options: &CommandOptions) -> Result<(), String> {
    build_example_elf(options)?;
    let chip = options.resolve_chip();
    let elf_path = options.elf_path()?;
    let mut command = Command::new("probe-rs");
    command.arg(subcommand);
    command.arg("--chip").arg(chip);
    if let Some(probe) = options.resolve_probe_selector() {
        command.arg("--probe").arg(probe);
    }
    command.arg(&elf_path);
    run_command(&mut command, "probe-rs")?;
    Ok(())
}

fn build_example_elf(options: &CommandOptions) -> Result<(), String> {
    let manifest_path = options.manifest_path()?;
    let project_dir = options.project_dir()?;
    let mut command = Command::new("cargo");
    command.current_dir(project_dir);
    command.arg("build");
    command.arg("--manifest-path").arg(manifest_path);
    command.arg("--target").arg(&options.target);
    command.arg("--bin").arg(&options.bin_name);
    if options.release {
        command.arg("--release");
    }
    run_command(&mut command, "cargo build")?;
    let elf_path = options.elf_path()?;
    if !elf_path.is_file() {
        return Err(format!(
            "build reported success but ELF {} was not found",
            elf_path.display()
        ));
    }
    Ok(())
}

fn run_command(command: &mut Command, tool_name: &str) -> Result<ExitStatus, String> {
    let printable = format!("{command:?}");
    command.stdout(std::process::Stdio::inherit());
    command.stderr(std::process::Stdio::inherit());
    let status = command.status().map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            format!("{tool_name} is not installed or not on PATH")
        } else {
            format!("failed to launch {printable}: {error}")
        }
    })?;
    if !status.success() {
        return Err(format!("{printable} exited with status {status}"));
    }
    Ok(status)
}

#[derive(Debug)]
struct FlashImage {
    base_address: u32,
    bytes: Vec<u8>,
}

fn extract_flash_image(
    elf_path: &Path,
    flash_base: u64,
    flash_len: usize,
) -> Result<FlashImage, String> {
    let bytes = fs::read(elf_path)
        .map_err(|error| format!("failed to read {}: {error}", elf_path.display()))?;
    let file = object::File::parse(&*bytes)
        .map_err(|error| format!("failed to parse ELF {}: {error}", elf_path.display()))?;

    let flash_end = flash_base
        .checked_add(u64::try_from(flash_len).map_err(|_| "flash length does not fit in u64")?)
        .ok_or_else(|| "flash address range overflowed".to_owned())?;

    let mut segments = Vec::new();
    let mut min_addr = u64::MAX;
    let mut max_addr = 0_u64;
    for segment in file.segments() {
        let data = segment
            .data()
            .map_err(|error| format!("failed to read ELF segment data: {error}"))?;
        if data.is_empty() {
            continue;
        }
        let seg_start = segment.address();
        let seg_end = seg_start
            .checked_add(u64::try_from(data.len()).map_err(|_| "segment length overflowed")?)
            .ok_or_else(|| "segment address range overflowed".to_owned())?;
        let copy_start = seg_start.max(flash_base);
        let copy_end = seg_end.min(flash_end);
        if copy_start >= copy_end {
            continue;
        }
        let start_offset = usize::try_from(copy_start - seg_start)
            .map_err(|_| "segment start offset does not fit in usize")?;
        let end_offset = usize::try_from(copy_end - seg_start)
            .map_err(|_| "segment end offset does not fit in usize")?;
        min_addr = min_addr.min(copy_start);
        max_addr = max_addr.max(copy_end);
        segments.push((copy_start, data[start_offset..end_offset].to_vec()));
    }

    if segments.is_empty() {
        return Err(format!(
            "ELF {} contained no flash-resident loadable data in {flash_base:#010x}..{flash_end:#010x}",
            elf_path.display()
        ));
    }

    let image_base = align_down(min_addr, UF2_PAYLOAD_SIZE as u64);
    let image_end = align_up(max_addr, UF2_PAYLOAD_SIZE as u64)?;
    let image_len = usize::try_from(image_end - image_base)
        .map_err(|_| "flash image length does not fit in usize")?;
    let mut image = vec![0xff_u8; image_len];
    for (address, data) in segments {
        let start = usize::try_from(address - image_base)
            .map_err(|_| "flash segment offset does not fit in usize")?;
        let end = start
            .checked_add(data.len())
            .ok_or_else(|| "flash segment range overflowed image buffer".to_owned())?;
        image[start..end].copy_from_slice(&data);
    }

    Ok(FlashImage {
        base_address: u32::try_from(image_base)
            .map_err(|_| "flash image base address does not fit in u32")?,
        bytes: image,
    })
}

const fn align_down(value: u64, align: u64) -> u64 {
    value - (value % align)
}

fn align_up(value: u64, align: u64) -> Result<u64, String> {
    let remainder = value % align;
    if remainder == 0 {
        Ok(value)
    } else {
        value
            .checked_add(align - remainder)
            .ok_or_else(|| "address alignment overflowed".to_owned())
    }
}

fn write_uf2(path: &Path, image: &FlashImage, family_id: u32) -> Result<(), String> {
    if !image.bytes.len().is_multiple_of(UF2_PAYLOAD_SIZE) {
        return Err("flash image length must already be 256-byte aligned".to_owned());
    }
    let total_blocks = image.bytes.len() / UF2_PAYLOAD_SIZE;
    let total_blocks_u32 =
        u32::try_from(total_blocks).map_err(|_| "UF2 block count does not fit in u32")?;
    let mut file = fs::File::create(path)
        .map_err(|error| format!("failed to create {}: {error}", path.display()))?;

    for (index, payload) in image.bytes.chunks_exact(UF2_PAYLOAD_SIZE).enumerate() {
        let mut block = [0_u8; 512];
        write_u32(&mut block[0..4], UF2_MAGIC_START0);
        write_u32(&mut block[4..8], UF2_MAGIC_START1);
        write_u32(&mut block[8..12], UF2_FLAG_FAMILY_ID_PRESENT);
        let target_addr = u32::try_from(
            u64::from(image.base_address)
                + u64::try_from(index * UF2_PAYLOAD_SIZE)
                    .map_err(|_| "UF2 target address overflowed")?,
        )
        .map_err(|_| "UF2 target address does not fit in u32")?;
        write_u32(&mut block[12..16], target_addr);
        write_u32(
            &mut block[16..20],
            u32::try_from(UF2_PAYLOAD_SIZE).map_err(|_| "UF2 payload size does not fit in u32")?,
        );
        write_u32(
            &mut block[20..24],
            u32::try_from(index).map_err(|_| "UF2 block index does not fit in u32")?,
        );
        write_u32(&mut block[24..28], total_blocks_u32);
        write_u32(&mut block[28..32], family_id);
        block[32..32 + UF2_PAYLOAD_SIZE].copy_from_slice(payload);
        write_u32(&mut block[512 - 4..512], UF2_MAGIC_END);
        file.write_all(&block)
            .map_err(|error| format!("failed to write {}: {error}", path.display()))?;
    }
    Ok(())
}

const fn write_u32(dst: &mut [u8], value: u32) {
    dst.copy_from_slice(&value.to_le_bytes());
}
