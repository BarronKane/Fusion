use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    if let Err(error) = run() {
        eprintln!("fusion_std_fiber_task_analyzer: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let config = AnalyzerConfig::parse()?;
    let outputs = generate_outputs(&config)?;

    if let Some(path) = config.report_path.as_ref() {
        write_unknown_symbol_report(path, &outputs.unknown_symbol_report)?;
    }
    if let Some(path) = config.rust_contracts_path.as_ref() {
        let rendered =
            render_rust_contracts(&outputs.generated_entries, config.crate_name.as_deref());
        fs::write(path, rendered)
            .map_err(|error| format!("failed to write {}: {error}", path.display()))?;
    }

    fs::write(&config.output_path, outputs.manifest_output)
        .map_err(|error| format!("failed to write {}: {error}", config.output_path.display()))
}

#[derive(Debug)]
struct AnalyzerConfig {
    roots_path: PathBuf,
    stack_sizes_path: PathBuf,
    output_path: PathBuf,
    call_graph_path: Option<PathBuf>,
    contracts_path: Option<PathBuf>,
    report_path: Option<PathBuf>,
    rust_contracts_path: Option<PathBuf>,
    crate_name: Option<String>,
}

impl AnalyzerConfig {
    fn parse() -> Result<Self, String> {
        let mut args = env::args_os();
        let _program = args.next();
        let roots_path = PathBuf::from(
            args.next()
                .ok_or_else(|| usage("missing roots file argument"))?,
        );
        let stack_sizes_path = PathBuf::from(
            args.next()
                .ok_or_else(|| usage("missing stack-sizes file argument"))?,
        );
        let output_path = PathBuf::from(
            args.next()
                .ok_or_else(|| usage("missing output manifest argument"))?,
        );
        let mut call_graph_path = None;
        let mut contracts_path = None;
        let mut report_path = None;
        let mut rust_contracts_path = None;
        let mut crate_name = None;
        while let Some(arg) = args.next() {
            match arg.to_string_lossy().as_ref() {
                "--contracts" => {
                    contracts_path = Some(PathBuf::from(
                        args.next()
                            .ok_or_else(|| usage("missing value for --contracts"))?,
                    ));
                }
                "--report" => {
                    report_path = Some(PathBuf::from(
                        args.next()
                            .ok_or_else(|| usage("missing value for --report"))?,
                    ));
                }
                "--rust-contracts" => {
                    rust_contracts_path =
                        Some(PathBuf::from(args.next().ok_or_else(|| {
                            usage("missing value for --rust-contracts")
                        })?));
                }
                "--crate-name" => {
                    crate_name = Some(
                        args.next()
                            .ok_or_else(|| usage("missing value for --crate-name"))?
                            .to_string_lossy()
                            .into_owned(),
                    );
                }
                other if other.starts_with("--") => {
                    return Err(usage(&format!("unexpected option `{other}`")));
                }
                _ => {
                    if call_graph_path.is_some() {
                        return Err(usage("unexpected extra positional argument"));
                    }
                    call_graph_path = Some(PathBuf::from(arg));
                }
            }
        }

        Ok(Self {
            roots_path,
            stack_sizes_path,
            output_path,
            call_graph_path,
            contracts_path,
            report_path,
            rust_contracts_path,
            crate_name,
        })
    }
}

#[derive(Debug)]
struct GeneratedOutputs {
    manifest_output: String,
    generated_entries: Vec<GeneratedRustContractEntry>,
    unknown_symbol_report: UnknownSymbolReport,
}

#[derive(Debug)]
struct RootEntry {
    type_name: String,
    symbol: String,
    priority: i8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GeneratedRustContractEntry {
    type_name: String,
    stack_bytes: usize,
    priority: i8,
}

#[derive(Debug)]
struct UnknownSymbolContract {
    symbol: String,
    stack_bytes: usize,
}

#[derive(Debug, Default)]
struct UnknownSymbolReport {
    observations: BTreeMap<String, UnknownSymbolObservation>,
}

#[derive(Debug)]
struct UnknownSymbolObservation {
    raw: String,
    demangled: Option<String>,
    normalized_demangled: Option<String>,
    matched_contract_symbol: Option<String>,
    contract_stack_bytes: Option<usize>,
}

#[derive(Debug)]
struct StackSizeInput {
    stack_sizes: BTreeMap<String, usize>,
    artifact_source: bool,
    symbol_index: Option<ArtifactSymbolIndex>,
}

#[derive(Debug)]
struct ArtifactSymbolIndex {
    entries: Vec<ArtifactSymbolEntry>,
}

#[derive(Debug)]
struct ArtifactSymbolEntry {
    raw: String,
    demangled: String,
    normalized_demangled: String,
}

struct SymbolResolutionContext<'a> {
    stack_sizes: &'a BTreeMap<String, usize>,
    call_graph: &'a BTreeMap<String, Vec<String>>,
    symbol_index: Option<&'a ArtifactSymbolIndex>,
    contracts: &'a [UnknownSymbolContract],
    unknown_symbol_report: &'a mut UnknownSymbolReport,
    aggregated_stack_sizes: &'a mut BTreeMap<String, usize>,
}

fn generate_outputs(config: &AnalyzerConfig) -> Result<GeneratedOutputs, String> {
    let roots = load_roots(&config.roots_path)?;
    let stack_size_input = load_stack_sizes(&config.stack_sizes_path)?;
    let call_graph = config.call_graph_path.as_ref().map_or_else(
        || {
            stack_size_input
                .artifact_source
                .then(|| load_artifact_call_graph(&config.stack_sizes_path))
                .transpose()
        },
        |path| load_call_graph(path).map(Some),
    )?;
    let contracts = config
        .contracts_path
        .as_ref()
        .map(load_contracts)
        .transpose()?
        .unwrap_or_default();
    let mut aggregated_stack_sizes = BTreeMap::new();
    let mut unknown_symbol_report = UnknownSymbolReport::default();
    let mut manifest_output = String::from(
        "# Generated by fusion_std_fiber_task_analyzer\n\
         # type_name = stack_bytes[, priority]\n",
    );
    let mut generated_entries = Vec::new();

    for root in roots {
        let stack_bytes = resolve_root_stack_bytes(
            &root,
            &stack_size_input.stack_sizes,
            stack_size_input.symbol_index.as_ref(),
            call_graph.as_ref(),
            &contracts,
            &mut unknown_symbol_report,
            &mut aggregated_stack_sizes,
        )?;
        manifest_output.push_str(&root.type_name);
        manifest_output.push_str(" = ");
        manifest_output.push_str(&stack_bytes.to_string());
        if root.priority != 0 {
            manifest_output.push_str(", ");
            manifest_output.push_str(&root.priority.to_string());
        }
        manifest_output.push('\n');
        generated_entries.push(GeneratedRustContractEntry {
            type_name: root.type_name,
            stack_bytes,
            priority: root.priority,
        });
    }

    Ok(GeneratedOutputs {
        manifest_output,
        generated_entries,
        unknown_symbol_report,
    })
}

fn resolve_root_stack_bytes(
    root: &RootEntry,
    stack_sizes: &BTreeMap<String, usize>,
    symbol_index: Option<&ArtifactSymbolIndex>,
    call_graph: Option<&BTreeMap<String, Vec<String>>>,
    contracts: &[UnknownSymbolContract],
    unknown_symbol_report: &mut UnknownSymbolReport,
    aggregated_stack_sizes: &mut BTreeMap<String, usize>,
) -> Result<usize, String> {
    let resolved_symbol = resolve_requested_symbol(&root.symbol, stack_sizes, symbol_index)
        .map_err(|error| {
            format!(
                "failed to resolve symbol for `{}` (`{}`): {error}",
                root.type_name, root.symbol
            )
        })?;
    call_graph.map_or_else(
        || Ok(stack_sizes[&resolved_symbol]),
        |graph| {
            let mut context = SymbolResolutionContext {
                stack_sizes,
                call_graph: graph,
                symbol_index,
                contracts,
                unknown_symbol_report,
                aggregated_stack_sizes,
            };
            context
                .resolve_symbol_stack_bytes(&resolved_symbol, &mut Vec::new())
                .map_err(|error| {
                    format!(
                        "failed to resolve worst-case stack for `{}` (`{}`): {error}",
                        root.type_name, root.symbol
                    )
                })
        },
    )
}

fn resolve_requested_symbol(
    requested: &str,
    stack_sizes: &BTreeMap<String, usize>,
    symbol_index: Option<&ArtifactSymbolIndex>,
) -> Result<String, String> {
    if stack_sizes.contains_key(requested) {
        return Ok(requested.to_owned());
    }

    let resolved = match symbol_index {
        Some(index) => index.resolve(requested, stack_sizes)?,
        None => None,
    };
    resolved.ok_or_else(|| format!("missing stack-size entry for symbol `{requested}`"))
}

fn load_roots(path: &PathBuf) -> Result<Vec<RootEntry>, String> {
    let contents = fs::read_to_string(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    let mut roots = Vec::new();
    for (line_no, raw_line) in contents.lines().enumerate() {
        let line = raw_line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }

        let (type_name, rest) = line
            .split_once('=')
            .ok_or_else(|| format!("roots line {} is missing '='", line_no + 1))?;
        let type_name = type_name.trim();
        if type_name.is_empty() {
            return Err(format!("roots line {} has empty type name", line_no + 1));
        }

        let mut parts = rest.split(',').map(str::trim);
        let symbol = parts
            .next()
            .ok_or_else(|| format!("roots line {} is missing symbol", line_no + 1))?;
        if symbol.is_empty() {
            return Err(format!("roots line {} has empty symbol", line_no + 1));
        }
        let priority = match parts.next() {
            Some(raw) if !raw.is_empty() => raw.parse::<i8>().map_err(|error| {
                format!("roots line {} priority parse failed: {error}", line_no + 1)
            })?,
            _ => 0,
        };
        if parts.next().is_some() {
            return Err(format!("roots line {} has too many fields", line_no + 1));
        }

        roots.push(RootEntry {
            type_name: type_name.to_owned(),
            symbol: symbol.to_owned(),
            priority,
        });
    }
    Ok(roots)
}

fn load_stack_sizes(path: &PathBuf) -> Result<StackSizeInput, String> {
    let bytes =
        fs::read(path).map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    match String::from_utf8(bytes) {
        Ok(contents) => Ok(StackSizeInput {
            stack_sizes: parse_stack_sizes(&contents)?,
            artifact_source: false,
            symbol_index: None,
        }),
        Err(_) => Ok(StackSizeInput {
            stack_sizes: load_artifact_stack_sizes(path)?,
            artifact_source: true,
            symbol_index: Some(load_artifact_symbol_index(path)?),
        }),
    }
}

fn load_contracts(path: &PathBuf) -> Result<Vec<UnknownSymbolContract>, String> {
    let contents = fs::read_to_string(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    load_contracts_from_str(&contents)
}

fn load_contracts_from_str(contents: &str) -> Result<Vec<UnknownSymbolContract>, String> {
    let mut contracts = Vec::new();
    for (line_no, raw_line) in contents.lines().enumerate() {
        let line = raw_line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }

        let (symbol, raw_stack_bytes) = line
            .split_once('=')
            .ok_or_else(|| format!("contract line {} is missing '='", line_no + 1))?;
        let symbol = symbol.trim();
        if symbol.is_empty() {
            return Err(format!("contract line {} has empty symbol", line_no + 1));
        }
        let stack_bytes = raw_stack_bytes
            .trim()
            .parse::<usize>()
            .map_err(|error| format!("contract line {} parse failed: {error}", line_no + 1))?;
        if stack_bytes == 0 {
            return Err(format!(
                "contract line {} stack bytes must be non-zero",
                line_no + 1
            ));
        }
        contracts.push(UnknownSymbolContract {
            symbol: symbol.to_owned(),
            stack_bytes,
        });
    }
    Ok(contracts)
}

fn load_call_graph(path: &PathBuf) -> Result<BTreeMap<String, Vec<String>>, String> {
    let bytes =
        fs::read(path).map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    String::from_utf8(bytes).map_or_else(
        |_| load_artifact_call_graph(path),
        |contents| parse_call_graph(&contents),
    )
}

fn parse_stack_sizes(contents: &str) -> Result<BTreeMap<String, usize>, String> {
    if contents.contains("StackSizes [") {
        return parse_llvm_readobj_stack_sizes(contents);
    }

    let mut stack_sizes = BTreeMap::new();
    for (line_no, raw_line) in contents.lines().enumerate() {
        let line = raw_line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }

        let (symbol, stack_bytes) = line
            .split_once('=')
            .ok_or_else(|| format!("stack-size line {} is missing '='", line_no + 1))?;
        let symbol = symbol.trim();
        if symbol.is_empty() {
            return Err(format!("stack-size line {} has empty symbol", line_no + 1));
        }
        let stack_bytes = stack_bytes
            .trim()
            .parse::<usize>()
            .map_err(|error| format!("stack-size line {} parse failed: {error}", line_no + 1))?;
        stack_sizes.insert(symbol.to_owned(), stack_bytes);
    }
    Ok(stack_sizes)
}

