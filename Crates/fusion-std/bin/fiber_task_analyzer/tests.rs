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
fn uses_contracts_to_break_recursive_cycles() {
    let stack_sizes = parse_stack_sizes(
        "_RNvCsdX_panic = 64\n\
             mid = 32\n",
    )
    .expect("stack sizes should parse");
    let call_graph = parse_call_graph(
        "_RNvCsdX_panic = mid\n\
             mid = _RNvCsdX_panic\n",
    )
    .expect("call graph should parse");
    let index = parse_llvm_nm_symbol_index(
        "_RNvCsdX_panic\nmid\n",
        "core::panicking::panic_fmt::h1234567890abcdef\ntest::mid::h1234567890abcdef\n",
    )
    .expect("llvm-nm output should parse");
    let contracts = load_contracts_from_str("core::panicking::panic_* = 512\n")
        .expect("contracts should parse");
    let mut report = UnknownSymbolReport::default();
    let mut aggregated = BTreeMap::new();

    let resolved = resolve_stack_for_test(
        "_RNvCsdX_panic",
        "test::RootTask",
        &stack_sizes,
        &call_graph,
        Some(&index),
        &contracts,
        &mut report,
        &mut aggregated,
    )
    .expect("contracted cycle should resolve");

    assert_eq!(resolved, 64 + (32 + 512));
    assert_eq!(aggregated.get("_RNvCsdX_panic"), Some(&resolved));
    let observation = report
        .observations
        .get("_RNvCsdX_panic")
        .expect("cycle break should record contracted symbol");
    assert_eq!(
        observation.matched_contract_symbol.as_deref(),
        Some("core::panicking::panic_*")
    );
    assert_eq!(observation.contract_stack_bytes, Some(512));
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
fn resolves_inline_no_yield_execution_for_plain_known_call_graph() {
    let root = RootEntry {
        type_name: "test::InlineTask".to_owned(),
        symbol: "root".to_owned(),
        priority: 0,
    };
    let stack_sizes = parse_stack_sizes(
        "root = 64\n\
             leaf = 32\n",
    )
    .expect("stack sizes should parse");
    let call_graph = parse_call_graph("root = leaf\n").expect("call graph should parse");

    let execution = resolve_root_execution(&root, &stack_sizes, None, Some(&call_graph))
        .expect("execution should resolve");

    assert_eq!(execution, GeneratedTaskExecution::InlineNoYield);
}

#[test]
fn resolves_fiber_execution_when_reachable_graph_contains_yield() {
    let root = RootEntry {
        type_name: "test::YieldTask".to_owned(),
        symbol: "root".to_owned(),
        priority: 0,
    };
    let stack_sizes = parse_stack_sizes(
        "root = 64\n\
             _RNv_yield = 16\n",
    )
    .expect("stack sizes should parse");
    let call_graph = parse_call_graph("root = _RNv_yield\n").expect("call graph should parse");
    let index = parse_llvm_nm_symbol_index(
        "_RNv_yield\n",
        "fusion_std::thread::fiber::yield_now::h1234567890abcdef\n",
    )
    .expect("llvm-nm output should parse");

    let execution = resolve_root_execution(&root, &stack_sizes, Some(&index), Some(&call_graph))
        .expect("execution should resolve");

    assert_eq!(execution, GeneratedTaskExecution::Fiber);
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
    let rendered = fs::read_to_string(&rendered_path).expect("rendered report should be readable");

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
    assert!(rendered.contains("pub const GPIO_FAST: fusion_std::thread::RedInlineCompatibility"));
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
            type_name: "fusion_std::thread::fiber::GeneratedFiberTaskMetadataAnchorTask".to_owned(),
            stack_bytes: 8192,
            priority: 5,
            execution: GeneratedTaskExecution::Fiber,
        }],
        Some("fusion_std"),
    );

    assert!(rendered.contains("fusion_std::declare_generated_fiber_task_contract!("));
    assert!(rendered.contains("crate::thread::fiber::GeneratedFiberTaskMetadataAnchorTask"));
    assert!(rendered.contains("core::num::NonZeroUsize::new(8192).unwrap()"));
    assert!(rendered.contains("fusion_std::thread::FiberTaskPriority::new(5)"));
    assert!(rendered.contains("fusion_std::thread::FiberTaskExecution::Fiber"));
}

#[test]
fn renders_rust_contracts_with_absolute_type_paths_without_crate_name() {
    let rendered = render_rust_contracts(
        &[GeneratedRustContractEntry {
            type_name: "external_crate::task::ExternalTask".to_owned(),
            stack_bytes: 4096,
            priority: 0,
            execution: GeneratedTaskExecution::Fiber,
        }],
        None,
    );

    assert!(rendered.contains("external_crate::task::ExternalTask"));
}

#[test]
fn render_rust_contracts_skips_unnameable_closure_types() {
    let rendered = render_rust_contracts(
        &[GeneratedRustContractEntry {
            type_name: "fusion_example_pico::main::{{closure}}".to_owned(),
            stack_bytes: 1024,
            priority: 0,
            execution: GeneratedTaskExecution::Fiber,
        }],
        Some("fusion_example_pico"),
    );

    assert!(!rendered.contains("declare_generated_fiber_task_contract!"));
}

