use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration as StdDuration, Instant};

use object::{Object, ObjectSegment, ObjectSymbol};

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
const DEFAULT_GDB_CONNECTION_STRING: &str = "[::1]:2345";
const PROBE_CHIP_ENV: &str = "FUSION_PICO_PROBE_CHIP";
const PROBE_SELECTOR_ENV: &str = "FUSION_PICO_PROBE_SELECTOR";
const PICO_BENCH_OUTPUT_SYMBOL: &str = "FUSION_PICO_BENCH_OUTPUT";
const PICO_BENCH_POLL_INTERVAL: StdDuration = StdDuration::from_millis(100);
const PICO_BENCH_RELEASE_TIMEOUT: StdDuration = StdDuration::from_secs(30);
const PICO_BENCH_DEBUG_TIMEOUT: StdDuration = StdDuration::from_secs(120);

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
        "build" => {
            let options = CommandOptions::parse(args)?;
            run_build(&options)?;
        }
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
        "benchmark" => {
            let options = CommandOptions::parse(args)?;
            run_benchmark(&options)?;
        }
        "debug-server" => {
            let options = CommandOptions::parse(args)?;
            run_debug_server(&options)?;
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
         cargo pico-build -- [--manifest-path PATH] [--target TRIPLE] [--bin NAME] [--release]\n  \
         cargo pico-uf2 -- [--manifest-path PATH] [--target TRIPLE] [--bin NAME] [--release] [--family FAMILY] [--output-dir DIR]\n  \
         cargo pico-flash -- [--manifest-path PATH] [--target TRIPLE] [--bin NAME] [--release] --chip CHIP [--probe SELECTOR]\n  \
         cargo pico-run -- [--manifest-path PATH] [--target TRIPLE] [--bin NAME] [--release] --chip CHIP [--probe SELECTOR]\n  \
         cargo pico-attach -- [--manifest-path PATH] [--target TRIPLE] [--bin NAME] [--release] --chip CHIP [--probe SELECTOR]\n  \
         cargo pico-benchmark -- [--manifest-path PATH] [--target TRIPLE] [--bin NAME] [--release] [--benchmark-timeout-secs SECONDS] --chip CHIP [--probe SELECTOR]\n  \
         cargo pico-debug-server -- [--manifest-path PATH] [--target TRIPLE] [--bin NAME] [--release] [--gdb-connection-string HOST:PORT] [--detach] [--output-dir DIR] --chip CHIP [--probe SELECTOR]\n\n\
         defaults:\n  \
         manifest-path = {DEFAULT_MANIFEST_PATH}\n  \
         target = {DEFAULT_TARGET}\n  \
         bin = {DEFAULT_BIN}\n  \
         family = rp2xxx-absolute\n  \
         gdb-connection-string = {DEFAULT_GDB_CONNECTION_STRING}\n\n\
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
    gdb_connection_string: String,
    detach: bool,
    output_dir: Option<PathBuf>,
    chip: Option<String>,
    probe: Option<String>,
    benchmark_timeout_secs: Option<u64>,
}

