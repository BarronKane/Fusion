//! AML VM configuration and lifecycle execution vocabulary.

use crate::aml::{
    AmlAddressSpaceId,
    AmlError,
    AmlExecutionPhase,
    AmlLoadedNamespace,
    AmlMethodInvocation,
    AmlMethodKind,
    AmlNamespaceNodePayload,
    AmlPureEvaluator,
    AmlRegionAccessHost,
    AmlResult,
    AmlRuntimeState,
    AmlValue,
};

/// VM configuration knobs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AmlVmConfig {
    pub enable_tracing: bool,
    pub allow_external_resolution: bool,
    pub eager_region_registration: bool,
}

impl Default for AmlVmConfig {
    fn default() -> Self {
        Self {
            enable_tracing: false,
            allow_external_resolution: false,
            eager_region_registration: true,
        }
    }
}

/// Coarse AML VM state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AmlVmState {
    Empty,
    Loaded,
    Ready,
    Running,
    Blocked,
}

/// Summary for one VM lifecycle execution pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AmlVmLifecycleReport {
    pub invoked: u16,
    pub skipped: u16,
    pub blocked: u16,
}

impl AmlVmLifecycleReport {
    #[must_use]
    pub const fn is_clean(self) -> bool {
        self.blocked == 0
    }
}

/// Summary for one notification/event-handler dispatch pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AmlVmHandlerDispatchReport {
    pub invoked: u16,
    pub blocked: u16,
    pub missing: u16,
}

impl AmlVmHandlerDispatchReport {
    #[must_use]
    pub const fn is_clean(self) -> bool {
        self.blocked == 0
    }
}

/// Event-handler dispatch family for `_Lxx` / `_Exx`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AmlEventHandlerKind {
    Level,
    Edge,
}

/// AML VM anchor and coarse lifecycle driver.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AmlVm {
    pub config: AmlVmConfig,
    pub state: AmlVmState,
}

enum AmlInitializerDecision {
    Run,
    Skip,
    Blocked,
}

impl Default for AmlVm {
    fn default() -> Self {
        Self::new(AmlVmConfig::default())
    }
}

impl AmlVm {
    #[must_use]
    pub const fn new(config: AmlVmConfig) -> Self {
        Self {
            config,
            state: AmlVmState::Empty,
        }
    }

    pub fn register_regions(
        &mut self,
        namespace: AmlLoadedNamespace<'_, '_>,
        host: &dyn AmlRegionAccessHost,
        runtime: &AmlRuntimeState<'_>,
    ) -> AmlResult<AmlVmLifecycleReport> {
        self.state = AmlVmState::Loaded;
        if !self.config.eager_region_registration {
            return Ok(AmlVmLifecycleReport {
                invoked: 0,
                skipped: 0,
                blocked: 0,
            });
        }

        let evaluator = AmlPureEvaluator::new(namespace);
        let mut report = AmlVmLifecycleReport {
            invoked: 0,
            skipped: 0,
            blocked: 0,
        };

        for record in namespace.records {
            let AmlNamespaceNodePayload::Method(method) = record.payload else {
                continue;
            };
            if method.kind != AmlMethodKind::RegionAvailability {
                continue;
            }

            let mut seen_spaces: [Option<AmlAddressSpaceId>; 16] = [None; 16];
            let mut seen_len = 0_usize;
            let mut sibling_count = 0_u16;

            for sibling in namespace.records {
                if sibling.descriptor.parent != record.descriptor.parent {
                    continue;
                }
                let AmlNamespaceNodePayload::OpRegion(region) = sibling.payload else {
                    continue;
                };
                if seen_spaces[..seen_len].contains(&Some(region.space)) {
                    continue;
                }
                let Some(slot) = seen_spaces.get_mut(seen_len) else {
                    return Err(AmlError::overflow());
                };
                *slot = Some(region.space);
                seen_len += 1;
                sibling_count += 1;
                let args = [
                    AmlValue::Integer(address_space_argument(region.space)),
                    AmlValue::Integer(1),
                ];

                let outcome = evaluator.evaluate_with_host_and_state(
                    host,
                    runtime,
                    AmlMethodInvocation {
                        method: method.node,
                        phase: AmlExecutionPhase::Initialization,
                        args: &args,
                    },
                )?;
                report.invoked = report.invoked.saturating_add(1);
                if outcome.blocked {
                    report.blocked = report.blocked.saturating_add(1);
                    self.state = AmlVmState::Blocked;
                    return Ok(report);
                }
            }

            if sibling_count == 0 {
                report.skipped = report.skipped.saturating_add(1);
            }
        }

        self.state = AmlVmState::Ready;
        Ok(report)
    }