#[test]
fn renders_async_poll_stack_rust_contracts_with_crate_relative_type_paths() {
    let rendered = render_async_poll_stack_rust_contracts(
        &[GeneratedAsyncPollStackRustContractEntry {
            type_name: "fusion_std::thread::executor::GeneratedAsyncPollStackMetadataAnchorFuture"
                .to_owned(),
            poll_stack_bytes: 1536,
        }],
        Some("fusion_std"),
    );

    assert!(rendered.contains("fusion_std::declare_generated_async_poll_stack_contract!("));
    assert!(
        rendered.contains("crate::thread::executor::GeneratedAsyncPollStackMetadataAnchorFuture")
    );
    assert!(rendered.contains("1536"));
}

#[test]
fn render_async_poll_stack_rust_contracts_skips_unnameable_closure_types() {
    let rendered = render_async_poll_stack_rust_contracts(
        &[GeneratedAsyncPollStackRustContractEntry {
            type_name: "fusion_example_pico::main::{{closure}}".to_owned(),
            poll_stack_bytes: 1024,
        }],
        Some("fusion_example_pico"),
    );

    assert!(!rendered.contains("declare_generated_async_poll_stack_contract!"));
}

#[test]
fn renders_async_poll_stack_rust_contracts_with_absolute_external_type_paths() {
    let rendered = render_async_poll_stack_rust_contracts(
        &[GeneratedAsyncPollStackRustContractEntry {
            type_name: "external_crate::task::ExternalFuture".to_owned(),
            poll_stack_bytes: 896,
        }],
        Some("fusion_example_pico"),
    );

    assert!(rendered.contains("declare_generated_async_poll_stack_contract!"));
    assert!(rendered.contains("external_crate::task::ExternalFuture"));
    assert!(rendered.contains("896"));
}

#[test]
fn generate_outputs_merges_duplicate_type_names_to_worst_case_stack() {
    let temp_root = std::env::temp_dir().join(format!(
        "fusion-std-fiber-task-analyzer-{}",
        std::process::id()
    ));
    let roots_path = temp_root.join("roots.txt");
    let stack_sizes_path = temp_root.join("stack.txt");
    let output_path = temp_root.join("out.generated");
    fs::create_dir_all(&temp_root).expect("temp dir should be creatable");
    fs::write(
        &roots_path,
        "test::Task::{{closure}} = root_a\n\
             test::Task::{{closure}} = root_b\n",
    )
    .expect("roots should write");
    fs::write(
        &stack_sizes_path,
        "root_a = 128\n\
             root_b = 256\n",
    )
    .expect("stack sizes should write");

    let outputs = generate_outputs(&AnalyzerConfig {
        roots_path,
        stack_sizes_path,
        output_path,
        call_graph_path: None,
        aux_artifact_paths: Vec::new(),
        contracts_path: None,
        report_path: None,
        rust_contracts_path: None,
        red_inline_contracts_path: None,
        red_inline_rust_path: None,
        async_poll_stack_roots_path: None,
        async_poll_stack_output_path: None,
        async_poll_stack_rust_path: None,
        crate_name: None,
    })
    .expect("outputs should generate");

    assert!(
        outputs
            .manifest_output
            .contains("test::Task::{{closure}} = 256")
    );
    assert_eq!(outputs.generated_entries.len(), 1);
    assert_eq!(
        outputs.generated_entries[0].type_name,
        "test::Task::{{closure}}"
    );
    assert_eq!(outputs.generated_entries[0].stack_bytes, 256);
    assert!(outputs.async_poll_stack_manifest_output.is_none());
    assert!(outputs.async_poll_stack_rust_output.is_none());
}

#[test]
fn generate_outputs_renders_async_poll_stack_manifest() {
    let temp_root = std::env::temp_dir().join(format!(
        "fusion-std-async-poll-stack-analyzer-{}",
        std::process::id()
    ));
    let roots_path = temp_root.join("roots.txt");
    let async_roots_path = temp_root.join("async-roots.txt");
    let stack_sizes_path = temp_root.join("stack.txt");
    let output_path = temp_root.join("out.generated");
    fs::create_dir_all(&temp_root).expect("temp dir should be creatable");
    fs::write(&roots_path, "test::FiberTask = fiber_root\n").expect("roots should write");
    fs::write(
        &async_roots_path,
        "test::AsyncFuture = async_root_a\n\
             test::AsyncFuture = async_root_b\n",
    )
    .expect("async roots should write");
    fs::write(
        &stack_sizes_path,
        "fiber_root = 128\n\
             async_root_a = 640\n\
             async_root_b = 768\n",
    )
    .expect("stack sizes should write");

    let outputs = generate_outputs(&AnalyzerConfig {
        roots_path,
        stack_sizes_path,
        output_path,
        call_graph_path: None,
        aux_artifact_paths: Vec::new(),
        contracts_path: None,
        report_path: None,
        rust_contracts_path: None,
        red_inline_contracts_path: None,
        red_inline_rust_path: None,
        async_poll_stack_roots_path: Some(async_roots_path),
        async_poll_stack_output_path: Some(temp_root.join("async.generated")),
        async_poll_stack_rust_path: Some(temp_root.join("async.contracts.rs")),
        crate_name: None,
    })
    .expect("outputs should generate");

    let async_manifest = outputs
        .async_poll_stack_manifest_output
        .expect("async poll stack manifest should render");
    assert!(async_manifest.contains("test::AsyncFuture = 768"));
    let async_rust = outputs
        .async_poll_stack_rust_output
        .expect("async poll stack Rust contracts should render");
    assert!(async_rust.contains("declare_generated_async_poll_stack_contract!"));
    assert!(async_rust.contains("test::AsyncFuture"));
    assert!(async_rust.contains("768"));
}
