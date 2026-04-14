//! AML backend-surface verification against one loaded namespace.

use core::mem::MaybeUninit;
use core::slice;

use fusion_hal::drivers::acpi::public::interface::backend::{
    AcpiAmlAddressSpaceKind,
    AcpiAmlBackend,
};

use crate::aml::{
    AmlAddressSpaceId,
    AmlError,
    AmlLoadedNamespace,
    AmlObjectKind,
    AmlResolvedNamePath,
    AmlResult,
};

/// Declared backend surface class being verified.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AmlBackendTargetKind {
    NamespaceRoot,
    Method,
    Field,
    OpRegion,
}

/// Verification issue class for one declared AML backend target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AmlBackendVerificationIssueKind {
    Missing,
    KindMismatch,
    OpRegionSpaceMismatch,
}

/// One missing or mismatched AML backend requirement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AmlBackendVerificationIssue {
    pub target: AmlBackendTargetKind,
    pub kind: AmlBackendVerificationIssueKind,
    pub path: &'static str,
    pub actual_kind: Option<AmlObjectKind>,
    pub expected_space: Option<AcpiAmlAddressSpaceKind>,
    pub actual_space: Option<AmlAddressSpaceId>,
}

/// Borrowed verification result for one backend namespace declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AmlBackendVerificationReport<'a> {
    pub issues: &'a [AmlBackendVerificationIssue],
}

impl<'a> AmlBackendVerificationReport<'a> {
    #[must_use]
    pub const fn is_clean(self) -> bool {
        self.issues.is_empty()
    }

    #[must_use]
    pub const fn issue_count(self) -> usize {
        self.issues.len()
    }
}

struct AmlBackendIssueWriter<'a> {
    storage: &'a mut [MaybeUninit<AmlBackendVerificationIssue>],
    len: usize,
}

impl<'a> AmlBackendIssueWriter<'a> {
    fn new(storage: &'a mut [MaybeUninit<AmlBackendVerificationIssue>]) -> Self {
        Self { storage, len: 0 }
    }

    fn push(&mut self, issue: AmlBackendVerificationIssue) -> AmlResult<()> {
        let Some(slot) = self.storage.get_mut(self.len) else {
            return Err(AmlError::overflow());
        };
        slot.write(issue);
        self.len += 1;
        Ok(())
    }

    fn finish(self) -> AmlBackendVerificationReport<'a> {
        let issues = unsafe {
            slice::from_raw_parts(
                self.storage.as_ptr().cast::<AmlBackendVerificationIssue>(),
                self.len,
            )
        };
        AmlBackendVerificationReport { issues }
    }
}

pub fn verify_acpi_backend<'records, 'blocks, 'storage, B: AcpiAmlBackend>(
    namespace: AmlLoadedNamespace<'records, 'blocks>,
    provider: u8,
    storage: &'storage mut [MaybeUninit<AmlBackendVerificationIssue>],
) -> AmlResult<AmlBackendVerificationReport<'storage>> {
    let mut writer = AmlBackendIssueWriter::new(storage);

    let root = B::aml_namespace(provider).map_err(|_| AmlError::invalid_namespace())?;
    verify_root(namespace, root.root, &mut writer)?;

    for method in B::aml_methods(provider) {
        verify_typed_path(
            namespace,
            AmlBackendTargetKind::Method,
            method.path,
            AmlObjectKind::Method,
            &mut writer,
        )?;
    }

    for field in B::aml_fields(provider) {
        verify_typed_path(
            namespace,
            AmlBackendTargetKind::Field,
            field.path,
            AmlObjectKind::Field,
            &mut writer,
        )?;
    }

    for region in B::aml_opregions(provider) {
        let path = AmlResolvedNamePath::parse_text(region.path)?;
        let Some(record) = namespace.record_by_path(path) else {
            writer.push(AmlBackendVerificationIssue {
                target: AmlBackendTargetKind::OpRegion,
                kind: AmlBackendVerificationIssueKind::Missing,
                path: region.path,
                actual_kind: None,
                expected_space: Some(region.space),
                actual_space: None,
            })?;
            continue;
        };

        if record.descriptor.kind != AmlObjectKind::OpRegion {
            writer.push(AmlBackendVerificationIssue {
                target: AmlBackendTargetKind::OpRegion,
                kind: AmlBackendVerificationIssueKind::KindMismatch,
                path: region.path,
                actual_kind: Some(record.descriptor.kind),
                expected_space: Some(region.space),
                actual_space: None,
            })?;
            continue;
        }

        let actual_space = match record.payload {
            crate::aml::AmlNamespaceNodePayload::OpRegion(region) => Some(region.space),
            _ => None,
        };

        if actual_space != Some(map_backend_space(region.space)) {
            writer.push(AmlBackendVerificationIssue {
                target: AmlBackendTargetKind::OpRegion,
                kind: AmlBackendVerificationIssueKind::OpRegionSpaceMismatch,
                path: region.path,
                actual_kind: Some(record.descriptor.kind),
                expected_space: Some(region.space),
                actual_space,
            })?;
        }
    }

    Ok(writer.finish())
}

