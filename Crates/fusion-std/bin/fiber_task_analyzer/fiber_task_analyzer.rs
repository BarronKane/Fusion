use std::collections::{
    BTreeMap,
    BTreeSet,
};
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
    if let Some(path) = config.async_poll_stack_output_path.as_ref() {
        let rendered = outputs
            .async_poll_stack_manifest_output
            .as_ref()
            .ok_or_else(|| "missing generated async poll-stack manifest".to_owned())?;
        fs::write(path, rendered)
            .map_err(|error| format!("failed to write {}: {error}", path.display()))?;
    }
    if let Some(path) = config.async_poll_stack_rust_path.as_ref() {
        let rendered = outputs
            .async_poll_stack_rust_output
            .as_ref()
            .ok_or_else(|| "missing generated async poll-stack Rust contracts".to_owned())?;
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
    aux_artifact_paths: Vec<PathBuf>,
    contracts_path: Option<PathBuf>,
    report_path: Option<PathBuf>,
    rust_contracts_path: Option<PathBuf>,
    red_inline_contracts_path: Option<PathBuf>,
    red_inline_rust_path: Option<PathBuf>,
    async_poll_stack_roots_path: Option<PathBuf>,
    async_poll_stack_output_path: Option<PathBuf>,
    async_poll_stack_rust_path: Option<PathBuf>,
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
        let mut aux_artifact_paths = Vec::new();
        let mut contracts_path = None;
        let mut report_path = None;
        let mut rust_contracts_path = None;
        let mut red_inline_contracts_path = None;
        let mut red_inline_rust_path = None;
        let mut async_poll_stack_roots_path = None;
        let mut async_poll_stack_output_path = None;
        let mut async_poll_stack_rust_path = None;
        let mut crate_name = None;
        while let Some(arg) = args.next() {
            match arg.to_string_lossy().as_ref() {
                "--contracts" => {
                    contracts_path = Some(PathBuf::from(
                        args.next()
                            .ok_or_else(|| usage("missing value for --contracts"))?,
                    ));
                }
                "--aux-artifact" => {
                    aux_artifact_paths.push(PathBuf::from(
                        args.next()
                            .ok_or_else(|| usage("missing value for --aux-artifact"))?,
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
                "--async-poll-stack-roots" => {
                    async_poll_stack_roots_path =
                        Some(PathBuf::from(args.next().ok_or_else(|| {
                            usage("missing value for --async-poll-stack-roots")
                        })?));
                }
                "--async-poll-stack-output" => {
                    async_poll_stack_output_path =
                        Some(PathBuf::from(args.next().ok_or_else(|| {
                            usage("missing value for --async-poll-stack-output")
                        })?));
                }
                "--async-poll-stack-rust" => {
                    async_poll_stack_rust_path =
                        Some(PathBuf::from(args.next().ok_or_else(|| {
                            usage("missing value for --async-poll-stack-rust")
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

        match (
            async_poll_stack_roots_path.is_some(),
            async_poll_stack_output_path.is_some(),
        ) {
            (true, false) if async_poll_stack_rust_path.is_none() => {
                return Err(usage(
                    "missing --async-poll-stack-output/--async-poll-stack-rust for async poll-stack roots",
                ));
            }
            (false, true) => {
                return Err(usage(
                    "missing --async-poll-stack-roots for async poll-stack output",
                ));
            }
            _ => {}
        }
        if async_poll_stack_roots_path.is_none() && async_poll_stack_rust_path.is_some() {
            return Err(usage(
                "missing --async-poll-stack-roots for async poll-stack Rust contracts",
            ));
        }

        Ok(Self {
            roots_path,
            stack_sizes_path,
            output_path,
            call_graph_path,
            aux_artifact_paths,
            contracts_path,
            report_path,
            rust_contracts_path,
            red_inline_contracts_path,
            red_inline_rust_path,
            async_poll_stack_roots_path,
            async_poll_stack_output_path,
            async_poll_stack_rust_path,
            crate_name,
        })
    }
}

#[derive(Debug)]
struct GeneratedOutputs {
    manifest_output: String,
    generated_entries: Vec<GeneratedRustContractEntry>,
    async_poll_stack_manifest_output: Option<String>,
    async_poll_stack_rust_output: Option<String>,
    unknown_symbol_report: UnknownSymbolReport,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GeneratedAsyncPollStackRustContractEntry {
    type_name: String,
    poll_stack_bytes: usize,
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
    execution: GeneratedTaskExecution,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GeneratedTaskExecution {
    Fiber,
    InlineNoYield,
}

impl GeneratedTaskExecution {
    const fn manifest_field(self) -> &'static str {
        match self {
            Self::Fiber => "fiber",
            Self::InlineNoYield => "inline-no-yield",
        }
    }

    const fn rust_path(self) -> &'static str {
        match self {
            Self::Fiber => "fusion_std::thread::FiberTaskExecution::Fiber",
            Self::InlineNoYield => "fusion_std::thread::FiberTaskExecution::InlineNoYield",
        }
    }
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
    let async_poll_stack_roots = config
        .async_poll_stack_roots_path
        .as_ref()
        .map(load_roots)
        .transpose()?
        .unwrap_or_default();
    let mut stack_size_input = load_stack_sizes(&config.stack_sizes_path)?;
    let mut artifact_paths = Vec::new();
    if stack_size_input.artifact_source {
        artifact_paths.push(config.stack_sizes_path.clone());
    }
    for path in &config.aux_artifact_paths {
        let auxiliary_input = load_stack_sizes(path)?;
        let auxiliary_is_artifact = auxiliary_input.artifact_source;
        merge_stack_size_inputs(&mut stack_size_input, auxiliary_input);
        if auxiliary_is_artifact {
            artifact_paths.push(path.clone());
        }
    }
    let call_graph = config.call_graph_path.as_ref().map_or_else(
        || load_merged_artifact_call_graph(&artifact_paths),
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
    let mut merged_entries = BTreeMap::<String, GeneratedRustContractEntry>::new();
    let mut manifest_output = String::from(
        "# Generated by fusion_std_fiber_task_analyzer\n\
         # type_name = stack_bytes[, priority][, execution]\n",
    );

    for root in roots {
        let type_name = root.type_name.clone();
        let stack_bytes = resolve_root_stack_bytes(
            &root,
            &stack_size_input.stack_sizes,
            stack_size_input.symbol_index.as_ref(),
            call_graph.as_ref(),
            &contracts,
            &mut unknown_symbol_report,
            &mut aggregated_stack_sizes,
        )?
        .max(1);
        let execution = resolve_root_execution(
            &root,
            &stack_size_input.stack_sizes,
            stack_size_input.symbol_index.as_ref(),
            call_graph.as_ref(),
        )?;
        match merged_entries.entry(type_name.clone()) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(GeneratedRustContractEntry {
                    type_name,
                    stack_bytes,
                    priority: root.priority,
                    execution,
                });
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                if entry.get().priority != root.priority {
                    return Err(format!(
                        "conflicting priorities for generated task `{}`: {} vs {}",
                        entry.get().type_name,
                        entry.get().priority,
                        root.priority
                    ));
                }
                if stack_bytes > entry.get().stack_bytes {
                    entry.get_mut().stack_bytes = stack_bytes;
                }
                if execution == GeneratedTaskExecution::Fiber {
                    entry.get_mut().execution = GeneratedTaskExecution::Fiber;
                }
            }
        }
    }

    let mut generated_entries = merged_entries.into_values().collect::<Vec<_>>();
    generated_entries.sort_by(|left, right| left.type_name.cmp(&right.type_name));
    for entry in &generated_entries {
        manifest_output.push_str(&entry.type_name);
        manifest_output.push_str(" = ");
        manifest_output.push_str(&entry.stack_bytes.to_string());
        if entry.priority != 0 {
            manifest_output.push_str(", ");
            manifest_output.push_str(&entry.priority.to_string());
        }
        if entry.execution != GeneratedTaskExecution::Fiber {
            manifest_output.push_str(", ");
            manifest_output.push_str(entry.execution.manifest_field());
        }
        manifest_output.push('\n');
    }

    let async_poll_stack_entries = if async_poll_stack_roots.is_empty() {
        Vec::new()
    } else {
        build_async_poll_stack_entries(
            &async_poll_stack_roots,
            &stack_size_input.stack_sizes,
            stack_size_input.symbol_index.as_ref(),
            call_graph.as_ref(),
            &contracts,
            &mut unknown_symbol_report,
            &mut aggregated_stack_sizes,
        )?
    };
    let async_poll_stack_manifest_output = (!async_poll_stack_entries.is_empty())
        .then(|| render_async_poll_stack_manifest(&async_poll_stack_entries));
    let async_poll_stack_rust_output = (!async_poll_stack_entries.is_empty()).then(|| {
        render_async_poll_stack_rust_contracts(
            &async_poll_stack_entries,
            config.crate_name.as_deref(),
        )
    });

    Ok(GeneratedOutputs {
        manifest_output,
        generated_entries,
        async_poll_stack_manifest_output,
        async_poll_stack_rust_output,
        unknown_symbol_report,
    })
}

fn build_async_poll_stack_entries(
    roots: &[RootEntry],
    stack_sizes: &BTreeMap<String, usize>,
    symbol_index: Option<&ArtifactSymbolIndex>,
    call_graph: Option<&BTreeMap<String, Vec<String>>>,
    contracts: &[UnknownSymbolContract],
    unknown_symbol_report: &mut UnknownSymbolReport,
    aggregated_stack_sizes: &mut BTreeMap<String, usize>,
) -> Result<Vec<GeneratedAsyncPollStackRustContractEntry>, String> {
    let mut merged_entries = BTreeMap::<String, usize>::new();
    for root in roots {
        let stack_bytes = resolve_root_stack_bytes(
            root,
            stack_sizes,
            symbol_index,
            call_graph,
            contracts,
            unknown_symbol_report,
            aggregated_stack_sizes,
        )?
        .max(1);
        merged_entries
            .entry(root.type_name.clone())
            .and_modify(|existing| {
                if stack_bytes > *existing {
                    *existing = stack_bytes;
                }
            })
            .or_insert(stack_bytes);
    }

    Ok(merged_entries
        .into_iter()
        .map(
            |(type_name, poll_stack_bytes)| GeneratedAsyncPollStackRustContractEntry {
                type_name,
                poll_stack_bytes,
            },
        )
        .collect())
}

fn render_async_poll_stack_manifest(
    entries: &[GeneratedAsyncPollStackRustContractEntry],
) -> String {
    let mut rendered = String::from(
        "# Generated by fusion_std_fiber_task_analyzer\n\
         # type_name = poll_stack_bytes\n",
    );
    for entry in entries {
        rendered.push_str(&entry.type_name);
        rendered.push_str(" = ");
        rendered.push_str(&entry.poll_stack_bytes.to_string());
        rendered.push('\n');
    }
    rendered
}

fn render_async_poll_stack_rust_contracts(
    entries: &[GeneratedAsyncPollStackRustContractEntry],
    crate_name: Option<&str>,
) -> String {
    let mut rendered = String::from(
        "// Generated by fusion_std_fiber_task_analyzer\n\
         // Include this file from a consumer crate to declare generated async poll-stack contracts.\n",
    );
    for entry in entries {
        if !generated_contract_type_is_nameable(&entry.type_name) {
            continue;
        }
        rendered.push_str("fusion_std::declare_generated_async_poll_stack_contract!(\n");
        rendered.push_str("    ");
        rendered.push_str(&render_contract_type_path(&entry.type_name, crate_name));
        rendered.push_str(",\n");
        rendered.push_str("    ");
        rendered.push_str(&entry.poll_stack_bytes.to_string());
        rendered.push_str(",\n");
        rendered.push_str(");\n\n");
    }
    rendered
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

fn resolve_root_execution(
    root: &RootEntry,
    stack_sizes: &BTreeMap<String, usize>,
    symbol_index: Option<&ArtifactSymbolIndex>,
    call_graph: Option<&BTreeMap<String, Vec<String>>>,
) -> Result<GeneratedTaskExecution, String> {
    let Some(graph) = call_graph else {
        return Ok(GeneratedTaskExecution::Fiber);
    };
    let resolved_symbol = resolve_requested_symbol(&root.symbol, stack_sizes, symbol_index)
        .map_err(|error| {
            format!(
                "failed to resolve execution symbol for `{}` (`{}`): {error}",
                root.type_name, root.symbol
            )
        })?;
    let mut visited = BTreeSet::new();
    let mut visiting = Vec::new();
    resolve_symbol_execution(
        &resolved_symbol,
        stack_sizes,
        graph,
        symbol_index,
        &mut visited,
        &mut visiting,
    )
    .map_err(|error| {
        format!(
            "failed to resolve execution kind for `{}` (`{}`): {error}",
            root.type_name, root.symbol
        )
    })
}

fn resolve_symbol_execution(
    symbol: &str,
    stack_sizes: &BTreeMap<String, usize>,
    call_graph: &BTreeMap<String, Vec<String>>,
    symbol_index: Option<&ArtifactSymbolIndex>,
    visited: &mut BTreeSet<String>,
    visiting: &mut Vec<String>,
) -> Result<GeneratedTaskExecution, String> {
    if visited.contains(symbol) {
        return Ok(GeneratedTaskExecution::InlineNoYield);
    }
    if !stack_sizes.contains_key(symbol) || symbol_requires_fiber(symbol, symbol_index) {
        return Ok(GeneratedTaskExecution::Fiber);
    }
    if visiting.iter().any(|entry| entry == symbol) {
        return Ok(GeneratedTaskExecution::Fiber);
    }

    visiting.push(symbol.to_owned());
    if let Some(callees) = call_graph.get(symbol) {
        for callee in callees {
            if resolve_symbol_execution(
                callee,
                stack_sizes,
                call_graph,
                symbol_index,
                visited,
                visiting,
            )? == GeneratedTaskExecution::Fiber
            {
                visiting.pop();
                visited.insert(symbol.to_owned());
                return Ok(GeneratedTaskExecution::Fiber);
            }
        }
    }
    visiting.pop();
    visited.insert(symbol.to_owned());
    Ok(GeneratedTaskExecution::InlineNoYield)
}

fn symbol_requires_fiber(symbol: &str, symbol_index: Option<&ArtifactSymbolIndex>) -> bool {
    let Some(metadata) = symbol_index.and_then(|index| index.metadata_for_raw(symbol)) else {
        return symbol.contains("yield_now")
            || symbol.contains("wait_for_readiness")
            || symbol.contains("GreenHandle")
            || symbol.contains("CurrentFiberHandle");
    };
    let candidates = [
        metadata.raw.as_str(),
        metadata.demangled.as_str(),
        metadata.normalized_demangled.as_str(),
    ];
    candidates.into_iter().any(|candidate| {
        candidate.contains("fusion_std::thread::fiber::yield_now")
            || candidate.contains("fusion_std::thread::fiber::wait_for_readiness")
            || candidate.contains("fusion_sys::fiber::yield_now")
            || (candidate.contains("fusion_std::thread::fiber::GreenHandle")
                && candidate.contains("::join"))
            || (candidate.contains("fusion_std::thread::fiber::CurrentFiberHandle")
                && candidate.contains("::join"))
    })
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

fn merge_stack_size_inputs(primary: &mut StackSizeInput, auxiliary: StackSizeInput) {
    for (symbol, bytes) in auxiliary.stack_sizes {
        primary
            .stack_sizes
            .entry(symbol)
            .and_modify(|existing| {
                if bytes > *existing {
                    *existing = bytes;
                }
            })
            .or_insert(bytes);
    }
    primary.artifact_source |= auxiliary.artifact_source;
    if let Some(auxiliary_index) = auxiliary.symbol_index {
        primary
            .symbol_index
            .get_or_insert_with(|| ArtifactSymbolIndex {
                entries: Vec::new(),
            })
            .merge(auxiliary_index);
    }
}

fn load_merged_artifact_call_graph(
    artifact_paths: &[PathBuf],
) -> Result<Option<BTreeMap<String, Vec<String>>>, String> {
    if artifact_paths.is_empty() {
        return Ok(None);
    }
    let mut merged = BTreeMap::<String, Vec<String>>::new();
    for path in artifact_paths {
        merge_call_graph(&mut merged, load_artifact_call_graph(path)?);
    }
    Ok(Some(merged))
}

fn merge_call_graph(
    primary: &mut BTreeMap<String, Vec<String>>,
    auxiliary: BTreeMap<String, Vec<String>>,
) {
    for (caller, callees) in auxiliary {
        let entry = primary.entry(caller).or_default();
        for callee in callees {
            if !entry.iter().any(|existing| existing == &callee) {
                entry.push(callee);
            }
        }
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
    let raw_output = run_tool("llvm-nm", ["--format=just-symbols"], path)?;
    let demangled_output = run_tool("llvm-nm", ["-C", "--format=just-symbols"], path)?;
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
    fn merge(&mut self, other: ArtifactSymbolIndex) {
        for entry in other.entries {
            if self
                .entries
                .iter()
                .any(|existing| existing.raw == entry.raw)
            {
                continue;
            }
            self.entries.push(entry);
        }
    }

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

        let frame_stack = self
            .resolve_symbol_frame_stack(symbol, caller)
            .ok_or_else(|| format!("missing stack-size entry for symbol `{symbol}`"))?;
        if let Some(cycle_start) = visiting.iter().position(|entry| entry == symbol) {
            if let Some((contract_symbol, contract_stack_bytes)) =
                resolve_unknown_symbol_contract(symbol, self.symbol_index, self.contracts)
            {
                let stack_bytes = frame_stack.max(contract_stack_bytes);
                self.unknown_symbol_report.observe(
                    symbol,
                    self.symbol_index,
                    Some(contract_symbol.as_str()),
                    Some(stack_bytes),
                    caller,
                    Some(self.root_type_name),
                );
                self.aggregated_stack_sizes
                    .insert(symbol.to_owned(), stack_bytes);
                return Ok(stack_bytes);
            }
            let mut cycle = visiting[cycle_start..].to_vec();
            cycle.push(symbol.to_owned());
            return Err(format!("call-graph cycle detected: {}", cycle.join(" -> ")));
        }
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
        if !generated_contract_type_is_nameable(&entry.type_name) {
            continue;
        }
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
        rendered.push_str("    ");
        rendered.push_str(entry.execution.rust_path());
        rendered.push_str(",\n");
        rendered.push_str(");\n\n");
    }
    rendered
}

fn generated_contract_type_is_nameable(type_name: &str) -> bool {
    !type_name.contains("{{closure}}")
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
            "fusion_sys::thread::vector::VectorDispatchLane::DeferredPrimary"
        }
        RedInlineFallbackLane::DeferredSecondary => {
            "fusion_sys::thread::vector::VectorDispatchLane::DeferredSecondary"
        }
    });
    rendered.push_str(", fusion_sys::thread::vector::VectorDispatchCookie(");
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
        "{reason}\nusage: cargo run -p fusion-std --bin fiber_task_analyzer -- <roots> <stack-sizes|artifact> <output> [call-graph|artifact] [--aux-artifact <path>]... [--contracts <path>] [--report <path>] [--rust-contracts <path>] [--red-inline-contracts <path>] [--red-inline-rust <path>] [--async-poll-stack-roots <path> [--async-poll-stack-output <path>] [--async-poll-stack-rust <path>]] [--crate-name <name>]"
    )
}

#[cfg(test)]
mod tests;
