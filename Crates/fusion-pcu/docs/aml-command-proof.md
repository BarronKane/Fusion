# AML Command conformance target

## Captured Dell `_LID` behavior

The first firmware proof target is Dell Latitude E6430 `\_SB.LID0._LID`, currently classified as a PCU `Command` target by the ACPI backend. The captured-DSDT evaluator fixture lives in `Crates/fusion-firmware/aml/verify.rs`. It runs the real loaded AML namespace with EC register 0 set to `0x10` and runtime integer `\ECRD` set to 1; the method returns `Integer(1)` and does not block. The test now also changes EC register 0 to zero and gets `Integer(0)`, then injects an EC host failure and observes `AmlErrorKind::HostFailure`. This proves that the evaluator result depends on the host-backed AML execution path and that host errors propagate. It is still reference-interpreter evidence, not PCU execution evidence.

The captured call chain is not a direct field read:

1. `_LID` calls root method `\ECG3()` and returns that result.
2. `ECG3()` returns `\ECBT(0, 0x10)`.
3. `ECBT` calls `\_SB.PCI0.LPCB.ECDV.ECR1(Arg0)`, masks the returned value with `Arg1`, and returns AML One or Zero.
4. `ECR1` is itself a one-argument AML method with a 637-byte body. It is not an `AmlFieldDescriptor` that can be represented as one `ReadResult`.

The current fixture initializes `\ECRD` as well as EC register 0 because that is how the captured AML helper context was originally exercised. The test does not claim that `\ECRD` alone determines the answer; the two EC-value outcomes and host failure are now explicit. The loaded namespace test asserts that the `ECBT` and `ECR1` method records and their argument counts match the expected helper chain.

## What current PCU Command can and cannot say

The core now has `PcuCommandResultId`, typed `PcuCommandResult`, `PcuOperand::Result`, and `PcuCommandOp::ReadResult`. The shared command verifier checks scalar width, result definition/use order, and declared typed binding or input-port compatibility. It rejects `PcuTarget::Named` and `PcuTarget::Intrinsic` reads as opaque. Those additions solve value flow for simple typed reads; they do not make an AML field or method executable.

There is no backend-neutral descriptor that binds an AML `OpRegion` and field bit range to a PCU host target. `PcuBinding` and `PcuPort` describe ordinary typed values, not an address-space identity plus region-relative offset, access width, or field extraction rules. Also, `PcuCommandOp::Invoke` has no result destination, so it cannot express the `ECG3 -> ECBT -> ECR1` return chain. The command vocabulary has no branch operation for `ECBT`'s `If`/two-return control flow, and the host operation error contract does not preserve AML host-failure kinds as a command result. A string target such as `Named("\\_SB...ECR1")` would only conceal these missing semantics.

The executable boundary check is `dell_lid_command_cannot_be_lowered_as_an_opaque_named_read` in `aml/lowering.rs`: it confirms Dell `_LID` is classified as `Command`, constructs the tempting `ReadResult(Named("\\_SB.LID0._LID"))` approximation, and asserts that the shared PCU verifier rejects it as `OpaqueReadTarget`. Thus there is no PCU read/effect trace to compare with the AML VM yet; this is an explicit rejection proof, not a conformance proof or Dell `_LID` support claim.

## Smallest honest next contract slice

Before lowering this fixture, add a backend-neutral host-region contract with:

- a typed region identity and address-space kind, distinct from `PcuTarget::Named`;
- a field view that carries region-relative bit offset, bit width, access granularity, and read/write permissions;
- ordered host read/write operations with explicit typed results and a defined error channel;
- result-producing method invocation (or an explicitly bounded inliner) and structured conditional control flow with return values.

Then implement an ACPI adapter that resolves AML names to those region/field identities and proves the restricted AML subset it lowers. For this fixture the adapter must either verify and inline the complete helper chain, including `ECR1`'s dependencies and control flow, or report “unsupported by this lowering profile” and retain the AML VM path. Lowering must not silently replace the helper with a fixed EC offset. The conformance test should compare return value, ordered EC access trace, blocked state, notifications, and error kind against the evaluator across at least two EC inputs and an injected host failure.

No Command IR is currently emitted for `_LID`; host execution remains in `AmlPureEvaluator`. `lowering_target_pcu_ir_kind` is classification metadata only. The separate `\_SB.PCI0.LPCB.ECDV._Q66` Signal target remains a follow-on: VM query dispatch is evidence for the AML signal path, not a PCU Signal implementation.
