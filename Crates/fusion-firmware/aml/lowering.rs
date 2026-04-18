//! AML lowering vocabulary and backend-declared execution routing.

use fusion_pcu::contract::ir::PcuIrKind;
use fusion_hal::drivers::acpi::public::interface::backend::{
    AcpiAmlBackend,
    AcpiAmlLoweringKind,
};

use crate::aml::{
    AmlError,
    AmlEventHandlerKind,
    AmlLoadedNamespace,
    AmlNamespaceNodeId,
    AmlRegionAccessHost,
    AmlResolvedNamePath,
    AmlResult,
    AmlRuntimeState,
    AmlVm,
    AmlVmHandlerDispatchReport,
};

pub use fusion_hal::drivers::acpi::public::interface::backend::AcpiAmlLoweringKind as AmlLoweringTargetKind;

/// Maps one AML lowering lane to the corresponding PCU IR family when one exists.
#[must_use]
pub const fn lowering_target_pcu_ir_kind(target: AmlLoweringTargetKind) -> Option<PcuIrKind> {
    match target {
        AcpiAmlLoweringKind::Interpret => None,
        AcpiAmlLoweringKind::Command => Some(PcuIrKind::Command),
        AcpiAmlLoweringKind::Signal => Some(PcuIrKind::Signal),
        AcpiAmlLoweringKind::Transaction => Some(PcuIrKind::Transaction),
        AcpiAmlLoweringKind::Dispatch => Some(PcuIrKind::Dispatch),
    }
}

/// One AML lowering plan for a namespace method or handler.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AmlLoweringPlan {
    pub source: AmlNamespaceNodeId,
    pub target: AmlLoweringTargetKind,
}

/// Marker trait for lowering targets.
pub trait AmlLoweringTarget {
    fn lowering_target(&self) -> AmlLoweringTargetKind;
}

pub fn backend_notification_query_target<B: AcpiAmlBackend>(
    provider: u8,
    query: u8,
) -> Option<AmlLoweringTargetKind> {
    backend_method_target::<B>(provider, query_handler_suffix(query))
}

pub fn backend_event_handler_target<B: AcpiAmlBackend>(
    provider: u8,
    kind: AmlEventHandlerKind,
    event: u8,
) -> Option<AmlLoweringTargetKind> {
    backend_method_target::<B>(provider, event_handler_suffix(kind, event))
}

pub fn dispatch_backend_notification_query<'records, 'blocks, B: AcpiAmlBackend>(
    provider: u8,
    vm: &mut AmlVm,
    namespace: AmlLoadedNamespace<'records, 'blocks>,
    host: &dyn AmlRegionAccessHost,
    runtime: &AmlRuntimeState<'_>,
    query: u8,
) -> AmlResult<AmlVmHandlerDispatchReport> {
    match backend_notification_query_target::<B>(provider, query) {
        Some(AcpiAmlLoweringKind::Signal) => {
            vm.dispatch_notification_query(namespace, host, runtime, query)
        }
        _ => Err(AmlError::unsupported()),
    }
}

pub fn dispatch_backend_event_handler<'records, 'blocks, B: AcpiAmlBackend>(
    provider: u8,
    vm: &mut AmlVm,
    namespace: AmlLoadedNamespace<'records, 'blocks>,
    host: &dyn AmlRegionAccessHost,
    runtime: &AmlRuntimeState<'_>,
    kind: AmlEventHandlerKind,
    event: u8,
) -> AmlResult<AmlVmHandlerDispatchReport> {
    match backend_event_handler_target::<B>(provider, kind, event) {
        Some(AcpiAmlLoweringKind::Signal) => {
            vm.dispatch_event_handler(namespace, host, runtime, kind, event)
        }
        _ => Err(AmlError::unsupported()),
    }
}

fn backend_method_target<B: AcpiAmlBackend>(
    provider: u8,
    expected_suffix: [u8; 4],
) -> Option<AmlLoweringTargetKind> {
    for method in B::aml_methods(provider) {
        let path = AmlResolvedNamePath::parse_text(method.path).ok()?;
        if path.last_segment()?.bytes() == expected_suffix {
            return Some(method.lowering);
        }
    }
    None
}

const fn query_handler_suffix(query: u8) -> [u8; 4] {
    [b'_', b'Q', hex_upper(query >> 4), hex_upper(query & 0x0f)]
}

const fn event_handler_suffix(kind: AmlEventHandlerKind, event: u8) -> [u8; 4] {
    let prefix = match kind {
        AmlEventHandlerKind::Level => b'L',
        AmlEventHandlerKind::Edge => b'E',
    };
    [b'_', prefix, hex_upper(event >> 4), hex_upper(event & 0x0f)]
}

const fn hex_upper(nibble: u8) -> u8 {
    match nibble & 0x0f {
        0..=9 => b'0' + (nibble & 0x0f),
        other => b'A' + (other - 10),
    }
}

#[cfg(test)]
mod tests {
    use core::cell::Cell;
    use core::mem::MaybeUninit;
    use std::boxed::Box;
    use std::vec::Vec;