fn parse_llvm_readobj_stack_sizes(contents: &str) -> Result<BTreeMap<String, usize>, String> {
    let mut stack_sizes = BTreeMap::new();
    let mut current_functions: Option<Vec<String>> = None;
    let mut current_size: Option<usize> = None;

    for (line_no, raw_line) in contents.lines().enumerate() {
        let line = raw_line.trim();
        if line == "Entry {" {
            current_functions = None;
            current_size = None;
            continue;
        }
        if let Some(rest) = line.strip_prefix("Functions: [") {
            let functions = rest.strip_suffix(']').ok_or_else(|| {
                format!(
                    "llvm stack-size line {} has malformed function list",
                    line_no + 1
                )
            })?;
            let parsed = functions
                .split(',')
                .map(str::trim)
                .filter(|symbol| !symbol.is_empty())
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>();
            if parsed.is_empty() {
                return Err(format!(
                    "llvm stack-size line {} has no functions",
                    line_no + 1
                ));
            }
            current_functions = Some(parsed);
            continue;
        }
        if let Some(rest) = line.strip_prefix("Size: ") {
            current_size = Some(parse_stack_size_value(rest.trim()).map_err(|error| {
                format!("llvm stack-size line {} parse failed: {error}", line_no + 1)
            })?);
            continue;
        }
        if line == "}"
            && let (Some(functions), Some(size)) = (current_functions.take(), current_size.take())
        {
            for function in functions {
                stack_sizes.insert(function, size);
            }
        }
    }

    Ok(stack_sizes)
}

