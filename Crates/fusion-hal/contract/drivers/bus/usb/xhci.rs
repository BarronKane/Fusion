//! xHCI-specific host-controller vocabulary.

use super::controller::*;
use super::core::*;
use super::host::*;

/// Observable xHCI context-size mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum XhciContextSize {
    Bytes32,
    Bytes64,
}

/// One xHCI supported-protocol capability record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct XhciSupportedProtocol {
    pub revision: UsbSpecRevision,
    pub compatible_port_offset: u8,
    pub compatible_port_count: u8,
    pub protocol_slot_type: Option<u8>,
    pub name_string: Option<&'static str>,
}

/// xHCI capability-register truth that does not belong in the generic USB controller lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct XhciCapabilities {
    pub max_device_slots: u8,
    pub max_interrupters: u16,
    pub max_ports: u8,
    pub max_scratchpad_buffers: Option<u16>,
    pub context_size: XhciContextSize,
    pub bandwidth_negotiation: bool,
    pub port_power_control: bool,
    pub port_indicators: bool,
    pub light_host_controller_reset: bool,
    pub latency_tolerance_messaging: bool,
    pub parse_all_event_data: bool,
    pub stopped_short_packet: bool,
    pub stopped_edtla: bool,
    pub contiguous_frame_id: bool,
    pub large_esit_payload: bool,
    pub extended_tbc: bool,
}

impl Default for XhciCapabilities {
    fn default() -> Self {
        Self {
            max_device_slots: 0,
            max_interrupters: 0,
            max_ports: 0,
            max_scratchpad_buffers: None,
            context_size: XhciContextSize::Bytes32,
            bandwidth_negotiation: false,
            port_power_control: false,
            port_indicators: false,
            light_host_controller_reset: false,
            latency_tolerance_messaging: false,
            parse_all_event_data: false,
            stopped_short_packet: false,
            stopped_edtla: false,
            contiguous_frame_id: false,
            large_esit_payload: false,
            extended_tbc: false,
        }
    }
}

/// xHCI host-controller extension layered on the generic USB host/controller contracts.
pub trait XhciHostControllerContract: UsbHostControllerContract + UsbControllerContract {
    /// Returns xHCI-specific capability-register truth.
    fn xhci_capabilities(&self) -> XhciCapabilities;

    /// Returns the supported protocol descriptors surfaced by xHCI extended capabilities.
    fn xhci_supported_protocols(&self) -> &[XhciSupportedProtocol];
}
