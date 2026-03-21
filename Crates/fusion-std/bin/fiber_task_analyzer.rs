use std::collections::BTreeMap;
use std::env;
use std::fmt::Write as _;
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
    let red_inline_contracts = config
        .red_inline_contracts_path
        .as_ref()
        .map(load_red_inline_contracts)
        .transpose()?
        .unwrap_or_default();

    if let Some(path) = config.report_path.as_ref() {
        write_unknown_symbol_report(path, &outputs.unknown_symbol_report)?;
    }
    if let Some(path) = config.rust_contracts_path.as_ref() {
        let rendered =
            render_rust_contracts(&outputs.generated_entries, config.crate_name.as_deref());
        fs::write(path, rendered)
            .map_err(|error| format!("failed to write {}: {error}", path.display()))?;
    }
    if let Some(path) = config.red_inline_rust_path.as_ref() {
        let rendered = render_red_inline_contracts(&red_inline_contracts);
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
    red_inline_contracts_path: Option<PathBuf>,
    red_inline_rust_path: Option<PathBuf>,
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
        let mut red_inline_contracts_path = None;
        let mut red_inline_rust_path = None;
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
                "--red-inline-contracts" => {
                    red_inline_contracts_path =
                        Some(PathBuf::from(args.next().ok_or_else(|| {
                            usage("missing value for --red-inline-contracts")
                        })?));
                }
                "--red-inline-rust" => {
                    red_inline_rust_path =
                        Some(PathBuf::from(args.next().ok_or_else(|| {
                            usage("missing value for --red-inline-rust")
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
            red_inline_contracts_path,
            red_inline_rust_path,
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct RedInlineContractEntry {
    name: String,
    spans: Vec<u16>,
    fallback_lane: RedInlineFallbackLane,
    fallback_cookie: u32,
    current_exception_stack_bytes: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RedInlineFallbackLane {
    DeferredPrimary,
    DeferredSecondary,
}

#[derive(Debug)]
struct UnknownSymbolContract {
    symbol: String,
    matcher: UnknownSymbolContractMatcher,
    stack_bytes: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UnknownSymbolContractMatcher {
    Exact,
    Prefix,
    Suffix,
    Contains,
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
    callers: Vec<String>,
    roots: Vec<String>,
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
    root_type_name: &'a str,
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
                root_type_name: &root.type_name,
            };
            context
                .resolve_symbol_stack_bytes(&resolved_symbol, None, &mut Vec::new())
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
        let (matcher, symbol) = parse_unknown_symbol_contract_matcher(symbol)
            .ok_or_else(|| format!("contract line {} has invalid wildcard pattern", line_no + 1))?;
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
            matcher,
            stack_bytes,
        });
    }
    Ok(contracts)
}

fn parse_unknown_symbol_contract_matcher(
    raw_symbol: &str,
) -> Option<(UnknownSymbolContractMatcher, &str)> {
    let starts_with_wildcard = raw_symbol.starts_with('*');
    let ends_with_wildcard = raw_symbol.ends_with('*');
    let trimmed = raw_symbol.trim_matches('*');
    if trimmed.is_empty() {
        return None;
    }

    let matcher = match (starts_with_wildcard, ends_with_wildcard) {
        (false, false) => UnknownSymbolContractMatcher::Exact,
        (false, true) => UnknownSymbolContractMatcher::Prefix,
        (true, false) => UnknownSymbolContractMatcher::Suffix,
        (true, true) => UnknownSymbolContractMatcher::Contains,
    };
    Some((matcher, trimmed))
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
        caller: Option<&str>,
        root: Option<&str>,
    ) {
        let metadata = symbol_index.and_then(|index| index.metadata_for_raw(symbol));
        let caller_label = caller.map(|value| display_symbol_label(value, symbol_index));
        let entry = self
            .observations
            .entry(symbol.to_owned())
            .or_insert_with(|| UnknownSymbolObservation {
                raw: symbol.to_owned(),
                demangled: metadata.map(|entry| entry.demangled.clone()),
                normalized_demangled: metadata.map(|entry| entry.normalized_demangled.clone()),
                matched_contract_symbol: matched_contract_symbol.map(ToOwned::to_owned),
                contract_stack_bytes,
                callers: caller_label.clone().into_iter().collect(),
                roots: root.map(ToOwned::to_owned).into_iter().collect(),
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
        if let Some(caller) = caller_label
            && !entry.callers.iter().any(|existing| existing == &caller)
        {
            entry.callers.push(caller);
        }
        if let Some(root) = root
            && !entry.roots.iter().any(|existing| existing == root)
        {
            entry.roots.push(root.to_owned());
        }
    }
}

fn display_symbol_label(symbol: &str, symbol_index: Option<&ArtifactSymbolIndex>) -> String {
    symbol_index
        .and_then(|index| index.metadata_for_raw(symbol))
        .map_or_else(
            || symbol.to_owned(),
            |entry| entry.normalized_demangled.clone(),
        )
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
        caller: Option<&str>,
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
            .resolve_symbol_frame_stack(symbol, caller)
            .ok_or_else(|| format!("missing stack-size entry for symbol `{symbol}`"))?;
        visiting.push(symbol.to_owned());

        let max_callee_stack = if let Some(callees) = self.call_graph.get(symbol) {
            let mut max_stack = 0usize;
            for callee in callees {
                let callee_stack =
                    self.resolve_symbol_stack_bytes(callee, Some(symbol), visiting)?;
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

    fn resolve_symbol_frame_stack(&mut self, symbol: &str, caller: Option<&str>) -> Option<usize> {
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
            caller,
            Some(self.root_type_name),
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

    let mut best_match = None::<(&UnknownSymbolContract, usize)>;
    for contract in contracts {
        let Some(score) =
            unknown_symbol_contract_score(contract, symbol, demangled, normalized_demangled)
        else {
            continue;
        };
        if best_match
            .as_ref()
            .is_none_or(|(_, best_score)| score > *best_score)
        {
            best_match = Some((contract, score));
        }
    }

    best_match.map(|(contract, _)| (contract.rendered_symbol(), contract.stack_bytes))
}

impl UnknownSymbolContract {
    fn rendered_symbol(&self) -> String {
        match self.matcher {
            UnknownSymbolContractMatcher::Exact => self.symbol.clone(),
            UnknownSymbolContractMatcher::Prefix => format!("{}*", self.symbol),
            UnknownSymbolContractMatcher::Suffix => format!("*{}", self.symbol),
            UnknownSymbolContractMatcher::Contains => format!("*{}*", self.symbol),
        }
    }
}

fn unknown_symbol_contract_score(
    contract: &UnknownSymbolContract,
    raw_symbol: &str,
    demangled: Option<&str>,
    normalized_demangled: Option<&str>,
) -> Option<usize> {
    let candidates = [
        raw_symbol,
        demangled.unwrap_or(""),
        normalized_demangled.unwrap_or(""),
    ];
    let mut best = None::<usize>;
    for candidate in candidates {
        if candidate.is_empty() {
            continue;
        }
        if let Some(score) = contract_match_score(contract, candidate) {
            best = Some(best.map_or(score, |current| current.max(score)));
        }
        if let Some(score) = namespace_suffix_contract_score(contract, candidate) {
            best = Some(best.map_or(score, |current| current.max(score)));
        }
    }
    best
}

fn contract_match_score(contract: &UnknownSymbolContract, candidate: &str) -> Option<usize> {
    let literal_len = contract.symbol.len();
    match contract.matcher {
        UnknownSymbolContractMatcher::Exact if candidate == contract.symbol => {
            Some(4_000 + literal_len)
        }
        UnknownSymbolContractMatcher::Prefix if candidate.starts_with(&contract.symbol) => {
            Some(3_000 + literal_len)
        }
        UnknownSymbolContractMatcher::Suffix if candidate.ends_with(&contract.symbol) => {
            Some(2_000 + literal_len)
        }
        UnknownSymbolContractMatcher::Contains if candidate.contains(&contract.symbol) => {
            Some(1_000 + literal_len)
        }
        _ => None,
    }
}

fn namespace_suffix_contract_score(
    contract: &UnknownSymbolContract,
    candidate: &str,
) -> Option<usize> {
    let suffix = format!("::{}", contract.symbol);
    match contract.matcher {
        UnknownSymbolContractMatcher::Exact if candidate.ends_with(&suffix) => {
            Some(3_500 + contract.symbol.len())
        }
        UnknownSymbolContractMatcher::Prefix if candidate.contains(&suffix) => {
            Some(2_500 + contract.symbol.len())
        }
        UnknownSymbolContractMatcher::Contains if candidate.contains(&suffix) => {
            Some(1_500 + contract.symbol.len())
        }
        _ => None,
    }
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
        if !observation.roots.is_empty() {
            rendered.push_str("# roots: ");
            rendered.push_str(&observation.roots.join(", "));
            rendered.push('\n');
        }
        if !observation.callers.is_empty() {
            rendered.push_str("# callers: ");
            rendered.push_str(&observation.callers.join(", "));
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

fn load_red_inline_contracts(path: &PathBuf) -> Result<Vec<RedInlineContractEntry>, String> {
    let contents = fs::read_to_string(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    load_red_inline_contracts_from_str(&contents)
}

fn load_red_inline_contracts_from_str(
    contents: &str,
) -> Result<Vec<RedInlineContractEntry>, String> {
    let mut entries = Vec::new();
    for (line_no, raw_line) in contents.lines().enumerate() {
        let line = raw_line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }

        let (name, rest) = line
            .split_once('=')
            .ok_or_else(|| format!("red inline contract line {} is missing '='", line_no + 1))?;
        let name = name.trim();
        if !is_valid_rust_identifier(name) {
            return Err(format!(
                "red inline contract line {} has invalid Rust identifier `{name}`",
                line_no + 1
            ));
        }

        let parts: Vec<_> = rest.split(';').map(str::trim).collect();
        if !(parts.len() == 3 || parts.len() == 4) {
            return Err(format!(
                "red inline contract line {} must use `<spans> ; <lane> ; <cookie> [; <stack_bytes>]`",
                line_no + 1
            ));
        }

        let mut spans = parse_red_inline_spans(parts[0], line_no + 1)?;
        spans.sort_unstable();
        spans.dedup();

        let fallback_lane = parse_red_inline_fallback_lane(parts[1]).ok_or_else(|| {
            format!(
                "red inline contract line {} has unsupported fallback lane `{}`",
                line_no + 1,
                parts[1]
            )
        })?;
        let fallback_cookie = parse_u32_value(parts[2]).map_err(|error| {
            format!(
                "red inline contract line {} fallback cookie parse failed: {error}",
                line_no + 1
            )
        })?;
        let current_exception_stack_bytes = if parts.len() == 4 {
            parse_stack_size_value(parts[3]).map_err(|error| {
                format!(
                    "red inline contract line {} stack bytes parse failed: {error}",
                    line_no + 1
                )
            })?
        } else {
            0
        };

        entries.push(RedInlineContractEntry {
            name: name.to_owned(),
            spans,
            fallback_lane,
            fallback_cookie,
            current_exception_stack_bytes,
        });
    }
    Ok(entries)
}

fn parse_red_inline_spans(raw: &str, line_no: usize) -> Result<Vec<u16>, String> {
    let raw = raw.trim();
    if raw.is_empty() || raw.eq_ignore_ascii_case("none") || raw == "-" {
        return Ok(Vec::new());
    }

    let mut spans = Vec::new();
    for piece in raw.split(',') {
        let piece = piece.trim();
        if piece.is_empty() {
            return Err(format!(
                "red inline contract line {line_no} contains an empty span identifier"
            ));
        }
        let span = parse_stack_size_value(piece)
            .map_err(|error| format!("span `{piece}` parse failed: {error}"))?;
        if span == 0 || span > usize::from(u16::MAX) {
            return Err(format!(
                "red inline contract line {line_no} span `{piece}` is out of range"
            ));
        }
        spans.push(u16::try_from(span).map_err(|_| {
            format!("red inline contract line {line_no} span `{piece}` is out of range")
        })?);
    }
    Ok(spans)
}

fn parse_red_inline_fallback_lane(raw: &str) -> Option<RedInlineFallbackLane> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "primary" | "deferredprimary" | "deferred-primary" => {
            Some(RedInlineFallbackLane::DeferredPrimary)
        }
        "secondary" | "deferredsecondary" | "deferred-secondary" => {
            Some(RedInlineFallbackLane::DeferredSecondary)
        }
        _ => None,
    }
}

fn is_valid_rust_identifier(raw: &str) -> bool {
    let mut chars = raw.chars();
    match chars.next() {
        Some(first) if first == '_' || first.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn render_red_inline_contracts(entries: &[RedInlineContractEntry]) -> String {
    let mut rendered = String::from(
        "// Generated by fusion_std_fiber_task_analyzer\n\
         // Include this file from a consumer crate to declare red inline compatibility contracts.\n",
    );
    let mut sorted = entries.to_vec();
    sorted.sort_by(|left, right| left.name.cmp(&right.name));
    for entry in &sorted {
        render_red_inline_contract(&mut rendered, entry);
    }
    rendered
}

fn render_red_inline_contract(rendered: &mut String, entry: &RedInlineContractEntry) {
    let (leaf_words, summary_levels) = build_red_inline_summary_tree_words(&entry.spans);
    let leaf_name = format!("{}_REQUIRED_CLEAR_LEAF", entry.name);
    let levels_name = format!("{}_REQUIRED_CLEAR_LEVELS", entry.name);
    let tree_name = format!("{}_REQUIRED_CLEAR_TREE", entry.name);

    rendered.push_str("const ");
    rendered.push_str(&leaf_name);
    rendered.push_str(": [u32; ");
    rendered.push_str(&leaf_words.len().to_string());
    rendered.push_str("] = ");
    rendered.push_str(&render_u32_array(&leaf_words));
    rendered.push_str(";\n");

    for (level_index, words) in summary_levels.iter().enumerate() {
        rendered.push_str("const ");
        let _ = write!(
            rendered,
            "{}_REQUIRED_CLEAR_LEVEL_{level_index}",
            entry.name
        );
        rendered.push_str(": [u32; ");
        rendered.push_str(&words.len().to_string());
        rendered.push_str("] = ");
        rendered.push_str(&render_u32_array(words));
        rendered.push_str(";\n");
    }

    rendered.push_str("const ");
    rendered.push_str(&levels_name);
    rendered.push_str(": [&[u32]; ");
    rendered.push_str(&summary_levels.len().to_string());
    rendered.push_str("] = [");
    for level_index in 0..summary_levels.len() {
        if level_index != 0 {
            rendered.push_str(", ");
        }
        rendered.push('&');
        let _ = write!(
            rendered,
            "{}_REQUIRED_CLEAR_LEVEL_{level_index}",
            entry.name
        );
    }
    rendered.push_str("];\n");

    rendered.push_str("pub const ");
    rendered.push_str(&tree_name);
    rendered.push_str(
        ": fusion_std::thread::CooperativeExclusionSummaryTree = \
         fusion_std::thread::CooperativeExclusionSummaryTree::new(",
    );
    rendered.push('&');
    rendered.push_str(&leaf_name);
    rendered.push_str(", &");
    rendered.push_str(&levels_name);
    rendered.push_str(");\n");

    rendered.push_str("pub const ");
    rendered.push_str(&entry.name);
    rendered.push_str(
        ": fusion_std::thread::RedInlineCompatibility = \
        fusion_std::thread::RedInlineCompatibility::from_summary_tree(&",
    );
    rendered.push_str(&tree_name);
    rendered.push_str(", ");
    rendered.push_str(match entry.fallback_lane {
        RedInlineFallbackLane::DeferredPrimary => {
            "fusion_sys::vector::VectorDispatchLane::DeferredPrimary"
        }
        RedInlineFallbackLane::DeferredSecondary => {
            "fusion_sys::vector::VectorDispatchLane::DeferredSecondary"
        }
    });
    rendered.push_str(", fusion_sys::vector::VectorDispatchCookie(");
    rendered.push_str(&entry.fallback_cookie.to_string());
    rendered.push_str(")).with_current_exception_stack_bytes(");
    rendered.push_str(&entry.current_exception_stack_bytes.to_string());
    rendered.push_str(");\n\n");
}

fn build_red_inline_summary_tree_words(spans: &[u16]) -> (Vec<u32>, Vec<Vec<u32>>) {
    const WORD_BITS: usize = 32;

    let max_span = spans.iter().copied().max().map_or(0, usize::from);
    let mut leaf_words = vec![0_u32; max_span.div_ceil(WORD_BITS)];
    for span in spans {
        let span_index = usize::from(*span - 1);
        leaf_words[span_index / WORD_BITS] |= 1_u32 << (span_index % WORD_BITS);
    }

    let mut summary_levels = Vec::new();
    let mut current = leaf_words.clone();
    while current.len() > 1 {
        let mut parent = vec![0_u32; current.len().div_ceil(WORD_BITS)];
        for (index, word) in current.iter().copied().enumerate() {
            if word != 0 {
                parent[index / WORD_BITS] |= 1_u32 << (index % WORD_BITS);
            }
        }
        summary_levels.push(parent.clone());
        current = parent;
    }

    (leaf_words, summary_levels)
}

fn render_u32_array(words: &[u32]) -> String {
    if words.is_empty() {
        return "[]".to_owned();
    }

    let mut rendered = String::from("[");
    for (index, word) in words.iter().enumerate() {
        if index != 0 {
            rendered.push_str(", ");
        }
        let _ = write!(rendered, "0x{word:08x}");
    }
    rendered.push(']');
    rendered
}

fn parse_u32_value(raw: &str) -> Result<u32, String> {
    let value = parse_stack_size_value(raw)?;
    u32::try_from(value).map_err(|_| "value exceeds u32 range".to_owned())
}

fn parse_stack_size_value(raw: &str) -> Result<usize, String> {
    raw.strip_prefix("0x").map_or_else(
        || raw.parse::<usize>().map_err(|error| error.to_string()),
        |hex| usize::from_str_radix(hex, 16).map_err(|error| error.to_string()),
    )
}

fn usage(reason: &str) -> String {
    format!(
        "{reason}\nusage: cargo run -p fusion-std --bin fiber_task_analyzer -- <roots> <stack-sizes|artifact> <output> [call-graph|artifact] [--contracts <path>] [--report <path>] [--rust-contracts <path>] [--red-inline-contracts <path>] [--red-inline-rust <path>] [--crate-name <name>]"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(clippy::too_many_arguments)]
    fn resolve_stack_for_test(
        symbol: &str,
        root_type_name: &str,
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
            root_type_name,
        };
        context.resolve_symbol_stack_bytes(symbol, None, &mut Vec::new())
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
            "test::RootTask",
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
            "test::RootTask",
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
    fn parses_llvm_objdump_direct_call_targets_without_relocations() {
        let parsed = parse_call_graph(
            "\
/tmp/sample.o:\tfile format elf64-littlearm\n\
\n\
Disassembly of section .text:\n\
\n\
00000000 <leaf>:\n\
       0:\tbx\tlr\n\
\n\
00000008 <root>:\n\
       8:\tbl\t0x0 <leaf>\n\
       c:\tbx\tlr\n",
        )
        .expect("objdump call graph should parse");

        assert_eq!(parsed.get("root"), Some(&vec!["leaf".to_owned()]));
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
             core::panicking::panic_* = 1024\n",
        )
        .expect("contracts should parse");
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].symbol, "memset");
        assert_eq!(parsed[0].matcher, UnknownSymbolContractMatcher::Exact);
        assert_eq!(parsed[1].matcher, UnknownSymbolContractMatcher::Prefix);
        assert_eq!(parsed[1].stack_bytes, 1024);
    }

    #[test]
    fn uses_unknown_symbol_contracts_for_missing_callees() {
        let stack_sizes = parse_stack_sizes("root = 64\n").expect("stack sizes should parse");
        let call_graph = parse_call_graph("root = memset\n").expect("call graph should parse");
        let contracts = load_contracts_from_str("memset = 96\n").expect("contracts should parse");

        let resolved = resolve_stack_for_test(
            "root",
            "test::ContractRootTask",
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
        let contracts = load_contracts_from_str("core::panicking::panic_* = 512\n")
            .expect("contracts should parse");

        let resolved = resolve_unknown_symbol_contract("_RNvCsdX_panic", Some(&index), &contracts);

        assert_eq!(resolved, Some(("core::panicking::panic_*".to_owned(), 512)));
    }

    #[test]
    fn exact_unknown_symbol_contracts_beat_broader_wildcards() {
        let index = parse_llvm_nm_symbol_index(
            "_RNvCsdX_panic\n",
            "core::panicking::panic_cannot_unwind::h1234567890abcdef\n",
        )
        .expect("llvm-nm output should parse");
        let contracts = load_contracts_from_str(
            "core::panicking::panic_* = 512\n\
             core::panicking::panic_cannot_unwind = 768\n",
        )
        .expect("contracts should parse");

        let resolved = resolve_unknown_symbol_contract("_RNvCsdX_panic", Some(&index), &contracts);

        assert_eq!(
            resolved,
            Some(("core::panicking::panic_cannot_unwind".to_owned(), 768))
        );
    }

    #[test]
    fn suffix_unknown_symbol_contracts_match_demangled_namespace_tails() {
        let index = parse_llvm_nm_symbol_index(
            "_RNvCsdX_unwind\n",
            "alloc::task::rust_begin_unwind::habcdef0123456789\n",
        )
        .expect("llvm-nm output should parse");
        let contracts =
            load_contracts_from_str("*rust_begin_unwind = 1024\n").expect("contracts should parse");

        let resolved = resolve_unknown_symbol_contract("_RNvCsdX_unwind", Some(&index), &contracts);

        assert_eq!(resolved, Some(("*rust_begin_unwind".to_owned(), 1024)));
    }

    #[test]
    fn report_records_observed_unknown_symbols() {
        let stack_sizes = parse_stack_sizes("root = 64\n").expect("stack sizes should parse");
        let call_graph = parse_call_graph("root = memset\n").expect("call graph should parse");
        let contracts = load_contracts_from_str("memset = 96\n").expect("contracts should parse");
        let mut report = UnknownSymbolReport::default();

        let resolved = resolve_stack_for_test(
            "root",
            "test::ContractRootTask",
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
        assert_eq!(observation.roots, vec!["test::ContractRootTask".to_owned()]);
        assert_eq!(observation.callers, vec!["root".to_owned()]);
    }

    #[test]
    fn report_renders_todo_for_unmatched_unknown_symbol() {
        let index = parse_llvm_nm_symbol_index(
            "_RNvCsdX_unknown\n",
            "core::mystery::opaque::h1234567890abcdef\n",
        )
        .expect("llvm-nm output should parse");
        let mut report = UnknownSymbolReport::default();
        report.observe(
            "_RNvCsdX_unknown",
            Some(&index),
            None,
            None,
            Some("_RNvCsdX_root"),
            Some("test::UnknownRootTask"),
        );

        let rendered_path = std::env::temp_dir().join("fusion-std-fiber-task-report.txt");
        write_unknown_symbol_report(&rendered_path, &report).expect("report should write");
        let rendered =
            fs::read_to_string(&rendered_path).expect("rendered report should be readable");

        assert!(rendered.contains("# TODO: core::mystery::opaque"));
        assert!(rendered.contains("# roots: test::UnknownRootTask"));
        assert!(rendered.contains("# callers: _RNvCsdX_root"));
    }

    #[test]
    fn parses_red_inline_contracts() {
        let parsed = load_red_inline_contracts_from_str(
            "GPIO_FAST = 3, 4, 1025 ; deferred-primary ; 0x20 ; 256\n\
             GPIO_SLOW = none ; secondary ; 7\n",
        )
        .expect("red inline contracts should parse");

        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].name, "GPIO_FAST");
        assert_eq!(parsed[0].spans, vec![3, 4, 1025]);
        assert_eq!(
            parsed[0].fallback_lane,
            RedInlineFallbackLane::DeferredPrimary
        );
        assert_eq!(parsed[0].fallback_cookie, 0x20);
        assert_eq!(parsed[0].current_exception_stack_bytes, 256);
        assert!(parsed[1].spans.is_empty());
        assert_eq!(
            parsed[1].fallback_lane,
            RedInlineFallbackLane::DeferredSecondary
        );
    }

    #[test]
    fn renders_red_inline_contracts_with_summary_tree() {
        let rendered = render_red_inline_contracts(&[RedInlineContractEntry {
            name: "GPIO_FAST".to_owned(),
            spans: vec![3, 4, 1025],
            fallback_lane: RedInlineFallbackLane::DeferredPrimary,
            fallback_cookie: 17,
            current_exception_stack_bytes: 256,
        }]);

        assert!(rendered.contains("pub const GPIO_FAST_REQUIRED_CLEAR_TREE"));
        assert!(
            rendered.contains("pub const GPIO_FAST: fusion_std::thread::RedInlineCompatibility")
        );
        assert!(rendered.contains("VectorDispatchLane::DeferredPrimary"));
        assert!(rendered.contains("VectorDispatchCookie(17)"));
        assert!(rendered.contains("with_current_exception_stack_bytes(256)"));
    }

    #[test]
    fn builds_red_inline_summary_tree_levels_for_sparse_spans() {
        let (leaf, levels) = build_red_inline_summary_tree_words(&[3, 4, 1025]);

        assert_eq!(leaf.len(), 33);
        assert_eq!(leaf[0], 0b1100);
        assert_eq!(leaf[32], 0b1);
        assert_eq!(levels.len(), 2);
        assert_eq!(levels[0], vec![0b1, 0b1]);
        assert_eq!(levels[1], vec![0b11]);
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