fn parse_call_graph(contents: &str) -> Result<BTreeMap<String, Vec<String>>, String> {
    if contents.contains("Disassembly of section ") {
        return Ok(parse_llvm_objdump_call_graph(contents));
    }

    let mut call_graph = BTreeMap::<String, Vec<String>>::new();

    for (line_no, raw_line) in contents.lines().enumerate() {
        let line = raw_line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }

        let (caller, callees) = line
            .split_once("->")
            .or_else(|| line.split_once('='))
            .ok_or_else(|| format!("call-graph line {} is missing '=' or '->'", line_no + 1))?;
        let caller = caller.trim();
        if caller.is_empty() {
            return Err(format!("call-graph line {} has empty caller", line_no + 1));
        }

        let parsed_callees = callees
            .split(',')
            .map(str::trim)
            .filter(|callee| !callee.is_empty())
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        if parsed_callees.is_empty() {
            return Err(format!("call-graph line {} has no callees", line_no + 1));
        }

        let entry = call_graph.entry(caller.to_owned()).or_default();
        for callee in parsed_callees {
            if !entry.iter().any(|existing| existing == &callee) {
                entry.push(callee);
            }
        }
    }

    Ok(call_graph)
}

fn load_artifact_stack_sizes(path: &Path) -> Result<BTreeMap<String, usize>, String> {
    let output = run_tool("llvm-readobj", ["--stack-sizes"], path)?;
    parse_stack_sizes(&output)
}