fn verify_root(
    namespace: AmlLoadedNamespace<'_, '_>,
    path_text: &'static str,
    writer: &mut AmlBackendIssueWriter<'_>,
) -> AmlResult<()> {
    let path = AmlResolvedNamePath::parse_text(path_text)?;
    if namespace.record_by_path(path).is_none() {
        writer.push(AmlBackendVerificationIssue {
            target: AmlBackendTargetKind::NamespaceRoot,
            kind: AmlBackendVerificationIssueKind::Missing,
            path: path_text,
            actual_kind: None,
            expected_space: None,
            actual_space: None,
        })?;
    }
    Ok(())
}

fn verify_typed_path(
    namespace: AmlLoadedNamespace<'_, '_>,
    target: AmlBackendTargetKind,
    path_text: &'static str,
    expected_kind: AmlObjectKind,
    writer: &mut AmlBackendIssueWriter<'_>,
) -> AmlResult<()> {
    let path = AmlResolvedNamePath::parse_text(path_text)?;
    let Some(record) = namespace.record_by_path(path) else {
        writer.push(AmlBackendVerificationIssue {
            target,
            kind: AmlBackendVerificationIssueKind::Missing,
            path: path_text,
            actual_kind: None,
            expected_space: None,
            actual_space: None,
        })?;
        return Ok(());
    };

    if record.descriptor.kind != expected_kind {
        writer.push(AmlBackendVerificationIssue {
            target,
            kind: AmlBackendVerificationIssueKind::KindMismatch,
            path: path_text,
            actual_kind: Some(record.descriptor.kind),
            expected_space: None,
            actual_space: None,
        })?;
    }

    Ok(())
}