    use fusion_hal::contract::drivers::acpi::AcpiError;
    use fusion_hal::contract::drivers::acpi::AcpiProviderDescriptor;
    use fusion_hal::drivers::acpi::public::interface::backend::{
        AcpiAmlFieldDescriptor,
        AcpiAmlMethodDescriptor,
        AcpiAmlNamespaceDescriptor,
        AcpiAmlOpRegionDescriptor,
    };
    use fusion_hal::drivers::acpi::public::interface::contract::AcpiHardware;
    use fusion_hal::drivers::acpi::vendor::dell::DellLatitudeE6430AcpiHardware;

    use super::*;
    use crate::aml::{
        AmlAccessWidth,
        AmlDefinitionBlock,
        AmlDefinitionBlockSet,
        AmlEmbeddedControllerHost,
        AmlHost,
        AmlNameSeg,
        AmlNamespaceLoadPlan,
        AmlNamespaceLoadRecord,
        AmlNotifyEvent,
        AmlNotifySink,
        AmlOspmInterface,
        AmlSleepHost,
        AmlSystemIoHost,
        AmlSystemMemoryHost,
        AmlPciConfigHost,
        AmlRuntimeIntegerSlot,
        AmlRuntimeMutexSlot,
    };
    use crate::pal::hal::acpi::Dsdt;

    #[derive(Debug, Clone, Copy, Default)]
    struct FakeSignalBackend;

    const FAKE_PROVIDER: AcpiProviderDescriptor = AcpiProviderDescriptor {
        id: "fake-signal-backend",
        vendor: "Fusion",
        platform: "Synthetic",
        description: "Synthetic AML signal proving backend",
    };

    const FAKE_NAMESPACE: AcpiAmlNamespaceDescriptor = AcpiAmlNamespaceDescriptor {
        root: "\\_SB",
        description: "Synthetic root",
    };

    const FAKE_METHODS: [AcpiAmlMethodDescriptor; 2] = [
        AcpiAmlMethodDescriptor {
            path: "\\_SB.DEV0._Q66",
            lowering: AcpiAmlLoweringKind::Signal,
            description: "Synthetic query handler",
        },
        AcpiAmlMethodDescriptor {
            path: "\\_SB.DEV0._L09",
            lowering: AcpiAmlLoweringKind::Signal,
            description: "Synthetic level handler",
        },
    ];

    impl AcpiHardware for FakeSignalBackend {
        fn provider_count() -> u8 {
            1
        }

        fn provider(provider: u8) -> Option<&'static AcpiProviderDescriptor> {
            (provider == 0).then_some(&FAKE_PROVIDER)
        }
    }

    impl AcpiAmlBackend for FakeSignalBackend {
        fn aml_namespace(provider: u8) -> Result<AcpiAmlNamespaceDescriptor, AcpiError> {
            if provider == 0 {
                Ok(FAKE_NAMESPACE)
            } else {
                Err(AcpiError::invalid())
            }
        }

        fn aml_methods(provider: u8) -> &'static [AcpiAmlMethodDescriptor] {
            if provider == 0 { &FAKE_METHODS } else { &[] }
        }

        fn aml_fields(_provider: u8) -> &'static [AcpiAmlFieldDescriptor] {
            &[]
        }

        fn aml_opregions(_provider: u8) -> &'static [AcpiAmlOpRegionDescriptor] {
            &[]
        }
    }

    #[derive(Default)]
    struct FakeHost {
        notifications: std::cell::RefCell<Vec<AmlNotifyEvent>>,
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
        fn notify(&self, source: crate::aml::AmlNamespaceNodeId, value: u8) -> AmlResult<()> {
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

    fn definition_block(payload: &[u8]) -> AmlDefinitionBlock<'static> {
        let mut table = Vec::from([0_u8; 36]);
        table[0..4].copy_from_slice(b"DSDT");
        table[4..8].copy_from_slice(&((36 + payload.len()) as u32).to_le_bytes());
        table[8] = 2;
        table[10..16].copy_from_slice(b"FUSION");
        table[16..24].copy_from_slice(b"AMLLOWER");
        table.extend_from_slice(payload);
        let checksum =
            (!table.iter().fold(0_u8, |sum, byte| sum.wrapping_add(*byte))).wrapping_add(1);
        table[9] = checksum;
        let leaked = Box::leak(table.into_boxed_slice());
        AmlDefinitionBlock::from_dsdt(Dsdt::parse(leaked).unwrap()).unwrap()
    }

    fn load_namespace(payload: &[u8]) -> AmlLoadedNamespace<'static, 'static> {
        let block = definition_block(payload);
        let plan =
            AmlNamespaceLoadPlan::from_definition_blocks(AmlDefinitionBlockSet::new(block, &[]));
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

    #[test]
    fn dell_backend_signal_query_target_is_visible() {
        assert_eq!(
            backend_notification_query_target::<DellLatitudeE6430AcpiHardware>(0, 0x66),
            Some(AcpiAmlLoweringKind::Signal)
        );
        assert_eq!(
            backend_notification_query_target::<DellLatitudeE6430AcpiHardware>(0, 0x67),
            None
        );
    }

    #[test]
    fn backend_dispatch_query_routes_through_vm_only_for_signal_targets() {
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

        let report = dispatch_backend_notification_query::<FakeSignalBackend>(
            0, &mut vm, namespace, &host, &runtime, 0x66,
        )
        .unwrap();
        assert_eq!(report.invoked, 1);
        assert_eq!(runtime.read_integer(qhit_node), Some(1));
    }
}