fn load_artifact_call_graph(path: &Path) -> Result<BTreeMap<String, Vec<String>>, String> {
    let output = run_tool("llvm-objdump", ["-dr", "--no-show-raw-insn"], path)?;
    Ok(parse_llvm_objdump_call_graph(&output))
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

fn parse_llvm_objdump_call_graph(contents: &str) -> BTreeMap<String, Vec<String>> {
    let mut call_graph = BTreeMap::<String, Vec<String>>::new();
    let mut current_function: Option<String> = None;
    let mut last_instruction_was_call = false;

    for raw_line in contents.lines() {
        let line = raw_line.trim_end();
        if let Some(function) = parse_objdump_function_header(line) {
            current_function = Some(function.to_owned());
            last_instruction_was_call = false;
            continue;
        }

        if let Some(target) = parse_objdump_relocation_target(line) {
            if last_instruction_was_call && let Some(caller) = current_function.as_ref() {
                let entry = call_graph.entry(caller.clone()).or_default();
                if !entry.iter().any(|existing| existing == target) {
                    entry.push(target.to_owned());
                }
            }
            last_instruction_was_call = false;
            continue;
        }

        if let Some(mnemonic) = parse_objdump_instruction_mnemonic(line) {
            last_instruction_was_call = instruction_maybe_calls(mnemonic);
        }
    }

    call_graph
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
    {
        return None;
    }
    if trimmed.starts_with('<') {
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
            demangled: demangled.to_owned(),
            normalized_demangled: normalize_demangled_symbol(demangled).to_owned(),
        })
        .collect::<Vec<_>>();
    Ok(ArtifactSymbolIndex { entries })
}

fn normalize_demangled_symbol(symbol: &str) -> &str {
    let Some((prefix, suffix)) = symbol.rsplit_once("::h") else {
        return symbol;
    };
    if suffix.len() == 16 && suffix.chars().all(|ch| ch.is_ascii_hexdigit()) {
        prefix
    } else {
        symbol
    }
}

impl ArtifactSymbolIndex {
    fn resolve(
        &self,
        requested: &str,
        stack_sizes: &BTreeMap<String, usize>,
    ) -> Result<Option<String>, String> {
        let mut exact_raw = Vec::new();
        let mut exact_demangled = Vec::new();
        let mut demangled_suffix = Vec::new();
        let mut substring = Vec::new();
        let demangled_suffix_pattern = format!("::{requested}");

        for entry in &self.entries {
            if !stack_sizes.contains_key(&entry.raw) {
                continue;
            }

            if entry.raw == requested {
                exact_raw.push(entry.raw.clone());
                continue;
            }
            if entry.demangled == requested || entry.normalized_demangled == requested {
                exact_demangled.push(entry.raw.clone());
                continue;
            }
            if entry.demangled.ends_with(&demangled_suffix_pattern)
                || entry
                    .normalized_demangled
                    .ends_with(&demangled_suffix_pattern)
            {
                demangled_suffix.push(entry.raw.clone());
                continue;
            }
            if entry.raw.contains(requested)
                || entry.demangled.contains(requested)
                || entry.normalized_demangled.contains(requested)
            {
                substring.push(entry.raw.clone());
            }
        }

        for (label, candidates) in [
            ("raw", exact_raw),
            ("demangled", exact_demangled),
            ("demangled suffix", demangled_suffix),
            ("substring", substring),
        ] {
            if let Some(symbol) = unique_candidate(requested, label, candidates)? {
                return Ok(Some(symbol));
            }
        }

        Ok(None)
    }

    fn metadata_for_raw<'a>(&'a self, raw: &str) -> Option<&'a ArtifactSymbolEntry> {
        self.entries.iter().find(|entry| entry.raw == raw)
    }
}