const fn map_backend_space(space: AcpiAmlAddressSpaceKind) -> AmlAddressSpaceId {
    match space {
        AcpiAmlAddressSpaceKind::SystemMemory => AmlAddressSpaceId::SystemMemory,
        AcpiAmlAddressSpaceKind::SystemIo => AmlAddressSpaceId::SystemIo,
        AcpiAmlAddressSpaceKind::PciConfig => AmlAddressSpaceId::PciConfig,
        AcpiAmlAddressSpaceKind::EmbeddedControl => AmlAddressSpaceId::EmbeddedControl,
        AcpiAmlAddressSpaceKind::SmBus => AmlAddressSpaceId::SmBus,
        AcpiAmlAddressSpaceKind::Gpio => AmlAddressSpaceId::Gpio,
        AcpiAmlAddressSpaceKind::GenericSerialBus => AmlAddressSpaceId::GenericSerialBus,
        AcpiAmlAddressSpaceKind::Other(value) => AmlAddressSpaceId::Oem(value),
    }
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use std::cell::{
        Cell,
        RefCell,
    };
    use std::boxed::Box;
    use std::fs;
    use std::path::Path;
    use std::vec::Vec;

    use fusion_hal::drivers::acpi::vendor::dell::DellLatitudeE6430AcpiHardware;

    use super::*;
    use crate::aml::{
        AmlAccessWidth,
        AmlDefinitionBlock,
        AmlDefinitionBlockSet,
        AmlExecutionPhase,
        AmlHost,
        AmlMethodInvocation,
        AmlNotifyEvent,
        AmlNotifySink,
        AmlOspmInterface,
        AmlPureEvaluator,
        AmlRuntimeIntegerSlot,
        AmlRuntimeMutexSlot,
        AmlRuntimeState,
        AmlSleepHost,
        AmlSystemIoHost,
        AmlSystemMemoryHost,
        AmlValue,
        AmlEmbeddedControllerHost,
        AmlPciConfigHost,
    };
    use crate::pal::hal::acpi::{
        AcpiSignature,
        AcpiTableView,
        Dsdt,
    };

    const DELL_ACPI_DUMP_DIR: &str =
        "/volumes/projects/acpi/PVAS-PL1/hw-export-PVAS-PL1-20260405-004625/acpi";

    struct DellRegionHost {
        ec: RefCell<[u8; 256]>,
        notifications: RefCell<Vec<AmlNotifyEvent>>,
    }

    impl Default for DellRegionHost {
        fn default() -> Self {
            Self {
                ec: RefCell::new([0; 256]),
                notifications: RefCell::new(Vec::new()),
            }
        }
    }

    impl AmlOspmInterface for DellRegionHost {
        fn osi_supported(&self, _interface: &str) -> bool {
            false
        }

        fn os_revision(&self) -> u64 {
            0
        }
    }

    impl AmlSleepHost for DellRegionHost {
        fn stall_us(&self, _microseconds: u32) -> AmlResult<()> {
            Ok(())
        }

        fn sleep_ms(&self, _milliseconds: u32) -> AmlResult<()> {
            Ok(())
        }
    }

    impl AmlNotifySink for DellRegionHost {
        fn notify(&self, source: crate::aml::AmlNamespaceNodeId, value: u8) -> AmlResult<()> {
            self.notifications
                .borrow_mut()
                .push(AmlNotifyEvent { source, value });
            Ok(())
        }
    }

    impl AmlSystemMemoryHost for DellRegionHost {
        fn read_system_memory(&self, _address: u64, _width: AmlAccessWidth) -> AmlResult<u64> {
            Err(AmlError::unsupported())
        }

        fn write_system_memory(
            &self,
            _address: u64,
            _width: AmlAccessWidth,
            _value: u64,
        ) -> AmlResult<()> {
            Err(AmlError::unsupported())
        }
    }

    impl AmlSystemIoHost for DellRegionHost {
        fn read_system_io(&self, _port: u64, _width: AmlAccessWidth) -> AmlResult<u64> {
            Err(AmlError::unsupported())
        }

        fn write_system_io(
            &self,
            _port: u64,
            _width: AmlAccessWidth,
            _value: u64,
        ) -> AmlResult<()> {
            Err(AmlError::unsupported())
        }
    }

    impl AmlPciConfigHost for DellRegionHost {
        fn read_pci_config(&self, _address: u64, _width: AmlAccessWidth) -> AmlResult<u64> {
            Err(AmlError::unsupported())
        }

        fn write_pci_config(
            &self,
            _address: u64,
            _width: AmlAccessWidth,
            _value: u64,
        ) -> AmlResult<()> {
            Err(AmlError::unsupported())
        }
    }

    impl AmlEmbeddedControllerHost for DellRegionHost {
        fn read_embedded_controller(&self, register: u8) -> AmlResult<u8> {
            Ok(self.ec.borrow()[usize::from(register)])
        }

        fn write_embedded_controller(&self, register: u8, value: u8) -> AmlResult<()> {
            self.ec.borrow_mut()[usize::from(register)] = value;
            Ok(())
        }
    }

    impl AmlHost for DellRegionHost {}

    fn load_definition_block(
        path: &Path,
        signature: AcpiSignature,
    ) -> AmlResult<AmlDefinitionBlock<'static>> {
        let bytes = fs::read(path).map_err(|_| AmlError::invalid_definition_block())?;
        let leaked = Box::leak(bytes.into_boxed_slice());
        let table = AcpiTableView::parse_signature(leaked, signature)
            .map_err(|_| AmlError::invalid_definition_block())?;
        AmlDefinitionBlock::from_acpi_table(table)
    }

    fn load_dell_namespace() -> AmlResult<AmlLoadedNamespace<'static, 'static>> {
        let root = Path::new(DELL_ACPI_DUMP_DIR);
        let dsdt = load_definition_block(&root.join("dsdt.dat"), AcpiSignature::DSDT)?;
        let plan = crate::aml::AmlNamespaceLoadPlan::from_definition_blocks(
            AmlDefinitionBlockSet::new(dsdt, &[]),
        );
        let mut storage = Vec::with_capacity(8192);
        storage.resize_with(8192, MaybeUninit::uninit);
        let leaked_storage = Box::leak(storage.into_boxed_slice());
        plan.load_into(leaked_storage)
    }

    fn skipped_if_missing_dump() -> bool {
        !Path::new(DELL_ACPI_DUMP_DIR).exists()
    }

    #[test]
    fn verifier_reports_missing_method_in_synthetic_namespace() {
        let body = [
            0x10, 0x33, b'\\', b'_', b'S', b'B', b'_', 0x08, b'F', b'O', b'O', b'0', 0x0a, 0x01,
            0x14, 0x08, b'_', b'S', b'T', b'A', 0x00, 0xa4, 0x01, 0x5b, 0x80, b'E', b'C', b'O',
            b'R', 0x03, 0x0a, 0x10, 0x0a, 0x20, 0x5b, 0x81, 0x10, b'E', b'C', b'O', b'R', 0x01,
            b'S', b'T', b'0', b'0', 0x08, b'S', b'T', b'0', b'1', 0x08,
        ];
        let table = {
            let mut bytes = Vec::from([0_u8; 36]);
            bytes[0..4].copy_from_slice(b"DSDT");
            bytes[4..8].copy_from_slice(&((36 + body.len()) as u32).to_le_bytes());
            bytes[8] = 2;
            bytes[10..16].copy_from_slice(b"FUSION");
            bytes[16..24].copy_from_slice(b"AMLVRFY ");
            bytes.extend_from_slice(&body);
            let checksum =
                (!bytes.iter().fold(0_u8, |sum, byte| sum.wrapping_add(*byte))).wrapping_add(1);
            bytes[9] = checksum;
            let leaked = Box::leak(bytes.into_boxed_slice());
            AmlDefinitionBlock::from_dsdt(Dsdt::parse(leaked).unwrap()).unwrap()
        };
        let plan = crate::aml::AmlNamespaceLoadPlan::from_definition_blocks(
            AmlDefinitionBlockSet::new(table, &[]),
        );
        let mut namespace_storage = Vec::with_capacity(32);
        namespace_storage.resize_with(32, MaybeUninit::uninit);
        let namespace = plan
            .load_into(Box::leak(namespace_storage.into_boxed_slice()))
            .unwrap();
        let mut issue_storage = [MaybeUninit::<AmlBackendVerificationIssue>::uninit(); 32];
        let report =
            verify_acpi_backend::<DellLatitudeE6430AcpiHardware>(namespace, 0, &mut issue_storage)
                .unwrap();
        assert!(!report.is_clean());
        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue.path == "\\_SB.AC._PSR"
                    && issue.kind == AmlBackendVerificationIssueKind::Missing)
        );
    }

    #[test]
    fn dell_backend_verifies_against_captured_namespace() {
        if skipped_if_missing_dump() {
            return;
        }

        let namespace = load_dell_namespace().expect("captured Dell namespace should load");
        let mut issue_storage = [MaybeUninit::<AmlBackendVerificationIssue>::uninit(); 64];
        let report =
            verify_acpi_backend::<DellLatitudeE6430AcpiHardware>(namespace, 0, &mut issue_storage)
                .unwrap();
        assert_eq!(report.issue_count(), 0, "{report:?}");
    }

    #[test]
    fn dell_captured_namespace_executes_lid_from_real_dsdt() {
        if skipped_if_missing_dump() {
            return;
        }

        let namespace = load_dell_namespace().expect("captured Dell namespace should load");
        let evaluator = AmlPureEvaluator::new(namespace);
        let lid_path = AmlResolvedNamePath::parse_text("\\_SB.LID0._LID").unwrap();
        let ecrd_path = AmlResolvedNamePath::parse_text("\\ECRD").unwrap();
        let lid_method = namespace.record_by_path(lid_path).unwrap().descriptor.id;
        let ecrd_node = namespace.record_by_path(ecrd_path).unwrap().descriptor.id;

        let integer_slots: [Cell<Option<AmlRuntimeIntegerSlot>>; 16] =
            core::array::from_fn(|_| Cell::new(None));
        let mutex_slots: [Cell<Option<AmlRuntimeMutexSlot>>; 8] =
            core::array::from_fn(|_| Cell::new(None));
        let state = AmlRuntimeState::new(&integer_slots).with_mutexes(&mutex_slots);
        let host = DellRegionHost::default();
        host.ec.borrow_mut()[0] = 0x10;
        state.write_integer(ecrd_node, 1).unwrap();

        let lid_outcome = evaluator
            .evaluate_with_host_and_state(
                &host,
                &state,
                AmlMethodInvocation {
                    method: lid_method,
                    phase: AmlExecutionPhase::Runtime,
                    args: &[],
                },
            )
            .unwrap();
        assert_eq!(lid_outcome.return_value, Some(AmlValue::Integer(1)));
        assert!(!lid_outcome.blocked);
    }
}