impl Default for CommandOptions {
    fn default() -> Self {
        Self {
            manifest_path: PathBuf::from(DEFAULT_MANIFEST_PATH),
            target: DEFAULT_TARGET.to_owned(),
            bin_name: DEFAULT_BIN.to_owned(),
            release: false,
            family: Uf2Family::Rp2xxxAbsolute,
            gdb_connection_string: DEFAULT_GDB_CONNECTION_STRING.to_owned(),
            detach: false,
            output_dir: None,
            chip: None,
            probe: None,
            benchmark_timeout_secs: None,
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
                "--gdb-connection-string" => {
                    options.gdb_connection_string =
                        next_value(&mut args, "--gdb-connection-string")?;
                }
                "--detach" => options.detach = true,
                "--output-dir" => {
                    options.output_dir =
                        Some(PathBuf::from(next_value(&mut args, "--output-dir")?));
                }
                "--chip" => options.chip = Some(next_value(&mut args, "--chip")?),
                "--probe" => options.probe = Some(next_value(&mut args, "--probe")?),
                "--benchmark-timeout-secs" => {
                    let raw = next_value(&mut args, "--benchmark-timeout-secs")?;
                    options.benchmark_timeout_secs =
                        Some(raw.parse::<u64>().map_err(|error| {
                            format!("invalid benchmark timeout `{raw}`: {error}")
                        })?);
                }
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

    const fn benchmark_timeout(&self) -> StdDuration {
        if let Some(seconds) = self.benchmark_timeout_secs {
            return StdDuration::from_secs(seconds);
        }
        if self.release {
            PICO_BENCH_RELEASE_TIMEOUT
        } else {
            PICO_BENCH_DEBUG_TIMEOUT
        }
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

fn run_build(options: &CommandOptions) -> Result<(), String> {
    build_example_elf(options)?;
    let elf_path = options.elf_path()?;
    println!("ELF : {}", elf_path.display());
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

fn run_benchmark(options: &CommandOptions) -> Result<(), String> {
    build_example_elf_with_symbols(options)?;
    let chip = options.resolve_chip();
    let elf_path = options.elf_path()?;
    let (output_addr, output_size) = find_symbol_range(&elf_path, PICO_BENCH_OUTPUT_SYMBOL)?;

    let mut download = Command::new("probe-rs");
    download.arg("download");
    download.arg("--chip").arg(&chip);
    if let Some(probe) = options.resolve_probe_selector() {
        download.arg("--probe").arg(probe);
    }
    download.arg(&elf_path);
    run_command(&mut download, "probe-rs")?;

    let mut reset = Command::new("probe-rs");
    reset.arg("reset");
    reset.arg("--chip").arg(&chip);
    if let Some(probe) = options.resolve_probe_selector() {
        reset.arg("--probe").arg(probe);
    }
    run_command(&mut reset, "probe-rs")?;

    let words = usize::try_from(output_size)
        .map_err(|_| "benchmark output size does not fit in usize".to_owned())?
        .div_ceil(4);
    let deadline = Instant::now() + options.benchmark_timeout();
    loop {
        let words_read = probe_read_words(options, output_addr, words)?;
        if pico_bench_state(&words_read) == Some(2) {
            print_pico_bench_report(&words_read);
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "timed out waiting for Pico benchmark completion at {output_addr:#010x}"
            ));
        }
        thread::sleep(PICO_BENCH_POLL_INTERVAL);
    }
}

fn probe_read_words(
    options: &CommandOptions,
    address: u64,
    words: usize,
) -> Result<Vec<u32>, String> {
    let chip = options.resolve_chip();
    let mut command = Command::new("probe-rs");
    command.arg("read");
    command.arg("--chip").arg(chip);
    if let Some(probe) = options.resolve_probe_selector() {
        command.arg("--probe").arg(probe);
    }
    command.arg("b32");
    command.arg(format!("{address:#x}"));
    command.arg(words.to_string());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::inherit());
    let output = command
        .output()
        .map_err(|error| format!("failed to launch probe-rs read: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "probe-rs read exited with status {}",
            output.status
        ));
    }
    let stdout = String::from_utf8(output.stdout)
        .map_err(|error| format!("probe-rs read returned invalid UTF-8: {error}"))?;
    stdout
        .split_whitespace()
        .map(|word| {
            u32::from_str_radix(word, 16)
                .map_err(|error| format!("failed to parse probe-rs read word `{word}`: {error}"))
        })
        .collect()
}

fn pico_bench_state(words: &[u32]) -> Option<u32> {
    if words.len() < 2 {
        return None;
    }
    if words[0] != 0x4655_4245 {
        return None;
    }
    Some(words[1])
}

fn print_pico_bench_report(words: &[u32]) {
    if words.len() < 4 || words[0] != 0x4655_4245 {
        println!("Pico benchmark output was missing or malformed");
        return;
    }
    let count = usize::try_from(words[2]).unwrap_or(0);
    println!("pico_benchmark");
    let mut offset = 4usize;
    for _ in 0..count {
        if offset + 4 > words.len() {
            break;
        }
        let bench_id = words[offset];
        let iterations = words[offset + 1];
        let total_nanos = words[offset + 2];
        let average_nanos = words[offset + 3];
        println!(
            "  {} iterations={} total_ns={} avg_ns={}",
            pico_bench_name(bench_id),
            iterations,
            total_nanos,
            average_nanos
        );
        offset += 4;
    }
}

const fn pico_bench_name(bench_id: u32) -> &'static str {
    match bench_id {
        1 => "baseline_direct_noop",
        2 => "current_fiber_pool_spawn_join_noop",
        3 => "current_fiber_pool_spawn_join_yield_once",
        4 => "current_async_runtime_spawn_join_noop",
        5 => "current_async_runtime_spawn_join_yield_once",
        _ => "unknown",
    }
}

fn run_debug_server(options: &CommandOptions) -> Result<(), String> {
    build_example_elf(options)?;
    let chip = options.resolve_chip();
    let elf_path = options.elf_path()?;

    let mut flash = Command::new("probe-rs");
    flash.arg("download");
    flash.arg("--chip").arg(&chip);
    if let Some(probe) = options.resolve_probe_selector() {
        flash.arg("--probe").arg(probe);
    }
    flash.arg(&elf_path);
    run_command(&mut flash, "probe-rs")?;

    println!("ELF : {}", elf_path.display());
    println!("LLDB: connect://{}", options.gdb_connection_string);

    if options.detach {
        let output_dir = options.output_dir()?;
        fs::create_dir_all(&output_dir)
            .map_err(|error| format!("failed to create {}: {error}", output_dir.display()))?;
        let log_path = output_dir.join(format!("{}-debug-server.log", options.bin_name));
        let pid_path = output_dir.join(format!("{}-debug-server.pid", options.bin_name));
        let mut child = spawn_detached_probe_gdb(options, &chip, &log_path)?;
        fs::write(&pid_path, format!("{}\n", child.id()))
            .map_err(|error| format!("failed to write {}: {error}", pid_path.display()))?;

        wait_for_debug_server(&options.gdb_connection_string, &mut child, &log_path)?;

        println!("PID : {}", child.id());
        println!("LOG : {}", log_path.display());
        println!("PIDF: {}", pid_path.display());
        return Ok(());
    }

    let mut command = Command::new("probe-rs");
    command.arg("gdb");
    command.arg("--chip").arg(chip);
    command
        .arg("--gdb-connection-string")
        .arg(&options.gdb_connection_string);
    command.arg("--reset-halt");
    if let Some(probe) = options.resolve_probe_selector() {
        command.arg("--probe").arg(probe);
    }
    run_command(&mut command, "probe-rs")?;
    Ok(())
}