impl UnknownSymbolReport {
    fn observe(
        &mut self,
        symbol: &str,
        symbol_index: Option<&ArtifactSymbolIndex>,
        matched_contract_symbol: Option<&str>,
        contract_stack_bytes: Option<usize>,
    ) {
        let metadata = symbol_index.and_then(|index| index.metadata_for_raw(symbol));
        let entry = self
            .observations
            .entry(symbol.to_owned())
            .or_insert_with(|| UnknownSymbolObservation {
                raw: symbol.to_owned(),
                demangled: metadata.map(|entry| entry.demangled.clone()),
                normalized_demangled: metadata.map(|entry| entry.normalized_demangled.clone()),
                matched_contract_symbol: matched_contract_symbol.map(ToOwned::to_owned),
                contract_stack_bytes,
            });
        if entry.demangled.is_none() {
            entry.demangled = metadata.map(|meta| meta.demangled.clone());
        }
        if entry.normalized_demangled.is_none() {
            entry.normalized_demangled = metadata.map(|meta| meta.normalized_demangled.clone());
        }
        if entry.matched_contract_symbol.is_none() {
            entry.matched_contract_symbol = matched_contract_symbol.map(ToOwned::to_owned);
        }
        if entry.contract_stack_bytes.is_none() {
            entry.contract_stack_bytes = contract_stack_bytes;
        }
    }
}

fn unique_candidate(
    requested: &str,
    label: &str,
    mut candidates: Vec<String>,
) -> Result<Option<String>, String> {
    candidates.sort();
    candidates.dedup();
    match candidates.len() {
        0 => Ok(None),
        1 => Ok(candidates.pop()),
        _ => Err(format!(
            "ambiguous {label} symbol match for `{requested}`: {}",
            candidates.join(", ")
        )),
    }
}

impl SymbolResolutionContext<'_> {
    fn resolve_symbol_stack_bytes(
        &mut self,
        symbol: &str,
        visiting: &mut Vec<String>,
    ) -> Result<usize, String> {
        if let Some(stack_bytes) = self.aggregated_stack_sizes.get(symbol).copied() {
            return Ok(stack_bytes);
        }

        if let Some(cycle_start) = visiting.iter().position(|entry| entry == symbol) {
            let mut cycle = visiting[cycle_start..].to_vec();
            cycle.push(symbol.to_owned());
            return Err(format!("call-graph cycle detected: {}", cycle.join(" -> ")));
        }

        let frame_stack = self
            .resolve_symbol_frame_stack(symbol)
            .ok_or_else(|| format!("missing stack-size entry for symbol `{symbol}`"))?;
        visiting.push(symbol.to_owned());

        let max_callee_stack = if let Some(callees) = self.call_graph.get(symbol) {
            let mut max_stack = 0usize;
            for callee in callees {
                let callee_stack = self.resolve_symbol_stack_bytes(callee, visiting)?;
                max_stack = max_stack.max(callee_stack);
            }
            max_stack
        } else {
            0
        };

        visiting.pop();
        let total_stack = frame_stack
            .checked_add(max_callee_stack)
            .ok_or_else(|| format!("stack-size aggregation overflow for symbol `{symbol}`"))?;
        self.aggregated_stack_sizes
            .insert(symbol.to_owned(), total_stack);
        Ok(total_stack)
    }

    fn resolve_symbol_frame_stack(&mut self, symbol: &str) -> Option<usize> {
        if let Some(stack_bytes) = self.stack_sizes.get(symbol).copied() {
            return Some(stack_bytes);
        }

        let resolved = resolve_unknown_symbol_contract(symbol, self.symbol_index, self.contracts);
        let matched_contract_symbol = resolved
            .as_ref()
            .map(|(contract_symbol, _)| contract_symbol.as_str());
        let contract_stack_bytes = resolved.as_ref().map(|(_, stack_bytes)| *stack_bytes);
        self.unknown_symbol_report.observe(
            symbol,
            self.symbol_index,
            matched_contract_symbol,
            contract_stack_bytes,
        );
        contract_stack_bytes
    }
}

fn resolve_unknown_symbol_contract(
    symbol: &str,
    symbol_index: Option<&ArtifactSymbolIndex>,
    contracts: &[UnknownSymbolContract],
) -> Option<(String, usize)> {
    let metadata = symbol_index.and_then(|index| index.metadata_for_raw(symbol));
    let demangled = metadata.map(|entry| entry.demangled.as_str());
    let normalized_demangled = metadata.map(|entry| entry.normalized_demangled.as_str());

    contracts
        .iter()
        .find(|contract| {
            contract.symbol == symbol
                || demangled.is_some_and(|value| contract.symbol == value)
                || normalized_demangled.is_some_and(|value| contract.symbol == value)
                || demangled.is_some_and(|value| value.ends_with(&format!("::{}", contract.symbol)))
                || normalized_demangled
                    .is_some_and(|value| value.ends_with(&format!("::{}", contract.symbol)))
        })
        .map(|contract| (contract.symbol.clone(), contract.stack_bytes))
}

