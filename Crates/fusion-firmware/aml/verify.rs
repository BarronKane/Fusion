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
        dispatch_backend_notification_query,
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
        AmlRuntimeAggregateValue,
        AmlRuntimeBufferSlot,
        AmlRuntimePackageSlot,
        AmlRuntimeIntegerSlot,
        AmlRuntimeMutexSlot,
        AmlRuntimeState,
        AmlSleepHost,
        AmlSystemIoHost,
        AmlSystemMemoryHost,
        AmlVm,
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
        ec_stream_command: Cell<u8>,
        ec_stream_index: Cell<u8>,
        ec_streams: RefCell<Vec<(u8, Vec<u8>)>>,
        memory: RefCell<Vec<(u64, u8)>>,
        system_io_writes: RefCell<Vec<(u64, AmlAccessWidth, u64)>>,
        notifications: RefCell<Vec<AmlNotifyEvent>>,
    }

    impl Default for DellRegionHost {
        fn default() -> Self {
            Self {
                ec: RefCell::new([0; 256]),
                ec_stream_command: Cell::new(0),
                ec_stream_index: Cell::new(0),
                ec_streams: RefCell::new(Vec::new()),
                memory: RefCell::new(Vec::new()),
                system_io_writes: RefCell::new(Vec::new()),
                notifications: RefCell::new(Vec::new()),
            }
        }
    }

    impl DellRegionHost {
        fn read_memory_byte(&self, address: u64) -> u8 {
            self.memory
                .borrow()
                .iter()
                .find_map(|(entry_address, value)| (*entry_address == address).then_some(*value))
                .unwrap_or(0)
        }

        fn write_memory_byte(&self, address: u64, value: u8) {
            let mut memory = self.memory.borrow_mut();
            if let Some((_, entry_value)) = memory
                .iter_mut()
                .find(|(entry_address, _)| *entry_address == address)
            {
                *entry_value = value;
            } else {
                memory.push((address, value));
            }
        }

        fn set_ec_stream(&self, command: u8, bytes: &[u8]) {
            let mut streams = self.ec_streams.borrow_mut();
            if let Some((_, existing)) = streams
                .iter_mut()
                .find(|(stream_command, _)| *stream_command == command)
            {
                existing.clear();
                existing.extend_from_slice(bytes);
                return;
            }
            streams.push((command, bytes.to_vec()));
        }

        fn read_ec_stream_byte(&self) -> Option<u8> {
            let command = self.ec_stream_command.get();
            let index = usize::from(self.ec_stream_index.get());
            let streams = self.ec_streams.borrow();
            let (_, bytes) = streams
                .iter()
                .find(|(stream_command, _)| *stream_command == command)?;
            let value = bytes.get(index).copied().unwrap_or(0);
            self.ec_stream_index
                .set(self.ec_stream_index.get().saturating_add(1));
            Some(value)
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
        fn read_system_memory(&self, address: u64, width: AmlAccessWidth) -> AmlResult<u64> {
            let byte_count = match width {
                AmlAccessWidth::Bits8 => 1_u64,
                AmlAccessWidth::Bits16 => 2_u64,
                AmlAccessWidth::Bits32 => 4_u64,
                AmlAccessWidth::Bits64 => 8_u64,
            };
            let mut value = 0_u64;
            let mut index = 0_u64;
            while index < byte_count {
                value |= u64::from(self.read_memory_byte(address + index)) << (index * 8);
                index += 1;
            }
            Ok(value)
        }

        fn write_system_memory(
            &self,
            address: u64,
            width: AmlAccessWidth,
            value: u64,
        ) -> AmlResult<()> {
            let byte_count = match width {
                AmlAccessWidth::Bits8 => 1_u64,
                AmlAccessWidth::Bits16 => 2_u64,
                AmlAccessWidth::Bits32 => 4_u64,
                AmlAccessWidth::Bits64 => 8_u64,
            };
            let mut index = 0_u64;
            while index < byte_count {
                self.write_memory_byte(address + index, ((value >> (index * 8)) & 0xff) as u8);
                index += 1;
            }
            Ok(())
        }
    }

    impl AmlSystemIoHost for DellRegionHost {
        fn read_system_io(&self, port: u64, _width: AmlAccessWidth) -> AmlResult<u64> {
            if port == 0xB2 {
                return Ok(0);
            }
            Err(AmlError::unsupported())
        }

        fn write_system_io(&self, port: u64, width: AmlAccessWidth, value: u64) -> AmlResult<()> {
            if port == 0xB2 {
                self.system_io_writes
                    .borrow_mut()
                    .push((port, width, value));
                return Ok(());
            }
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
            if register == 0x2A {
                if let Some(value) = self.read_ec_stream_byte() {
                    return Ok(value);
                }
            }
            Ok(self.ec.borrow()[usize::from(register)])
        }

        fn write_embedded_controller(&self, register: u8, value: u8) -> AmlResult<()> {
            if register == 0x04 {
                self.ec_stream_command.set(value);
                self.ec_stream_index.set(0);
            }
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

    fn buffer_bytes(
        state: &AmlRuntimeState<'_>,
        handle: crate::aml::AmlRuntimeBufferHandle,
    ) -> Vec<u8> {
        let len = state.read_buffer_len(handle).unwrap_or(0);
        let mut bytes = Vec::with_capacity(usize::from(len));
        let mut index = 0_u8;
        while index < len {
            bytes.push(state.read_buffer_byte(handle, index).unwrap());
            index += 1;
        }
        bytes
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
        let package_slots: [Cell<Option<AmlRuntimePackageSlot>>; 8] =
            core::array::from_fn(|_| Cell::new(None));
        let mutex_slots: [Cell<Option<AmlRuntimeMutexSlot>>; 8] =
            core::array::from_fn(|_| Cell::new(None));
        let state = AmlRuntimeState::new(&integer_slots)
            .with_packages(&package_slots)
            .with_mutexes(&mutex_slots);
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

    #[test]
    fn dell_captured_namespace_executes_psr_from_real_dsdt() {
        if skipped_if_missing_dump() {
            return;
        }

        const GNVS_BASE: u64 = 0xDA7FDE18;
        const PWRS_OFFSET: u64 = 16;

        let namespace = load_dell_namespace().expect("captured Dell namespace should load");
        let evaluator = AmlPureEvaluator::new(namespace);
        let psr_path = AmlResolvedNamePath::parse_text("\\_SB.AC._PSR").unwrap();
        let ecrd_path = AmlResolvedNamePath::parse_text("\\ECRD").unwrap();
        let psr_method = namespace.record_by_path(psr_path).unwrap().descriptor.id;
        let ecrd_node = namespace.record_by_path(ecrd_path).unwrap().descriptor.id;

        let integer_slots: [Cell<Option<AmlRuntimeIntegerSlot>>; 16] =
            core::array::from_fn(|_| Cell::new(None));
        let package_slots: [Cell<Option<AmlRuntimePackageSlot>>; 8] =
            core::array::from_fn(|_| Cell::new(None));
        let mutex_slots: [Cell<Option<AmlRuntimeMutexSlot>>; 8] =
            core::array::from_fn(|_| Cell::new(None));
        let state = AmlRuntimeState::new(&integer_slots)
            .with_packages(&package_slots)
            .with_mutexes(&mutex_slots);
        let host = DellRegionHost::default();
        state.write_integer(ecrd_node, 1).unwrap();
        host.ec.borrow_mut()[6] = 1;
        host.write_system_memory(GNVS_BASE + PWRS_OFFSET, AmlAccessWidth::Bits8, 1)
            .unwrap();

        let psr_outcome = evaluator
            .evaluate_with_host_and_state(
                &host,
                &state,
                AmlMethodInvocation {
                    method: psr_method,
                    phase: AmlExecutionPhase::Runtime,
                    args: &[],
                },
            )
            .unwrap();
        assert_eq!(psr_outcome.return_value, Some(AmlValue::Integer(1)));
        assert!(!psr_outcome.blocked);
        assert!(host.notifications.borrow().is_empty());
    }

    #[test]
    fn dell_captured_namespace_executes_bst_from_real_dsdt() {
        if skipped_if_missing_dump() {
            return;
        }

        let namespace = load_dell_namespace().expect("captured Dell namespace should load");
        let evaluator = AmlPureEvaluator::new(namespace);
        let bst_path = AmlResolvedNamePath::parse_text("\\_SB.BAT0._BST").unwrap();
        let ecrd_path = AmlResolvedNamePath::parse_text("\\ECRD").unwrap();
        let bst_method = namespace.record_by_path(bst_path).unwrap().descriptor.id;
        let ecrd_node = namespace.record_by_path(ecrd_path).unwrap().descriptor.id;

        let integer_slots: [Cell<Option<AmlRuntimeIntegerSlot>>; 16] =
            core::array::from_fn(|_| Cell::new(None));
        let package_slots: [Cell<Option<AmlRuntimePackageSlot>>; 8] =
            core::array::from_fn(|_| Cell::new(None));
        let mutex_slots: [Cell<Option<AmlRuntimeMutexSlot>>; 8] =
            core::array::from_fn(|_| Cell::new(None));
        let state = AmlRuntimeState::new(&integer_slots)
            .with_packages(&package_slots)
            .with_mutexes(&mutex_slots);
        let host = DellRegionHost::default();
        state.write_integer(ecrd_node, 1).unwrap();
        host.ec.borrow_mut()[0] = 1;
        host.ec.borrow_mut()[0x10] = 0x55;
        host.ec.borrow_mut()[0x12] = 0x34;
        host.ec.borrow_mut()[0x13] = 0x12;
        host.ec.borrow_mut()[0x14] = 0xbc;
        host.ec.borrow_mut()[0x15] = 0x9a;
        host.ec.borrow_mut()[0x16] = 0x78;
        host.ec.borrow_mut()[0x17] = 0x56;

        let bst_outcome = evaluator
            .evaluate_with_host_and_state(
                &host,
                &state,
                AmlMethodInvocation {
                    method: bst_method,
                    phase: AmlExecutionPhase::Runtime,
                    args: &[],
                },
            )
            .unwrap();
        let handle = match bst_outcome.return_value {
            Some(AmlValue::PackageHandle(handle)) => handle,
            other => panic!("expected package handle, got {other:?}"),
        };
        assert_eq!(state.read_package_len(handle), Some(4));
        assert_eq!(state.read_package_integer(handle, 0), Some(0x55));
        assert_eq!(state.read_package_integer(handle, 1), Some(0x1234));
        assert_eq!(state.read_package_integer(handle, 2), Some(0x5678));
        assert_eq!(state.read_package_integer(handle, 3), Some(0x9abc));
        assert!(!bst_outcome.blocked);
    }

    #[test]
    fn dell_captured_namespace_executes_bif_from_real_dsdt() {
        if skipped_if_missing_dump() {
            return;
        }

        let namespace = load_dell_namespace().expect("captured Dell namespace should load");
        let evaluator = AmlPureEvaluator::new(namespace);
        let bif_path = AmlResolvedNamePath::parse_text("\\_SB.BAT0._BIF").unwrap();
        let ecrd_path = AmlResolvedNamePath::parse_text("\\ECRD").unwrap();
        let bif_method = namespace.record_by_path(bif_path).unwrap().descriptor.id;
        let ecrd_node = namespace.record_by_path(ecrd_path).unwrap().descriptor.id;

        let integer_slots: [Cell<Option<AmlRuntimeIntegerSlot>>; 16] =
            core::array::from_fn(|_| Cell::new(None));
        let package_slots: [Cell<Option<AmlRuntimePackageSlot>>; 8] =
            core::array::from_fn(|_| Cell::new(None));
        let buffer_slots: [Cell<Option<AmlRuntimeBufferSlot>>; 16] =
            core::array::from_fn(|_| Cell::new(None));
        let mutex_slots: [Cell<Option<AmlRuntimeMutexSlot>>; 8] =
            core::array::from_fn(|_| Cell::new(None));
        let state = AmlRuntimeState::new(&integer_slots)
            .with_packages(&package_slots)
            .with_buffers(&buffer_slots)
            .with_mutexes(&mutex_slots);
        let host = DellRegionHost::default();
        state.write_integer(ecrd_node, 1).unwrap();
        host.ec.borrow_mut()[0x1E] = 0x78;
        host.ec.borrow_mut()[0x1F] = 0x56;
        host.ec.borrow_mut()[0x20] = 0x34;
        host.ec.borrow_mut()[0x21] = 0x12;
        host.ec.borrow_mut()[0x22] = 0xbc;
        host.ec.borrow_mut()[0x23] = 0x9a;
        host.ec.borrow_mut()[0x26] = 0x39;
        host.ec.borrow_mut()[0x27] = 0x30;
        host.ec.borrow_mut()[0x28] = 0x03;
        host.ec.borrow_mut()[0x29] = 0x02;
        host.set_ec_stream(1, b"Primary\0");

        let bif_outcome = evaluator
            .evaluate_with_host_and_state(
                &host,
                &state,
                AmlMethodInvocation {
                    method: bif_method,
                    phase: AmlExecutionPhase::Runtime,
                    args: &[],
                },
            )
            .unwrap();
        let handle = match bif_outcome.return_value {
            Some(AmlValue::PackageHandle(handle)) => handle,
            other => panic!("expected package handle, got {other:?}"),
        };
        assert_eq!(state.read_package_len(handle), Some(13));
        assert_eq!(state.read_package_integer(handle, 0), Some(1));
        assert_eq!(state.read_package_integer(handle, 1), Some(0x1234));
        assert_eq!(state.read_package_integer(handle, 2), Some(0x5678));
        assert_eq!(state.read_package_integer(handle, 3), Some(1));
        assert_eq!(state.read_package_integer(handle, 4), Some(0x9abc));
        assert_eq!(state.read_package_integer(handle, 5), Some(0x1234 / 0x0A));
        assert_eq!(state.read_package_integer(handle, 6), Some(0x1234 / 0x21));
        assert_eq!(state.read_package_integer(handle, 7), Some(0x1234 / 0x64));
        assert_eq!(state.read_package_integer(handle, 8), Some(0x1234 / 0x64));

        let model_handle = match state.read_package_value(handle, 9) {
            Some(AmlRuntimeAggregateValue::Buffer(handle)) => handle,
            other => panic!("expected model buffer handle, got {other:?}"),
        };
        let serial_handle = match state.read_package_value(handle, 10) {
            Some(AmlRuntimeAggregateValue::Buffer(handle)) => handle,
            other => panic!("expected serial buffer handle, got {other:?}"),
        };
        let chemistry_handle = match state.read_package_value(handle, 11) {
            Some(AmlRuntimeAggregateValue::Buffer(handle)) => handle,
            other => panic!("expected chemistry buffer handle, got {other:?}"),
        };
        let vendor_handle = match state.read_package_value(handle, 12) {
            Some(AmlRuntimeAggregateValue::Buffer(handle)) => handle,
            other => panic!("expected vendor buffer handle, got {other:?}"),
        };

        assert_eq!(buffer_bytes(&state, model_handle), b"Primary\0");
        assert_eq!(buffer_bytes(&state, serial_handle), b"12345\0");
        assert_eq!(buffer_bytes(&state, chemistry_handle), b"LION\0");
        assert_eq!(buffer_bytes(&state, vendor_handle), b"Sanyo\0");
        assert!(!bif_outcome.blocked);
    }

    #[test]
    fn dell_captured_namespace_executes_crt_from_real_dsdt() {
        if skipped_if_missing_dump() {
            return;
        }

        let namespace = load_dell_namespace().expect("captured Dell namespace should load");
        let evaluator = AmlPureEvaluator::new(namespace);
        let crt_path = AmlResolvedNamePath::parse_text("\\_TZ.THM._CRT").unwrap();
        let crt_method = namespace.record_by_path(crt_path).unwrap().descriptor.id;

        let crt_outcome = evaluator
            .evaluate(AmlMethodInvocation {
                method: crt_method,
                phase: AmlExecutionPhase::Runtime,
                args: &[],
            })
            .unwrap();
        assert_eq!(crt_outcome.return_value, Some(AmlValue::Integer(3802)));
        assert!(!crt_outcome.blocked);
    }

    #[test]
    fn dell_captured_namespace_executes_tmp_from_real_dsdt() {
        if skipped_if_missing_dump() {
            return;
        }

        const SMIB_BASE: u64 = 0xDA7D6000;

        let namespace = load_dell_namespace().expect("captured Dell namespace should load");
        let evaluator = AmlPureEvaluator::new(namespace);
        let tmp_path = AmlResolvedNamePath::parse_text("\\_TZ.THM._TMP").unwrap();
        let tmp_method = namespace.record_by_path(tmp_path).unwrap().descriptor.id;

        let integer_slots: [Cell<Option<AmlRuntimeIntegerSlot>>; 16] =
            core::array::from_fn(|_| Cell::new(None));
        let package_slots: [Cell<Option<AmlRuntimePackageSlot>>; 8] =
            core::array::from_fn(|_| Cell::new(None));
        let mutex_slots: [Cell<Option<AmlRuntimeMutexSlot>>; 8] =
            core::array::from_fn(|_| Cell::new(None));
        let state = AmlRuntimeState::new(&integer_slots)
            .with_packages(&package_slots)
            .with_mutexes(&mutex_slots);
        let host = DellRegionHost::default();

        host.write_system_memory(SMIB_BASE + 0x04, AmlAccessWidth::Bits32, 0x0B90)
            .unwrap();

        let tmp_outcome = evaluator
            .evaluate_with_host_and_state(
                &host,
                &state,
                AmlMethodInvocation {
                    method: tmp_method,
                    phase: AmlExecutionPhase::Runtime,
                    args: &[],
                },
            )
            .unwrap();
        assert_eq!(tmp_outcome.return_value, Some(AmlValue::Integer(0x0BA6)));
        assert!(!tmp_outcome.blocked);
        assert!(
            host.system_io_writes
                .borrow()
                .iter()
                .any(|(port, width, value)| *port == 0xB2
                    && *width == AmlAccessWidth::Bits8
                    && *value == 0x04)
        );
    }

    #[test]
    fn dell_captured_namespace_dispatches_q66_from_real_dsdt() {
        if skipped_if_missing_dump() {
            return;
        }

        let namespace = load_dell_namespace().expect("captured Dell namespace should load");
        let ecrd_path = AmlResolvedNamePath::parse_text("\\ECRD").unwrap();
        let ecrd_node = namespace.record_by_path(ecrd_path).unwrap().descriptor.id;

        let integer_slots: [Cell<Option<AmlRuntimeIntegerSlot>>; 16] =
            core::array::from_fn(|_| Cell::new(None));
        let package_slots: [Cell<Option<AmlRuntimePackageSlot>>; 8] =
            core::array::from_fn(|_| Cell::new(None));
        let mutex_slots: [Cell<Option<AmlRuntimeMutexSlot>>; 8] =
            core::array::from_fn(|_| Cell::new(None));
        let state = AmlRuntimeState::new(&integer_slots)
            .with_packages(&package_slots)
            .with_mutexes(&mutex_slots);
        let host = DellRegionHost::default();
        let mut vm = AmlVm::default();

        state.write_integer(ecrd_node, 1).unwrap();

        let report = dispatch_backend_notification_query::<DellLatitudeE6430AcpiHardware>(
            0, &mut vm, namespace, &host, &state, 0x66,
        )
        .unwrap();
        assert_eq!(report.invoked, 1);
        assert_eq!(report.blocked, 0);
        assert_eq!(report.missing, 0);
        assert!(host.notifications.borrow().is_empty());
    }
}