    pub fn initialize_devices(
        &mut self,
        namespace: AmlLoadedNamespace<'_, '_>,
        host: &dyn AmlRegionAccessHost,
        runtime: &AmlRuntimeState<'_>,
    ) -> AmlResult<AmlVmLifecycleReport> {
        if matches!(self.state, AmlVmState::Empty) {
            self.state = AmlVmState::Loaded;
        }

        let evaluator = AmlPureEvaluator::new(namespace);
        let mut report = AmlVmLifecycleReport {
            invoked: 0,
            skipped: 0,
            blocked: 0,
        };

        for record in namespace.records {
            let AmlNamespaceNodePayload::Method(method) = record.payload else {
                continue;
            };
            if method.kind != AmlMethodKind::Initialize {
                continue;
            }

            let decision = self.should_run_initializer(
                namespace,
                &evaluator,
                host,
                runtime,
                record.descriptor.parent,
            )?;
            match decision {
                AmlInitializerDecision::Run => {}
                AmlInitializerDecision::Skip => {
                    report.skipped = report.skipped.saturating_add(1);
                    continue;
                }
                AmlInitializerDecision::Blocked => {
                    report.blocked = report.blocked.saturating_add(1);
                    self.state = AmlVmState::Blocked;
                    return Ok(report);
                }
            }

            let outcome = evaluator.evaluate_with_host_and_state(
                host,
                runtime,
                AmlMethodInvocation {
                    method: method.node,
                    phase: AmlExecutionPhase::Initialization,
                    args: &[],
                },
            )?;
            report.invoked = report.invoked.saturating_add(1);
            if outcome.blocked {
                report.blocked = report.blocked.saturating_add(1);
                self.state = AmlVmState::Blocked;
                return Ok(report);
            }
        }

        self.state = AmlVmState::Ready;
        Ok(report)
    }

    pub fn dispatch_notification_query(
        &mut self,
        namespace: AmlLoadedNamespace<'_, '_>,
        host: &dyn AmlRegionAccessHost,
        runtime: &AmlRuntimeState<'_>,
        query: u8,
    ) -> AmlResult<AmlVmHandlerDispatchReport> {
        self.dispatch_handler_kind(
            namespace,
            host,
            runtime,
            AmlMethodKind::NotificationQuery,
            [b'_', b'Q'],
            query,
        )
    }

    pub fn dispatch_event_handler(
        &mut self,
        namespace: AmlLoadedNamespace<'_, '_>,
        host: &dyn AmlRegionAccessHost,
        runtime: &AmlRuntimeState<'_>,
        kind: AmlEventHandlerKind,
        event: u8,
    ) -> AmlResult<AmlVmHandlerDispatchReport> {
        let prefix = match kind {
            AmlEventHandlerKind::Level => [b'_', b'L'],
            AmlEventHandlerKind::Edge => [b'_', b'E'],
        };
        self.dispatch_handler_kind(
            namespace,
            host,
            runtime,
            AmlMethodKind::EventHandler,
            prefix,
            event,
        )
    }

    fn should_run_initializer(
        &self,
        namespace: AmlLoadedNamespace<'_, '_>,
        evaluator: &AmlPureEvaluator<'_, '_>,
        host: &dyn AmlRegionAccessHost,
        runtime: &AmlRuntimeState<'_>,
        parent: Option<crate::aml::AmlNamespaceNodeId>,
    ) -> AmlResult<AmlInitializerDecision> {
        let Some(parent) = parent else {
            return Ok(AmlInitializerDecision::Run);
        };
        for sibling in namespace.records {
            if sibling.descriptor.parent != Some(parent) {
                continue;
            }
            let AmlNamespaceNodePayload::Method(method) = sibling.payload else {
                continue;
            };
            if method.kind != AmlMethodKind::Status {
                continue;
            }

            let outcome = evaluator.evaluate_with_host_and_state(
                host,
                runtime,
                AmlMethodInvocation {
                    method: method.node,
                    phase: AmlExecutionPhase::Initialization,
                    args: &[],
                },
            )?;
            if outcome.blocked {
                return Ok(AmlInitializerDecision::Blocked);
            }
            let Some(value) = outcome.return_value else {
                return Err(AmlError::invalid_state());
            };
            let status = value.as_integer()?;
            return Ok(if (status & 0x01) != 0 && (status & 0x08) != 0 {
                AmlInitializerDecision::Run
            } else {
                AmlInitializerDecision::Skip
            });
        }

        Ok(AmlInitializerDecision::Run)
    }