fn write_unknown_symbol_report(path: &Path, report: &UnknownSymbolReport) -> Result<(), String> {
    let mut rendered = String::from(
        "# Unknown external/toolchain symbols observed during stack aggregation\n\
         # contract_symbol = stack_bytes\n\
         # Entries without a matched contract are emitted as commented TODO lines.\n",
    );

    for observation in report.observations.values() {
        rendered.push_str("# raw: ");
        rendered.push_str(&observation.raw);
        rendered.push('\n');
        if let Some(demangled) = observation.demangled.as_ref() {
            rendered.push_str("# demangled: ");
            rendered.push_str(demangled);
            rendered.push('\n');
        }
        if let Some(normalized) = observation.normalized_demangled.as_ref() {
            rendered.push_str("# normalized: ");
            rendered.push_str(normalized);
            rendered.push('\n');
        }
        if let (Some(contract_symbol), Some(stack_bytes)) = (
            observation.matched_contract_symbol.as_ref(),
            observation.contract_stack_bytes,
        ) {
            rendered.push_str(contract_symbol);
            rendered.push_str(" = ");
            rendered.push_str(&stack_bytes.to_string());
            rendered.push('\n');
        } else {
            let suggested_symbol = observation
                .normalized_demangled
                .as_deref()
                .or(observation.demangled.as_deref())
                .unwrap_or(&observation.raw);
            rendered.push_str("# TODO: ");
            rendered.push_str(suggested_symbol);
            rendered.push_str(" = <stack-bytes>\n");
        }
        rendered.push('\n');
    }

    fs::write(path, rendered)
        .map_err(|error| format!("failed to write {}: {error}", path.display()))
}

fn render_rust_contracts(
    entries: &[GeneratedRustContractEntry],
    crate_name: Option<&str>,
) -> String {
    let mut rendered = String::from(
        "// Generated by fusion_std_fiber_task_analyzer\n\
         // Include this file from a consumer crate to declare generated fiber-task contracts.\n",
    );
    for entry in entries {
        rendered.push_str("fusion_std::declare_generated_fiber_task_contract!(\n");
        rendered.push_str("    ");
        rendered.push_str(&render_contract_type_path(&entry.type_name, crate_name));
        rendered.push_str(",\n");
        rendered.push_str("    core::num::NonZeroUsize::new(");
        rendered.push_str(&entry.stack_bytes.to_string());
        rendered.push_str(").unwrap(),\n");
        rendered.push_str("    fusion_std::thread::FiberTaskPriority::new(");
        rendered.push_str(&entry.priority.to_string());
        rendered.push_str("),\n");
        rendered.push_str(");\n\n");
    }
    rendered
}

fn render_contract_type_path(type_name: &str, crate_name: Option<&str>) -> String {
    match crate_name {
        Some(crate_name) => {
            let prefix = format!("{crate_name}::");
            if let Some(suffix) = type_name.strip_prefix(&prefix) {
                return format!("crate::{suffix}");
            }
            if type_name == crate_name {
                return "crate".to_owned();
            }
            type_name.to_owned()
        }
        None => type_name.to_owned(),
    }
}

fn parse_stack_size_value(raw: &str) -> Result<usize, String> {
    raw.strip_prefix("0x").map_or_else(
        || raw.parse::<usize>().map_err(|error| error.to_string()),
        |hex| usize::from_str_radix(hex, 16).map_err(|error| error.to_string()),
    )
}