fn spawn_detached_probe_gdb(
    options: &CommandOptions,
    chip: &str,
    log_path: &Path,
) -> Result<std::process::Child, String> {
    #[cfg(unix)]
    {
        let command_line = shell_escape("probe-rs")
            + " gdb --chip "
            + &shell_escape(chip)
            + " --gdb-connection-string "
            + &shell_escape(&options.gdb_connection_string)
            + " --reset-halt";
        let command_line = if let Some(probe) = options.resolve_probe_selector() {
            command_line + " --probe " + &shell_escape(&probe)
        } else {
            command_line
        };

        let mut command = Command::new("setsid");
        command.arg("script");
        command.arg("-qefc");
        command.arg(command_line);
        command.arg(log_path);
        command.stdin(Stdio::null());
        command.stdout(Stdio::null());
        command.stderr(Stdio::null());

        match command.spawn() {
            Ok(child) => return Ok(child),
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                return Err(format!(
                    "failed to launch detached probe-rs gdb via script: {error}"
                ));
            }
            Err(_) => {}
        }
    }

    let log_file = fs::File::create(log_path)
        .map_err(|error| format!("failed to create {}: {error}", log_path.display()))?;
    let stderr_file = log_file
        .try_clone()
        .map_err(|error| format!("failed to clone {}: {error}", log_path.display()))?;

    let mut command = Command::new("probe-rs");
    command.arg("gdb");
    command.arg("--chip").arg(chip);
    command
        .arg("--gdb-connection-string")
        .arg(&options.gdb_connection_string);
    command.arg("--reset-halt");
    if let Some(probe) = options.resolve_probe_selector() {
        command.arg("--probe").arg(probe);
    }
    command.stdin(Stdio::null());
    command.stdout(Stdio::from(log_file));
    command.stderr(Stdio::from(stderr_file));
    command
        .spawn()
        .map_err(|error| format!("failed to launch detached probe-rs gdb: {error}"))
}

fn shell_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len() + 2);
    escaped.push('\'');
    for ch in value.chars() {
        if ch == '\'' {
            escaped.push_str("'\"'\"'");
        } else {
            escaped.push(ch);
        }
    }
    escaped.push('\'');
    escaped
}

fn wait_for_debug_server(
    connection_string: &str,
    child: &mut std::process::Child,
    log_path: &Path,
) -> Result<(), String> {
    let deadline = Instant::now() + StdDuration::from_secs(5);
    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|error| format!("failed to poll detached debug server: {error}"))?
        {
            return Err(format!(
                "detached debug server exited early with {status}; see {}",
                log_path.display()
            ));
        }

        let log_contents = fs::read_to_string(log_path).unwrap_or_default();
        if log_contents.contains("Firing up GDB stub") {
            // The probe-rs stub exits if a TCP client connects and disconnects without speaking
            // GDB remote. Poll the log instead of touching the socket during readiness checks.
            thread::sleep(StdDuration::from_millis(150));
            return Ok(());
        }

        if Instant::now() >= deadline {
            return Err(format!(
                "timed out waiting for detached debug server at {connection_string}; see {}",
                log_path.display()
            ));
        }

        thread::sleep(StdDuration::from_millis(50));
    }
}

fn build_example_elf(options: &CommandOptions) -> Result<(), String> {
    build_example_elf_impl(options, false)
}

fn build_example_elf_with_symbols(options: &CommandOptions) -> Result<(), String> {
    build_example_elf_impl(options, true)
}

fn build_example_elf_impl(options: &CommandOptions, preserve_symbols: bool) -> Result<(), String> {
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
        if preserve_symbols {
            command.arg("--config");
            command.arg("profile.release.strip=\"none\"");
        }
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

fn find_symbol_range(elf_path: &Path, symbol_name: &str) -> Result<(u64, u64), String> {
    let bytes = fs::read(elf_path)
        .map_err(|error| format!("failed to read {}: {error}", elf_path.display()))?;
    let file = object::File::parse(&*bytes)
        .map_err(|error| format!("failed to parse ELF {}: {error}", elf_path.display()))?;
    for symbol in file.symbols() {
        let Ok(name) = symbol.name() else {
            continue;
        };
        if name != symbol_name {
            continue;
        }
        let size = symbol.size();
        if size == 0 {
            return Err(format!(
                "symbol `{symbol_name}` in {} had size 0",
                elf_path.display()
            ));
        }
        return Ok((symbol.address(), size));
    }
    Err(format!(
        "symbol `{symbol_name}` was not found in {}",
        elf_path.display()
    ))
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