    fn dispatch_handler_kind(
        &mut self,
        namespace: AmlLoadedNamespace<'_, '_>,
        host: &dyn AmlRegionAccessHost,
        runtime: &AmlRuntimeState<'_>,
        expected_kind: AmlMethodKind,
        expected_prefix: [u8; 2],
        expected_code: u8,
    ) -> AmlResult<AmlVmHandlerDispatchReport> {
        let evaluator = AmlPureEvaluator::new(namespace);
        let mut report = AmlVmHandlerDispatchReport {
            invoked: 0,
            blocked: 0,
            missing: 0,
        };

        for record in namespace.records {
            let AmlNamespaceNodePayload::Method(method) = record.payload else {
                continue;
            };
            if method.kind != expected_kind {
                continue;
            }
            let Some(code) =
                method_suffix_code(record.descriptor.path.last_segment(), expected_prefix)
            else {
                continue;
            };
            if code != expected_code {
                continue;
            }

            let outcome = evaluator.evaluate_with_host_and_state(
                host,
                runtime,
                AmlMethodInvocation {
                    method: method.node,
                    phase: AmlExecutionPhase::Notification,
                    args: &[],
                },
            )?;
            report.invoked = report.invoked.saturating_add(1);
            if outcome.blocked {
                report.blocked = report.blocked.saturating_add(1);
                self.state = AmlVmState::Blocked;
                return Ok(report);
            }
        }

        if report.invoked == 0 {
            report.missing = 1;
        }
        self.state = AmlVmState::Ready;
        Ok(report)
    }
}

const fn address_space_argument(space: AmlAddressSpaceId) -> u64 {
    match space {
        AmlAddressSpaceId::SystemMemory => 0x00,
        AmlAddressSpaceId::SystemIo => 0x01,
        AmlAddressSpaceId::PciConfig => 0x02,
        AmlAddressSpaceId::EmbeddedControl => 0x03,
        AmlAddressSpaceId::SmBus => 0x04,
        AmlAddressSpaceId::Cmos => 0x05,
        AmlAddressSpaceId::PciBarTarget => 0x06,
        AmlAddressSpaceId::Ipmi => 0x07,
        AmlAddressSpaceId::Gpio => 0x08,
        AmlAddressSpaceId::GenericSerialBus => 0x09,
        AmlAddressSpaceId::PlatformCommChannel => 0x0a,
        AmlAddressSpaceId::FunctionalFixedHardware => 0x7f,
        AmlAddressSpaceId::Oem(value) => value as u64,
    }
}

fn method_suffix_code(
    segment: Option<crate::aml::AmlNameSeg>,
    expected_prefix: [u8; 2],
) -> Option<u8> {
    let bytes = segment?.bytes();
    if bytes[0] != expected_prefix[0] || bytes[1] != expected_prefix[1] {
        return None;
    }
    let high = hex_nibble(bytes[2])?;
    let low = hex_nibble(bytes[3])?;
    Some((high << 4) | low)
}

const fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use core::cell::Cell;
    use core::mem::MaybeUninit;
    use std::boxed::Box;

    use crate::aml::{
        AmlDefinitionBlock,
        AmlDefinitionBlockSet,
        AmlEmbeddedControllerHost,
        AmlHost,
        AmlNameSeg,
        AmlNamespaceLoadRecord,
        AmlNamespaceNodeId,
        AmlNotifySink,
        AmlOspmInterface,
        AmlSleepHost,
        AmlSystemIoHost,
        AmlSystemMemoryHost,
        AmlPciConfigHost,
        AmlAccessWidth,
        AmlNotifyEvent,
        AmlResolvedNamePath,
        AmlRuntimeIntegerSlot,
        AmlRuntimeMutexSlot,
        AmlRuntimeState,
        AmlResult,
    };
    use crate::pal::hal::acpi::Dsdt;
    use std::cell::RefCell;
    use std::vec::Vec;

    use super::*;

    fn encode_pkg_length(payload_len: usize) -> Vec<u8> {
        let one_byte_value = payload_len + 1;
        if one_byte_value < 0x40 {
            return vec![one_byte_value as u8];
        }
        let two_byte_value = payload_len + 2;
        vec![
            0b0100_0000 | ((two_byte_value & 0x0f) as u8),
            ((two_byte_value >> 4) & 0xff) as u8,
        ]
    }

    fn pkg(opcode: u8, payload: &[u8]) -> Vec<u8> {
        let pkg_length = encode_pkg_length(payload.len());
        let mut bytes = Vec::with_capacity(1 + pkg_length.len() + payload.len());
        bytes.push(opcode);
        bytes.extend_from_slice(&pkg_length);
        bytes.extend_from_slice(payload);
        bytes
    }

    fn method(name: [u8; 4], flags: u8, body: &[u8]) -> Vec<u8> {
        let mut payload = Vec::new();
        payload.extend_from_slice(&name);
        payload.push(flags);
        payload.extend_from_slice(body);
        pkg(0x14, &payload)
    }

    fn name_integer(name: [u8; 4], value: u8) -> Vec<u8> {
        vec![0x08, name[0], name[1], name[2], name[3], 0x0a, value]
    }

    fn scope(name: &[u8], body: &[u8]) -> Vec<u8> {
        let mut payload = Vec::new();
        payload.extend_from_slice(name);
        payload.extend_from_slice(body);
        pkg(0x10, &payload)
    }

    fn device(name: [u8; 4], body: &[u8]) -> Vec<u8> {
        let mut payload = Vec::new();
        payload.extend_from_slice(&name);
        payload.extend_from_slice(body);
        let pkg_length = encode_pkg_length(payload.len());
        let mut bytes = Vec::with_capacity(2 + pkg_length.len() + payload.len());
        bytes.push(0x5b);
        bytes.push(0x82);
        bytes.extend_from_slice(&pkg_length);
        bytes.extend_from_slice(&payload);
        bytes
    }

    fn opregion(name: [u8; 4], space: u8, offset: u8, length: u8) -> Vec<u8> {
        vec![
            0x5b, 0x80, name[0], name[1], name[2], name[3], space, 0x0a, offset, 0x0a, length,
        ]
    }

    fn definition_block(payload: &[u8]) -> AmlDefinitionBlock<'static> {
        let mut table = Vec::from([0_u8; 36]);
        table[0..4].copy_from_slice(b"DSDT");
        table[4..8].copy_from_slice(&((36 + payload.len()) as u32).to_le_bytes());
        table[8] = 2;
        table[10..16].copy_from_slice(b"FUSION");
        table[16..24].copy_from_slice(b"AMLVM___");
        table.extend_from_slice(payload);
        let checksum =
            (!table.iter().fold(0_u8, |sum, byte| sum.wrapping_add(*byte))).wrapping_add(1);
        table[9] = checksum;
        let leaked = Box::leak(table.into_boxed_slice());
        AmlDefinitionBlock::from_dsdt(Dsdt::parse(leaked).unwrap()).unwrap()
    }

    fn load_namespace(payload: &[u8]) -> AmlLoadedNamespace<'static, 'static> {
        let block = definition_block(payload);
        let plan = crate::aml::AmlNamespaceLoadPlan::from_definition_blocks(
            AmlDefinitionBlockSet::new(block, &[]),
        );
        let storage = Box::leak(Box::new(
            [MaybeUninit::<AmlNamespaceLoadRecord>::uninit(); 64],
        ));
        plan.load_into(storage).unwrap()
    }

    fn root_sb_path() -> AmlResolvedNamePath {
        let mut path = AmlResolvedNamePath::root();
        path.push(AmlNameSeg::from_bytes(*b"_SB_").unwrap())
            .unwrap();
        path
    }

    #[derive(Default)]
    struct FakeHost {
        notifications: RefCell<Vec<AmlNotifyEvent>>,
    }

    impl AmlOspmInterface for FakeHost {
        fn osi_supported(&self, _interface: &str) -> bool {
            false
        }
        fn os_revision(&self) -> u64 {
            0
        }
    }

    impl AmlSleepHost for FakeHost {
        fn stall_us(&self, _microseconds: u32) -> AmlResult<()> {
            Ok(())
        }
        fn sleep_ms(&self, _milliseconds: u32) -> AmlResult<()> {
            Ok(())
        }
    }

    impl AmlNotifySink for FakeHost {
        fn notify(&self, source: AmlNamespaceNodeId, value: u8) -> AmlResult<()> {
            self.notifications
                .borrow_mut()
                .push(AmlNotifyEvent { source, value });
            Ok(())
        }
    }

    impl AmlSystemMemoryHost for FakeHost {
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

    impl AmlSystemIoHost for FakeHost {
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

    impl AmlPciConfigHost for FakeHost {
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

    impl AmlEmbeddedControllerHost for FakeHost {
        fn read_embedded_controller(&self, _register: u8) -> AmlResult<u8> {
            Err(AmlError::unsupported())
        }
        fn write_embedded_controller(&self, _register: u8, _value: u8) -> AmlResult<()> {
            Err(AmlError::unsupported())
        }
    }

    impl AmlHost for FakeHost {}

    #[test]
    fn vm_registers_region_methods_against_sibling_opregions() {
        let mut device_body = Vec::new();
        device_body.extend_from_slice(&name_integer(*b"ST0R", 0));
        device_body.extend_from_slice(&opregion(*b"ECOR", 0x03, 0x00, 0x10));
        device_body.extend_from_slice(&method(
            *b"_REG",
            0x02,
            &[0x70, 0x69, b'S', b'T', b'0', b'R'],
        ));
        let payload = scope(b"\\_SB_", &device(*b"DEV0", &device_body));
        let namespace = load_namespace(&payload);
        let mut path = root_sb_path();
        path.push(AmlNameSeg::from_bytes(*b"DEV0").unwrap())
            .unwrap();
        path.push(AmlNameSeg::from_bytes(*b"ST0R").unwrap())
            .unwrap();
        let node = namespace.record_by_path(path).unwrap().descriptor.id;

        let integer_slots: [Cell<Option<AmlRuntimeIntegerSlot>>; 8] =
            core::array::from_fn(|_| Cell::new(None));
        let mutex_slots: [Cell<Option<AmlRuntimeMutexSlot>>; 4] =
            core::array::from_fn(|_| Cell::new(None));
        let runtime = AmlRuntimeState::new(&integer_slots).with_mutexes(&mutex_slots);
        let host = FakeHost::default();
        let mut vm = AmlVm::default();

        let report = vm.register_regions(namespace, &host, &runtime).unwrap();
        assert_eq!(report.invoked, 1);
        assert_eq!(report.skipped, 0);
        assert_eq!(runtime.read_integer(node), Some(1));
        assert_eq!(vm.state, AmlVmState::Ready);
    }

    #[test]
    fn vm_runs_ini_only_when_sta_reports_present_and_functioning() {
        let mut dev0 = Vec::new();
        dev0.extend_from_slice(&name_integer(*b"INIT", 0));
        dev0.extend_from_slice(&method(*b"_STA", 0x00, &[0xA4, 0x0A, 0x09]));
        dev0.extend_from_slice(&method(
            *b"_INI",
            0x00,
            &[0x70, 0x01, b'I', b'N', b'I', b'T'],
        ));

        let mut dev1 = Vec::new();
        dev1.extend_from_slice(&name_integer(*b"INIT", 0));
        dev1.extend_from_slice(&method(*b"_STA", 0x00, &[0xA4, 0x00]));
        dev1.extend_from_slice(&method(
            *b"_INI",
            0x00,
            &[0x70, 0x01, b'I', b'N', b'I', b'T'],
        ));

        let mut payload = scope(b"\\_SB_", &device(*b"DEV0", &dev0));
        payload.extend_from_slice(&scope(b"\\_SB_", &device(*b"DEV1", &dev1)));
        let namespace = load_namespace(&payload);

        let mut init0 = root_sb_path();
        init0
            .push(AmlNameSeg::from_bytes(*b"DEV0").unwrap())
            .unwrap();
        init0
            .push(AmlNameSeg::from_bytes(*b"INIT").unwrap())
            .unwrap();
        let init0_node = namespace.record_by_path(init0).unwrap().descriptor.id;

        let mut init1 = root_sb_path();
        init1
            .push(AmlNameSeg::from_bytes(*b"DEV1").unwrap())
            .unwrap();
        init1
            .push(AmlNameSeg::from_bytes(*b"INIT").unwrap())
            .unwrap();
        let init1_node = namespace.record_by_path(init1).unwrap().descriptor.id;

        let integer_slots: [Cell<Option<AmlRuntimeIntegerSlot>>; 8] =
            core::array::from_fn(|_| Cell::new(None));
        let mutex_slots: [Cell<Option<AmlRuntimeMutexSlot>>; 4] =
            core::array::from_fn(|_| Cell::new(None));
        let runtime = AmlRuntimeState::new(&integer_slots).with_mutexes(&mutex_slots);
        let host = FakeHost::default();
        let mut vm = AmlVm::default();

        let report = vm.initialize_devices(namespace, &host, &runtime).unwrap();
        assert_eq!(report.invoked, 1);
        assert_eq!(report.skipped, 1);
        assert_eq!(runtime.read_integer(init0_node), Some(1));
        assert_eq!(runtime.read_integer(init1_node), None);
        assert_eq!(vm.state, AmlVmState::Ready);
    }

    #[test]
    fn vm_dispatches_notification_query_handlers_by_hex_suffix() {
        let mut dev0 = Vec::new();
        dev0.extend_from_slice(&name_integer(*b"QHIT", 0));
        dev0.extend_from_slice(&method(
            *b"_Q66",
            0x00,
            &[0x70, 0x01, b'Q', b'H', b'I', b'T', 0xA4, 0x00],
        ));
        let payload = scope(b"\\_SB_", &device(*b"DEV0", &dev0));
        let namespace = load_namespace(&payload);

        let mut qhit = root_sb_path();
        qhit.push(AmlNameSeg::from_bytes(*b"DEV0").unwrap())
            .unwrap();
        qhit.push(AmlNameSeg::from_bytes(*b"QHIT").unwrap())
            .unwrap();
        let qhit_node = namespace.record_by_path(qhit).unwrap().descriptor.id;

        let integer_slots: [Cell<Option<AmlRuntimeIntegerSlot>>; 8] =
            core::array::from_fn(|_| Cell::new(None));
        let mutex_slots: [Cell<Option<AmlRuntimeMutexSlot>>; 4] =
            core::array::from_fn(|_| Cell::new(None));
        let runtime = AmlRuntimeState::new(&integer_slots).with_mutexes(&mutex_slots);
        let host = FakeHost::default();
        let mut vm = AmlVm::default();

        let report = vm
            .dispatch_notification_query(namespace, &host, &runtime, 0x66)
            .unwrap();
        assert_eq!(report.invoked, 1);
        assert_eq!(report.missing, 0);
        assert_eq!(runtime.read_integer(qhit_node), Some(1));
        assert_eq!(vm.state, AmlVmState::Ready);
    }

    #[test]
    fn vm_dispatches_level_event_handlers_by_hex_suffix() {
        let mut dev0 = Vec::new();
        dev0.extend_from_slice(&name_integer(*b"LHIT", 0));
        dev0.extend_from_slice(&method(
            *b"_L09",
            0x00,
            &[0x70, 0x01, b'L', b'H', b'I', b'T', 0xA4, 0x00],
        ));
        let payload = scope(b"\\_SB_", &device(*b"DEV0", &dev0));
        let namespace = load_namespace(&payload);

        let mut lhit = root_sb_path();
        lhit.push(AmlNameSeg::from_bytes(*b"DEV0").unwrap())
            .unwrap();
        lhit.push(AmlNameSeg::from_bytes(*b"LHIT").unwrap())
            .unwrap();
        let lhit_node = namespace.record_by_path(lhit).unwrap().descriptor.id;

        let integer_slots: [Cell<Option<AmlRuntimeIntegerSlot>>; 8] =
            core::array::from_fn(|_| Cell::new(None));
        let mutex_slots: [Cell<Option<AmlRuntimeMutexSlot>>; 4] =
            core::array::from_fn(|_| Cell::new(None));
        let runtime = AmlRuntimeState::new(&integer_slots).with_mutexes(&mutex_slots);
        let host = FakeHost::default();
        let mut vm = AmlVm::default();

        let report = vm
            .dispatch_event_handler(namespace, &host, &runtime, AmlEventHandlerKind::Level, 0x09)
            .unwrap();
        assert_eq!(report.invoked, 1);
        assert_eq!(report.missing, 0);
        assert_eq!(runtime.read_integer(lhit_node), Some(1));
        assert_eq!(vm.state, AmlVmState::Ready);
    }
}