fn usage(reason: &str) -> String {
    format!(
        "{reason}\nusage: cargo run -p fusion-std --bin fiber_task_analyzer -- <roots> <stack-sizes|artifact> <output> [call-graph|artifact] [--contracts <path>] [--report <path>] [--rust-contracts <path>] [--crate-name <name>]"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolve_stack_for_test(
        symbol: &str,
        stack_sizes: &BTreeMap<String, usize>,
        call_graph: &BTreeMap<String, Vec<String>>,
        symbol_index: Option<&ArtifactSymbolIndex>,
        contracts: &[UnknownSymbolContract],
        report: &mut UnknownSymbolReport,
        aggregated_stack_sizes: &mut BTreeMap<String, usize>,
    ) -> Result<usize, String> {
        let mut context = SymbolResolutionContext {
            stack_sizes,
            call_graph,
            symbol_index,
            contracts,
            unknown_symbol_report: report,
            aggregated_stack_sizes,
        };
        context.resolve_symbol_stack_bytes(symbol, &mut Vec::new())
    }

    #[test]
    fn parses_simple_symbol_equals_bytes_format() {
        let parsed = parse_stack_sizes(
            "foo = 8192\n\
             bar = 4096\n",
        )
        .expect("simple stack-size format should parse");
        assert_eq!(parsed.get("foo"), Some(&8192));
        assert_eq!(parsed.get("bar"), Some(&4096));
    }

    #[test]
    fn parses_llvm_readobj_stack_sizes_format() {
        let parsed = parse_stack_sizes(
            "File: sample.o\n\
             StackSizes [\n\
               Entry {\n\
                 Functions: [foo]\n\
                 Size: 0x48\n\
               }\n\
               Entry {\n\
                 Functions: [bar, baz]\n\
                 Size: 16\n\
               }\n\
             ]\n",
        )
        .expect("llvm-readobj stack-size format should parse");
        assert_eq!(parsed.get("foo"), Some(&0x48));
        assert_eq!(parsed.get("bar"), Some(&16));
        assert_eq!(parsed.get("baz"), Some(&16));
    }

    #[test]
    fn parses_call_graph_in_equals_or_arrow_form() {
        let parsed = parse_call_graph(
            "root = foo, bar\n\
             foo -> leaf\n",
        )
        .expect("call graph should parse");
        assert_eq!(
            parsed.get("root"),
            Some(&vec!["foo".to_owned(), "bar".to_owned()])
        );
        assert_eq!(parsed.get("foo"), Some(&vec!["leaf".to_owned()]));
    }

    #[test]
    fn aggregates_worst_case_stack_across_call_graph() {
        let stack_sizes = parse_stack_sizes(
            "root = 64\n\
             foo = 32\n\
             bar = 48\n\
             leaf = 8\n",
        )
        .expect("stack sizes should parse");
        let call_graph = parse_call_graph(
            "root = foo, bar\n\
             foo = leaf\n",
        )
        .expect("call graph should parse");

        let mut aggregated_stack_sizes = BTreeMap::new();
        let resolved = resolve_stack_for_test(
            "root",
            &stack_sizes,
            &call_graph,
            None,
            &[],
            &mut UnknownSymbolReport::default(),
            &mut aggregated_stack_sizes,
        )
        .expect("aggregation should succeed");

        assert_eq!(resolved, 64 + 48);
        assert_eq!(aggregated_stack_sizes.get("foo"), Some(&(32 + 8)));
    }

    #[test]
    fn rejects_recursive_call_graph_cycles() {
        let stack_sizes = parse_stack_sizes(
            "root = 64\n\
             mid = 32\n",
        )
        .expect("stack sizes should parse");
        let call_graph = parse_call_graph(
            "root = mid\n\
             mid = root\n",
        )
        .expect("call graph should parse");

        let error = resolve_stack_for_test(
            "root",
            &stack_sizes,
            &call_graph,
            None,
            &[],
            &mut UnknownSymbolReport::default(),
            &mut BTreeMap::new(),
        )
        .expect_err("cycles should fail");

        assert!(error.contains("call-graph cycle detected: root -> mid -> root"));
    }

    #[test]
    fn parses_llvm_objdump_call_graph_output() {
        let parsed = parse_call_graph(
            "\
/tmp/sample.o:\tfile format elf64-x86-64\n\
\n\
Disassembly of section .text:\n\
\n\
0000000000000000 <leaf>:\n\
       0:\tpushq\t%rbp\n\
\n\
0000000000000010 <mid>:\n\
      10:\tpushq\t%rbp\n\
      1e:\tcallq\t0x23 <mid+0x13>\n\
\t\t000000000000001f:  R_X86_64_PLT32\tleaf-0x4\n\
\n\
0000000000000030 <root>:\n\
      3e:\tcallq\t0x43 <root+0x13>\n\
\t\t000000000000003f:  R_X86_64_PLT32\tmid-0x4\n\
      49:\tcallq\t0x4e <root+0x1e>\n\
\t\t000000000000004a:  R_X86_64_PLT32\tleaf-0x4\n",
        )
        .expect("objdump call graph should parse");

        assert_eq!(parsed.get("mid"), Some(&vec!["leaf".to_owned()]));
        assert_eq!(
            parsed.get("root"),
            Some(&vec!["mid".to_owned(), "leaf".to_owned()])
        );
    }

    #[test]
    fn normalizes_relocation_targets() {
        assert_eq!(normalize_relocation_target("leaf-0x4"), Some("leaf"));
        assert_eq!(normalize_relocation_target("mid+0x8"), Some("mid"));
        assert_eq!(normalize_relocation_target("foo@PLT"), Some("foo"));
        assert_eq!(
            normalize_relocation_target(".text._RNvCsdX_anchor"),
            Some("_RNvCsdX_anchor")
        );
    }

    #[test]
    fn parses_llvm_nm_symbol_pairs() {
        let index = parse_llvm_nm_symbol_index(
            "_RNvCsdX_anchor\n\
             _RNvCsdX_other\n",
            "fusion_std::thread::fiber::GeneratedFiberTaskMetadataAnchorTask::run::h1234567890abcdef\n\
             other::symbol::hfedcba0987654321\n",
        )
        .expect("llvm-nm output should parse");

        assert_eq!(index.entries.len(), 2);
        assert_eq!(
            index.entries[0].normalized_demangled,
            "fusion_std::thread::fiber::GeneratedFiberTaskMetadataAnchorTask::run"
        );
    }

    #[test]
    fn resolves_requested_symbol_from_unique_demangled_suffix() {
        let index = parse_llvm_nm_symbol_index(
            "_RNvCsdX_anchor\n",
            "fusion_std::thread::fiber::generated_fiber_task_metadata_anchor::h1234567890abcdef\n",
        )
        .expect("llvm-nm output should parse");
        let stack_sizes = BTreeMap::from([("_RNvCsdX_anchor".to_owned(), 8192_usize)]);

        let resolved = resolve_requested_symbol(
            "generated_fiber_task_metadata_anchor",
            &stack_sizes,
            Some(&index),
        )
        .expect("resolver should succeed");

        assert_eq!(resolved, "_RNvCsdX_anchor");
    }

    #[test]
    fn rejects_ambiguous_requested_symbol_matches() {
        let index = parse_llvm_nm_symbol_index(
            "_RNvCsdX_anchor_a\n\
             _RNvCsdX_anchor_b\n",
            "foo::generated_fiber_task_metadata_anchor::h1234567890abcdef\n\
             bar::generated_fiber_task_metadata_anchor::hfedcba0987654321\n",
        )
        .expect("llvm-nm output should parse");
        let stack_sizes = BTreeMap::from([
            ("_RNvCsdX_anchor_a".to_owned(), 4096_usize),
            ("_RNvCsdX_anchor_b".to_owned(), 8192_usize),
        ]);

        let error = resolve_requested_symbol(
            "generated_fiber_task_metadata_anchor",
            &stack_sizes,
            Some(&index),
        )
        .expect_err("ambiguous match should fail");

        assert!(error.contains("ambiguous demangled suffix symbol match"));
    }

    #[test]
    fn parses_unknown_symbol_contracts() {
        let parsed = load_contracts_from_str(
            "memset = 256\n\
             core::panicking::panic_cannot_unwind = 1024\n",
        )
        .expect("contracts should parse");
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].symbol, "memset");
        assert_eq!(parsed[1].stack_bytes, 1024);
    }

    #[test]
    fn uses_unknown_symbol_contracts_for_missing_callees() {
        let stack_sizes = parse_stack_sizes("root = 64\n").expect("stack sizes should parse");
        let call_graph = parse_call_graph("root = memset\n").expect("call graph should parse");
        let contracts = load_contracts_from_str("memset = 96\n").expect("contracts should parse");

        let resolved = resolve_stack_for_test(
            "root",
            &stack_sizes,
            &call_graph,
            None,
            &contracts,
            &mut UnknownSymbolReport::default(),
            &mut BTreeMap::new(),
        )
        .expect("contract-backed aggregation should succeed");

        assert_eq!(resolved, 160);
    }

    #[test]
    fn uses_demangled_unknown_symbol_contracts() {
        let index = parse_llvm_nm_symbol_index(
            "_RNvCsdX_panic\n",
            "core::panicking::panic_cannot_unwind::h1234567890abcdef\n",
        )
        .expect("llvm-nm output should parse");
        let contracts = load_contracts_from_str("core::panicking::panic_cannot_unwind = 512\n")
            .expect("contracts should parse");

        let resolved = resolve_unknown_symbol_contract("_RNvCsdX_panic", Some(&index), &contracts);

        assert_eq!(
            resolved,
            Some(("core::panicking::panic_cannot_unwind".to_owned(), 512))
        );
    }

    #[test]
    fn report_records_observed_unknown_symbols() {
        let stack_sizes = parse_stack_sizes("root = 64\n").expect("stack sizes should parse");
        let call_graph = parse_call_graph("root = memset\n").expect("call graph should parse");
        let contracts = load_contracts_from_str("memset = 96\n").expect("contracts should parse");
        let mut report = UnknownSymbolReport::default();

        let resolved = resolve_stack_for_test(
            "root",
            &stack_sizes,
            &call_graph,
            None,
            &contracts,
            &mut report,
            &mut BTreeMap::new(),
        )
        .expect("contract-backed aggregation should succeed");

        assert_eq!(resolved, 160);
        assert!(report.observations.contains_key("memset"));
        let observation = report
            .observations
            .get("memset")
            .expect("observation should exist");
        assert_eq!(
            observation.matched_contract_symbol.as_deref(),
            Some("memset")
        );
        assert_eq!(observation.contract_stack_bytes, Some(96));
    }

    #[test]
    fn report_renders_todo_for_unmatched_unknown_symbol() {
        let index = parse_llvm_nm_symbol_index(
            "_RNvCsdX_unknown\n",
            "core::mystery::opaque::h1234567890abcdef\n",
        )
        .expect("llvm-nm output should parse");
        let mut report = UnknownSymbolReport::default();
        report.observe("_RNvCsdX_unknown", Some(&index), None, None);

        let rendered_path = std::env::temp_dir().join("fusion-std-fiber-task-report.txt");
        write_unknown_symbol_report(&rendered_path, &report).expect("report should write");
        let rendered =
            fs::read_to_string(&rendered_path).expect("rendered report should be readable");

        assert!(rendered.contains("# TODO: core::mystery::opaque"));
    }

    #[test]
    fn renders_rust_contracts_with_crate_relative_type_paths() {
        let rendered = render_rust_contracts(
            &[GeneratedRustContractEntry {
                type_name: "fusion_std::thread::fiber::GeneratedFiberTaskMetadataAnchorTask"
                    .to_owned(),
                stack_bytes: 8192,
                priority: 5,
            }],
            Some("fusion_std"),
        );

        assert!(rendered.contains("fusion_std::declare_generated_fiber_task_contract!("));
        assert!(rendered.contains("crate::thread::fiber::GeneratedFiberTaskMetadataAnchorTask"));
        assert!(rendered.contains("core::num::NonZeroUsize::new(8192).unwrap()"));
        assert!(rendered.contains("fusion_std::thread::FiberTaskPriority::new(5)"));
    }

    #[test]
    fn renders_rust_contracts_with_absolute_type_paths_without_crate_name() {
        let rendered = render_rust_contracts(
            &[GeneratedRustContractEntry {
                type_name: "external_crate::task::ExternalTask".to_owned(),
                stack_bytes: 4096,
                priority: 0,
            }],
            None,
        );

        assert!(rendered.contains("external_crate::task::ExternalTask"));
    }
}
