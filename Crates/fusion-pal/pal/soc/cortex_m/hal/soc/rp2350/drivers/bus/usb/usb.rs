//! RP2350 USB controller substrate.
//!
//! The current RP2350 cut is intentionally device-side first:
//! - the Pico 2 W needs to enumerate honestly on its upstream USB port
//! - host-mode/controller-family work comes later
//! - endpoint-zero control is enough to prove the contract and the PAL path

use core::cell::UnsafeCell;
use core::cmp::min;
use core::mem::MaybeUninit;
use core::ptr;
use core::slice;
use core::sync::atomic::{
    AtomicU8,
    Ordering,
};

use cortex_m::interrupt;
use fd_bus_usb::interface::contract::{
    UsbHardware as UsbHardwareContract,
    UsbHardwareTopology,
};
use fusion_hal::contract::drivers::bus::usb::{
    UsbConfigurationDescriptor,
    UsbConnectorKind,
    UsbControllerAttachment,
    UsbControllerCapabilities,
    UsbControllerContract,
    UsbControllerDiscoverySource,
    UsbControllerIdentity,
    UsbControllerKind,
    UsbControllerMetadata,
    UsbControllerRole,
    UsbCoreCapabilities,
    UsbCoreContract,
    UsbCoreMetadata,
    UsbDeviceAddress,
    UsbDeviceControllerContract,
    UsbDeviceDescriptor,
    UsbDeviceEndpointConfiguration,
    UsbDeviceState,
    UsbDirection,
    UsbError,
    UsbHostControllerContract,
    UsbHostDeviceContract,
    UsbHostDeviceState,
    UsbImplementationKind,
    UsbInterfaceDescriptor,
    UsbMmioWindow,
    UsbPortId,
    UsbPortStatus,
    UsbRequestKind,
    UsbRequestRecipient,
    UsbSetupPacket,
    UsbSpecRevision,
    UsbSpeed,
    UsbSpeedSupport,
    UsbSupport,
    UsbTransferCompletion,
    UsbTransferRequest,
    UsbEndpointAddress,
    UsbDescriptorType,
};

use crate::pal::soc::cortex_m::hal::soc::rp2350::ensure_boot_clocks_initialized;

const RP2350_USBCTRL_DPRAM_BASE: usize = 0x5010_0000;
const RP2350_USBCTRL_REGS_BASE: usize = 0x5011_0000;
const RP2350_USBCTRL_DPRAM_BYTES: usize = 4096;
const RP2350_USBCTRL_IRQN: u16 = 14;
const CORTEX_M_EXTERNAL_EXCEPTION_BASE: i16 = 16;
const RP2350_USB_EP0_MAX_PACKET_SIZE: usize = 64;

const RP2350_RESETS_BASE: usize = 0x4002_0000;
const RP2350_RESETS_RESET_OFFSET: usize = 0x00;
const RP2350_RESETS_RESET_DONE_OFFSET: usize = 0x08;
const RP2350_RESETS_RESET_USBCTRL_BITS: u32 = 0x1000_0000;

const USB_ADDR_ENDP_OFFSET: usize = 0x00;
const USB_MAIN_CTRL_OFFSET: usize = 0x40;
const USB_SIE_CTRL_OFFSET: usize = 0x4c;
const USB_SIE_STATUS_OFFSET: usize = 0x50;
const USB_BUFF_STATUS_OFFSET: usize = 0x58;
const USB_EP_STALL_ARM_OFFSET: usize = 0x68;
const USB_USB_MUXING_OFFSET: usize = 0x74;
const USB_USB_PWR_OFFSET: usize = 0x78;
const USB_INTE_OFFSET: usize = 0x90;
const USB_INTS_OFFSET: usize = 0x98;

const USB_MAIN_CTRL_CONTROLLER_EN_BITS: u32 = 0x0000_0001;

const USB_SIE_CTRL_EP0_INT_1BUF_BITS: u32 = 0x2000_0000;
const USB_SIE_CTRL_PULLUP_EN_BITS: u32 = 0x0001_0000;

const USB_SIE_STATUS_BUS_RESET_BITS: u32 = 0x0008_0000;
const USB_SIE_STATUS_SETUP_REC_BITS: u32 = 0x0002_0000;
const USB_SIE_STATUS_CONNECTED_BITS: u32 = 0x0001_0000;
const USB_SIE_STATUS_SUSPENDED_BITS: u32 = 0x0000_0010;

const USB_BUFF_STATUS_EP0_OUT_BITS: u32 = 0x0000_0002;
const USB_BUFF_STATUS_EP0_IN_BITS: u32 = 0x0000_0001;

const USB_EP_STALL_ARM_EP0_OUT_BITS: u32 = 0x0000_0002;
const USB_EP_STALL_ARM_EP0_IN_BITS: u32 = 0x0000_0001;

const USB_USB_MUXING_TO_PHY_BITS: u32 = 0x0000_0001;
const USB_USB_MUXING_SOFTCON_BITS: u32 = 0x0000_0008;

const USB_USB_PWR_VBUS_DETECT_BITS: u32 = 0x0000_0004;
const USB_USB_PWR_VBUS_DETECT_OVERRIDE_EN_BITS: u32 = 0x0000_0008;

const USB_INTE_SETUP_REQ_BITS: u32 = 0x0001_0000;
const USB_INTE_BUS_RESET_BITS: u32 = 0x0000_1000;
const USB_INTE_BUFF_STATUS_BITS: u32 = 0x0000_0010;

const USB_BUF_CTRL_FULL: u32 = 0x0000_8000;
const USB_BUF_CTRL_LAST: u32 = 0x0000_4000;
const USB_BUF_CTRL_DATA1_PID: u32 = 0x0000_2000;
const USB_BUF_CTRL_RESET_SEL: u32 = 0x0000_1000;
const USB_BUF_CTRL_STALL: u32 = 0x0000_0800;
const USB_BUF_CTRL_AVAIL: u32 = 0x0000_0400;
const USB_BUF_CTRL_LEN_MASK: u32 = 0x0000_03ff;

const USB_REQUEST_GET_STATUS: u8 = 0x00;
const USB_REQUEST_CLEAR_FEATURE: u8 = 0x01;
const USB_REQUEST_SET_FEATURE: u8 = 0x03;
const USB_REQUEST_SET_ADDRESS: u8 = 0x05;
const USB_REQUEST_GET_DESCRIPTOR: u8 = 0x06;
const USB_REQUEST_GET_CONFIGURATION: u8 = 0x08;
const USB_REQUEST_SET_CONFIGURATION: u8 = 0x09;
const USB_REQUEST_GET_INTERFACE: u8 = 0x0a;
const USB_REQUEST_SET_INTERFACE: u8 = 0x0b;

const USB_STRING_LANGUAGES_INDEX: u8 = 0;
const USB_STRING_MANUFACTURER_INDEX: u8 = 1;
const USB_STRING_PRODUCT_INDEX: u8 = 2;
const USB_STRING_SERIAL_INDEX: u8 = 3;
const USB_STRING_INTERFACE_INDEX: u8 = 4;
const USB_LANGUAGE_EN_US: u16 = 0x0409;

const RP2350_USB_RUNTIME_UNINITIALIZED: u8 = 0;
const RP2350_USB_RUNTIME_RUNNING: u8 = 1;
const RP2350_USB_RUNTIME_READY: u8 = 2;

const RP2350_USB_DEVICE_DESCRIPTOR: UsbDeviceDescriptor = UsbDeviceDescriptor {
    usb_revision: UsbSpecRevision::USB_2_0,
    device_class: 0,
    device_subclass: 0,
    device_protocol: 0,
    max_packet_size_ep0: 64,
    vendor_id: 0xcafe,
    product_id: 0x4020,
    device_revision: 0x0001,
    manufacturer_string_index: USB_STRING_MANUFACTURER_INDEX,
    product_string_index: USB_STRING_PRODUCT_INDEX,
    serial_number_string_index: USB_STRING_SERIAL_INDEX,
    configuration_count: 1,
};

const RP2350_USB_CONFIGURATION_DESCRIPTOR: UsbConfigurationDescriptor =
    UsbConfigurationDescriptor {
        total_length: 18,
        interface_count: 1,
        configuration_value: 1,
        configuration_string_index: 0,
        attributes: 0x80,
        max_power_raw: 50,
    };

const RP2350_USB_INTERFACE_DESCRIPTOR: UsbInterfaceDescriptor = UsbInterfaceDescriptor {
    interface_number: 0,
    alternate_setting: 0,
    endpoint_count: 0,
    interface_class: 0xff,
    interface_subclass: 0,
    interface_protocol: 0,
    interface_string_index: USB_STRING_INTERFACE_INDEX,
};

const RP2350_USB_DEVICE_DESCRIPTOR_BYTES: [u8; 18] = [
    18,
    0x01,
    0x00,
    0x02,
    0x00,
    0x00,
    0x00,
    64,
    0xfe,
    0xca,
    0x20,
    0x40,
    0x01,
    0x00,
    USB_STRING_MANUFACTURER_INDEX,
    USB_STRING_PRODUCT_INDEX,
    USB_STRING_SERIAL_INDEX,
    1,
];

const RP2350_USB_CONFIGURATION_DESCRIPTOR_BYTES: [u8; 18] = [
    9,
    0x02,
    18,
    0,
    1,
    1,
    0,
    0x80,
    50,
    9,
    0x04,
    0,
    0,
    0,
    0xff,
    0x00,
    0x00,
    USB_STRING_INTERFACE_INDEX,
];

const RP2350_USB_MANUFACTURER_STRING: &[u8] = b"Fusion";
const RP2350_USB_PRODUCT_STRING: &[u8] = b"Fusion Debug";
const RP2350_USB_SERIAL_STRING: &[u8] = b"RP2350";
const RP2350_USB_INTERFACE_STRING: &[u8] = b"Debug Channel";

const RP2350_USB_SUPPORT: UsbSupport = UsbSupport {
    implementation: UsbImplementationKind::Hardware,
    host_controller: false,
    device_controller: true,
    typec: false,
    pd: false,
    usb4: false,
    thunderbolt: false,
};

const RP2350_USB_CORE_METADATA: UsbCoreMetadata = UsbCoreMetadata {
    declared_revision: Some(UsbSpecRevision::USB_2_0),
    observed_minimum_revision: Some(UsbSpecRevision::USB_2_0),
    observed_maximum_revision: Some(UsbSpecRevision::USB_2_0),
    supported_speeds: UsbSpeedSupport {
        low_speed: false,
        full_speed: true,
        high_speed: false,
        super_speed: false,
        super_speed_plus: false,
    },
    capabilities: UsbCoreCapabilities {
        control_transfer: true,
        bulk_transfer: false,
        interrupt_transfer: false,
        isochronous_transfer: false,
        bos_descriptor: false,
        lpm: false,
        streams: false,
        otg: false,
        composite: false,
        self_powered: false,
        remote_wakeup: false,
    },
    usb4_capable: false,
};

const RP2350_USB_CONTROLLER_METADATA: UsbControllerMetadata = UsbControllerMetadata {
    kind: UsbControllerKind::VendorSpecific,
    role: UsbControllerRole::Device,
    discovery_source: UsbControllerDiscoverySource::StaticSoc,
    attachment: UsbControllerAttachment::Mmio(UsbMmioWindow {
        base: RP2350_USBCTRL_REGS_BASE as u64,
        length: 0x1_0000,
    }),
    identity: UsbControllerIdentity {
        vendor_id: None,
        device_id: None,
        revision_id: None,
        programming_interface: None,
    },
    interrupt_vectors: Some(1),
    visible_ports: Some(1),
};

const RP2350_USB_CONTROLLER_CAPABILITIES: UsbControllerCapabilities = UsbControllerCapabilities {
    dma: false,
    sixty_four_bit_addressing: false,
    multiple_interrupters: false,
    streams: false,
    port_power_control: false,
    companion_controllers: false,
    runtime_power_management: false,
};

#[repr(C)]
struct UsbEndpointControlPair {
    in_: u32,
    out: u32,
}

#[repr(C)]
struct UsbEndpointBufferControlPair {
    in_: u32,
    out: u32,
}

#[repr(C)]
struct Rp2350UsbDeviceDpram {
    setup_packet: [u8; 8],
    ep_ctrl: [UsbEndpointControlPair; 15],
    ep_buf_ctrl: [UsbEndpointBufferControlPair; 16],
    ep0_buf_a: [u8; 64],
    ep0_buf_b: [u8; 64],
    epx_data: [u8; RP2350_USBCTRL_DPRAM_BYTES - 0x180],
}

#[derive(Debug, Clone, Copy, Default)]
pub struct UsbHardware;

#[derive(Debug, Clone, Copy, Default)]
pub struct UsbDeviceController;

#[derive(Debug, Clone, Copy, Default)]
pub struct UsbHostController;

#[derive(Debug, Clone, Copy, Default)]
pub struct UsbHostDevice;

struct Rp2350UsbRuntime {
    pending_address: Option<u8>,
    device_address: u8,
    active_configuration: u8,
    ep0_in_data1: bool,
    ep0_out_data1: bool,
    expect_status_out: bool,
    bus_reset_seen: bool,
}

impl Rp2350UsbRuntime {
    const fn new() -> Self {
        Self {
            pending_address: None,
            device_address: 0,
            active_configuration: 0,
            ep0_in_data1: true,
            ep0_out_data1: true,
            expect_status_out: false,
            bus_reset_seen: false,
        }
    }

    fn reset_for_bus(&mut self) {
        self.pending_address = None;
        self.device_address = 0;
        self.active_configuration = 0;
        self.ep0_in_data1 = true;
        self.ep0_out_data1 = true;
        self.expect_status_out = false;
        self.bus_reset_seen = true;
    }
}

struct UsbRuntimeSlot {
    state: AtomicU8,
    value: UnsafeCell<MaybeUninit<Rp2350UsbRuntime>>,
}

impl UsbRuntimeSlot {
    const fn new() -> Self {
        Self {
            state: AtomicU8::new(RP2350_USB_RUNTIME_UNINITIALIZED),
            value: UnsafeCell::new(MaybeUninit::uninit()),
        }
    }

    fn ensure_ready(&self) -> Result<(), UsbError> {
        loop {
            match self.state.load(Ordering::Acquire) {
                RP2350_USB_RUNTIME_READY => return Ok(()),
                RP2350_USB_RUNTIME_UNINITIALIZED => {
                    if self
                        .state
                        .compare_exchange(
                            RP2350_USB_RUNTIME_UNINITIALIZED,
                            RP2350_USB_RUNTIME_RUNNING,
                            Ordering::AcqRel,
                            Ordering::Acquire,
                        )
                        .is_err()
                    {
                        continue;
                    }

                    let init_result = interrupt::free(|_| rp2350_usb_initialize_controller());
                    match init_result {
                        Ok(runtime) => {
                            unsafe { (*self.value.get()).write(runtime) };
                            self.state
                                .store(RP2350_USB_RUNTIME_READY, Ordering::Release);
                            match rp2350_usb_activate_controller() {
                                Ok(()) => return Ok(()),
                                Err(error) => {
                                    self.state
                                        .store(RP2350_USB_RUNTIME_UNINITIALIZED, Ordering::Release);
                                    return Err(error);
                                }
                            }
                        }
                        Err(error) => {
                            self.state
                                .store(RP2350_USB_RUNTIME_UNINITIALIZED, Ordering::Release);
                            return Err(error);
                        }
                    }
                }
                RP2350_USB_RUNTIME_RUNNING => core::hint::spin_loop(),
                _ => core::hint::spin_loop(),
            }
        }
    }

    fn with<R>(&self, f: impl FnOnce(&Rp2350UsbRuntime) -> R) -> Result<R, UsbError> {
        self.ensure_ready()?;
        Ok(interrupt::free(|_| {
            let runtime = unsafe { &*(*self.value.get()).as_ptr() };
            f(runtime)
        }))
    }

    fn with_mut<R>(&self, f: impl FnOnce(&mut Rp2350UsbRuntime) -> R) -> Result<R, UsbError> {
        self.ensure_ready()?;
        Ok(interrupt::free(|_| {
            let runtime = unsafe { &mut *(*self.value.get()).as_mut_ptr() };
            f(runtime)
        }))
    }
}

unsafe impl Sync for UsbRuntimeSlot {}

static RP2350_USB_RUNTIME: UsbRuntimeSlot = UsbRuntimeSlot::new();

/// Canonical RP2350 USB device-controller runtime IRQ line.
pub const USB_RUNTIME_IRQN: u16 = RP2350_USBCTRL_IRQN;

enum SetupResponse {
    DataIn(usize),
    AckIn,
    Stall,
}

impl UsbHardwareContract for UsbHardware {
    type HostController = UsbHostController;
    type DeviceController = UsbDeviceController;

    fn support() -> UsbSupport {
        RP2350_USB_SUPPORT
    }

    fn core_metadata() -> UsbCoreMetadata {
        RP2350_USB_CORE_METADATA
    }

    fn host_controller() -> Result<Option<Self::HostController>, UsbError> {
        Ok(None)
    }

    fn device_controller() -> Result<Option<Self::DeviceController>, UsbError> {
        RP2350_USB_RUNTIME.ensure_ready()?;
        Ok(Some(UsbDeviceController))
    }
}

impl UsbHardwareTopology for UsbHardware {
    fn topology_port_count() -> usize {
        1
    }

    fn topology_port_status(port: UsbPortId) -> Result<UsbPortStatus, UsbError> {
        if port.parent_device.is_some() || port.port_number != 1 {
            return Err(UsbError::invalid());
        }

        let state = rp2350_usb_device_state()?;
        Ok(UsbPortStatus {
            connected: rp2350_usb_connected(),
            enabled: matches!(
                state,
                UsbDeviceState::Default | UsbDeviceState::Addressed | UsbDeviceState::Configured
            ),
            powered: !matches!(state, UsbDeviceState::Detached),
            overcurrent: false,
            reset_in_progress: false,
            suspended: matches!(state, UsbDeviceState::Suspended),
            connector: UsbConnectorKind::MicroB,
            negotiated_speed: rp2350_usb_connected().then_some(UsbSpeed::Full),
            typec_orientation: None,
            data_role: None,
            power_role: None,
            usb4_capable: false,
            thunderbolt_compatible: false,
        })
    }
}

impl UsbCoreContract for UsbHostController {
    fn usb_support(&self) -> UsbSupport {
        UsbSupport::unsupported()
    }

    fn usb_core_metadata(&self) -> UsbCoreMetadata {
        UsbCoreMetadata::default()
    }
}

impl UsbControllerContract for UsbHostController {
    fn controller_metadata(&self) -> UsbControllerMetadata {
        UsbControllerMetadata::default()
    }

    fn controller_capabilities(&self) -> UsbControllerCapabilities {
        UsbControllerCapabilities::default()
    }
}

impl fusion_hal::contract::drivers::bus::usb::UsbTopologyContract for UsbHostController {
    fn port_count(&self) -> usize {
        0
    }

    fn port_status(&self, _port: UsbPortId) -> Result<UsbPortStatus, UsbError> {
        Err(UsbError::unsupported())
    }
}

impl UsbHostControllerContract for UsbHostController {
    type Device = UsbHostDevice;

    fn enumerate(&mut self, _port: UsbPortId) -> Result<Self::Device, UsbError> {
        Err(UsbError::unsupported())
    }

    fn get_descriptor(
        &mut self,
        _address: UsbDeviceAddress,
        _descriptor_type: UsbDescriptorType,
        _descriptor_index: u8,
        _language_id: u16,
        _buffer: &mut [u8],
    ) -> Result<usize, UsbError> {
        Err(UsbError::unsupported())
    }
}

impl UsbCoreContract for UsbHostDevice {
    fn usb_support(&self) -> UsbSupport {
        UsbSupport::unsupported()
    }

    fn usb_core_metadata(&self) -> UsbCoreMetadata {
        UsbCoreMetadata::default()
    }
}

impl UsbHostDeviceContract for UsbHostDevice {
    fn address(&self) -> UsbDeviceAddress {
        UsbDeviceAddress(0)
    }

    fn state(&self) -> fusion_hal::contract::drivers::bus::usb::UsbHostDeviceState {
        UsbHostDeviceState::Detached
    }

    fn device_descriptor(&self) -> UsbDeviceDescriptor {
        RP2350_USB_DEVICE_DESCRIPTOR
    }

    fn configuration_descriptor(&self) -> Option<UsbConfigurationDescriptor> {
        Some(RP2350_USB_CONFIGURATION_DESCRIPTOR)
    }

    fn interface_descriptors(&self) -> &[UsbInterfaceDescriptor] {
        slice::from_ref(&RP2350_USB_INTERFACE_DESCRIPTOR)
    }

    fn endpoint_descriptors(
        &self,
    ) -> &[fusion_hal::contract::drivers::bus::usb::UsbEndpointDescriptor] {
        &[]
    }

    fn control_transfer<'a>(
        &mut self,
        _setup: UsbSetupPacket,
        _data: &'a mut [u8],
    ) -> Result<&'a [u8], UsbError> {
        Err(UsbError::unsupported())
    }

    fn submit_transfer<'a>(
        &mut self,
        _request: UsbTransferRequest<'a>,
    ) -> Result<UsbTransferCompletion<'a>, UsbError> {
        Err(UsbError::unsupported())
    }
}

impl UsbCoreContract for UsbDeviceController {
    fn usb_support(&self) -> UsbSupport {
        RP2350_USB_SUPPORT
    }

    fn usb_core_metadata(&self) -> UsbCoreMetadata {
        RP2350_USB_CORE_METADATA
    }
}

impl UsbControllerContract for UsbDeviceController {
    fn controller_metadata(&self) -> UsbControllerMetadata {
        RP2350_USB_CONTROLLER_METADATA
    }

    fn controller_capabilities(&self) -> UsbControllerCapabilities {
        RP2350_USB_CONTROLLER_CAPABILITIES
    }
}

impl UsbDeviceControllerContract for UsbDeviceController {
    fn device_state(&self) -> UsbDeviceState {
        rp2350_usb_service_pending_best_effort();
        rp2350_usb_device_state().unwrap_or(UsbDeviceState::Detached)
    }

    fn device_descriptor(&self) -> UsbDeviceDescriptor {
        RP2350_USB_DEVICE_DESCRIPTOR
    }

    fn configuration_descriptors(&self) -> &[UsbConfigurationDescriptor] {
        slice::from_ref(&RP2350_USB_CONFIGURATION_DESCRIPTOR)
    }

    fn interface_descriptors(&self) -> &[UsbInterfaceDescriptor] {
        slice::from_ref(&RP2350_USB_INTERFACE_DESCRIPTOR)
    }

    fn configure_endpoint(
        &mut self,
        _endpoint: UsbDeviceEndpointConfiguration,
    ) -> Result<(), UsbError> {
        Err(UsbError::unsupported())
    }

    fn queue_in(&mut self, endpoint: UsbEndpointAddress, payload: &[u8]) -> Result<(), UsbError> {
        if endpoint.number.0 == 0
            && matches!(endpoint.direction, UsbDirection::In)
            && payload.len() <= 64
        {
            RP2350_USB_RUNTIME.with_mut(|runtime| {
                rp2350_usb_start_ep0_in(runtime, payload, true);
            })?;
            return Ok(());
        }
        Err(UsbError::unsupported())
    }

    fn dequeue_out<'a>(
        &mut self,
        _endpoint: UsbEndpointAddress,
        _buffer: &'a mut [u8],
    ) -> Result<&'a [u8], UsbError> {
        Err(UsbError::unsupported())
    }

    fn handle_setup<'a>(
        &mut self,
        setup: UsbSetupPacket,
        data: &'a mut [u8],
    ) -> Result<&'a [u8], UsbError> {
        let length =
            RP2350_USB_RUNTIME.with_mut(|runtime| {
                match rp2350_usb_prepare_setup_response(runtime, setup, data) {
                    Ok(SetupResponse::DataIn(length)) => Ok(length),
                    Ok(SetupResponse::AckIn) => Ok(0),
                    Ok(SetupResponse::Stall) => Err(UsbError::stall()),
                    Err(error) => Err(error),
                }
            })??;
        Ok(&data[..length])
    }
}

pub fn service_runtime_irq(irqn: i16) -> Result<bool, UsbError> {
    let usb_irqn = RP2350_USBCTRL_IRQN as i16;
    let usb_exception_number = usb_irqn + CORTEX_M_EXTERNAL_EXCEPTION_BASE;
    if irqn != usb_irqn && irqn != usb_exception_number {
        return Ok(false);
    }

    if RP2350_USB_RUNTIME.state.load(Ordering::Acquire) != RP2350_USB_RUNTIME_READY {
        return Ok(false);
    }

    if rp2350_usb_read_reg(USB_INTS_OFFSET) == 0 {
        return Ok(false);
    }

    interrupt::free(|_| rp2350_usb_service_irq())?;
    Ok(true)
}

fn rp2350_usb_service_pending_best_effort() {
    if RP2350_USB_RUNTIME.state.load(Ordering::Acquire) != RP2350_USB_RUNTIME_READY {
        return;
    }
    if rp2350_usb_read_reg(USB_INTS_OFFSET) == 0 {
        return;
    }
    let _ = interrupt::free(|_| rp2350_usb_service_irq());
}

fn rp2350_usb_initialize_controller() -> Result<Rp2350UsbRuntime, UsbError> {
    ensure_boot_clocks_initialized().map_err(|_| UsbError::unsupported())?;
    rp2350_usb_reset_block()?;

    unsafe {
        ptr::write_bytes(
            RP2350_USBCTRL_DPRAM_BASE as *mut u8,
            0,
            RP2350_USBCTRL_DPRAM_BYTES,
        )
    };

    let _ = crate::pal::soc::cortex_m::hal::soc::rp2350::irq_disable(RP2350_USBCTRL_IRQN);
    let _ = crate::pal::soc::cortex_m::hal::soc::rp2350::irq_clear_pending(RP2350_USBCTRL_IRQN);

    rp2350_usb_write_reg(
        USB_USB_MUXING_OFFSET,
        USB_USB_MUXING_TO_PHY_BITS | USB_USB_MUXING_SOFTCON_BITS,
    );
    rp2350_usb_write_reg(
        USB_USB_PWR_OFFSET,
        USB_USB_PWR_VBUS_DETECT_BITS | USB_USB_PWR_VBUS_DETECT_OVERRIDE_EN_BITS,
    );
    rp2350_usb_write_reg(USB_MAIN_CTRL_OFFSET, USB_MAIN_CTRL_CONTROLLER_EN_BITS);
    rp2350_usb_write_reg(USB_SIE_CTRL_OFFSET, USB_SIE_CTRL_EP0_INT_1BUF_BITS);
    rp2350_usb_write_reg(
        USB_INTE_OFFSET,
        USB_INTE_BUFF_STATUS_BITS | USB_INTE_BUS_RESET_BITS | USB_INTE_SETUP_REQ_BITS,
    );

    let mut runtime = Rp2350UsbRuntime::new();
    rp2350_usb_reset_ep0(&mut runtime);
    rp2350_usb_write_reg(
        USB_SIE_STATUS_OFFSET,
        USB_SIE_STATUS_BUS_RESET_BITS
            | USB_SIE_STATUS_SETUP_REC_BITS
            | USB_SIE_STATUS_SUSPENDED_BITS,
    );
    rp2350_usb_write_reg(USB_BUFF_STATUS_OFFSET, u32::MAX);
    Ok(runtime)
}

fn rp2350_usb_activate_controller() -> Result<(), UsbError> {
    rp2350_usb_write_reg(
        USB_SIE_CTRL_OFFSET,
        USB_SIE_CTRL_EP0_INT_1BUF_BITS | USB_SIE_CTRL_PULLUP_EN_BITS,
    );
    Ok(())
}

fn rp2350_usb_reset_block() -> Result<(), UsbError> {
    let reset = (RP2350_RESETS_BASE + RP2350_RESETS_RESET_OFFSET) as *mut u32;
    let reset_done = (RP2350_RESETS_BASE + RP2350_RESETS_RESET_DONE_OFFSET) as *const u32;
    unsafe {
        let current = ptr::read_volatile(reset);
        ptr::write_volatile(reset, current | RP2350_RESETS_RESET_USBCTRL_BITS);
        let current = ptr::read_volatile(reset);
        ptr::write_volatile(reset, current & !RP2350_RESETS_RESET_USBCTRL_BITS);
    }

    for _ in 0..4096 {
        let done = unsafe { ptr::read_volatile(reset_done) };
        if done & RP2350_RESETS_RESET_USBCTRL_BITS != 0 {
            return Ok(());
        }
        core::hint::spin_loop();
    }

    Err(UsbError::busy())
}

fn rp2350_usb_service_irq() -> Result<(), UsbError> {
    let ints = rp2350_usb_read_reg(USB_INTS_OFFSET);

    let runtime = unsafe { &mut *(*RP2350_USB_RUNTIME.value.get()).as_mut_ptr() };

    if (ints & USB_INTE_SETUP_REQ_BITS) != 0 {
        rp2350_usb_write_reg(USB_SIE_STATUS_OFFSET, USB_SIE_STATUS_SETUP_REC_BITS);
        rp2350_usb_handle_setup_irq(runtime)?;
    }

    if (ints & USB_INTE_BUFF_STATUS_BITS) != 0 {
        rp2350_usb_handle_buffer_status(runtime);
    }

    if (ints & USB_INTE_BUS_RESET_BITS) != 0 {
        rp2350_usb_write_reg(USB_SIE_STATUS_OFFSET, USB_SIE_STATUS_BUS_RESET_BITS);
        rp2350_usb_handle_bus_reset(runtime);
    }

    Ok(())
}

fn rp2350_usb_handle_bus_reset(runtime: &mut Rp2350UsbRuntime) {
    runtime.reset_for_bus();
    rp2350_usb_write_reg(USB_ADDR_ENDP_OFFSET, 0);
    rp2350_usb_write_reg(USB_BUFF_STATUS_OFFSET, u32::MAX);
    rp2350_usb_reset_ep0(runtime);
}

fn rp2350_usb_handle_setup_irq(runtime: &mut Rp2350UsbRuntime) -> Result<(), UsbError> {
    let setup = rp2350_usb_read_setup_packet();
    rp2350_usb_reset_ep0(runtime);

    let mut response = [0_u8; RP2350_USB_EP0_MAX_PACKET_SIZE];
    match rp2350_usb_prepare_setup_response(runtime, setup, &mut response)? {
        SetupResponse::DataIn(length) => {
            rp2350_usb_start_ep0_in(runtime, &response[..length], true);
            Ok(())
        }
        SetupResponse::AckIn => {
            rp2350_usb_start_ep0_in(runtime, &[], false);
            Ok(())
        }
        SetupResponse::Stall => {
            rp2350_usb_stall_ep0();
            Ok(())
        }
    }
}

fn rp2350_usb_prepare_setup_response(
    runtime: &mut Rp2350UsbRuntime,
    setup: UsbSetupPacket,
    buffer: &mut [u8],
) -> Result<SetupResponse, UsbError> {
    if !matches!(setup.kind, UsbRequestKind::Standard) {
        return Ok(SetupResponse::Stall);
    }

    match (setup.direction, setup.request) {
        (UsbDirection::In, USB_REQUEST_GET_DESCRIPTOR) => {
            let descriptor_type = UsbDescriptorType::from_u8((setup.value >> 8) as u8);
            let descriptor_index = (setup.value & 0xff) as u8;
            match descriptor_type {
                UsbDescriptorType::Device => {
                    let len = min(
                        RP2350_USB_DEVICE_DESCRIPTOR_BYTES.len(),
                        setup.length as usize,
                    );
                    if buffer.len() < len {
                        return Err(UsbError::resource_exhausted());
                    }
                    buffer[..len].copy_from_slice(&RP2350_USB_DEVICE_DESCRIPTOR_BYTES[..len]);
                    Ok(SetupResponse::DataIn(len))
                }
                UsbDescriptorType::Configuration => {
                    let len = min(
                        RP2350_USB_CONFIGURATION_DESCRIPTOR_BYTES.len(),
                        setup.length as usize,
                    );
                    if buffer.len() < len {
                        return Err(UsbError::resource_exhausted());
                    }
                    buffer[..len]
                        .copy_from_slice(&RP2350_USB_CONFIGURATION_DESCRIPTOR_BYTES[..len]);
                    Ok(SetupResponse::DataIn(len))
                }
                UsbDescriptorType::String => {
                    let len =
                        rp2350_usb_write_string_descriptor(descriptor_index, setup.index, buffer)?;
                    Ok(SetupResponse::DataIn(min(len, setup.length as usize)))
                }
                _ => Ok(SetupResponse::Stall),
            }
        }
        (UsbDirection::In, USB_REQUEST_GET_STATUS) => {
            if buffer.len() < 2 {
                return Err(UsbError::resource_exhausted());
            }
            buffer[0] = 0;
            buffer[1] = 0;
            Ok(SetupResponse::DataIn(min(2, setup.length as usize)))
        }
        (UsbDirection::In, USB_REQUEST_GET_CONFIGURATION) => {
            if buffer.is_empty() {
                return Err(UsbError::resource_exhausted());
            }
            buffer[0] = runtime.active_configuration;
            Ok(SetupResponse::DataIn(min(1, setup.length as usize)))
        }
        (UsbDirection::In, USB_REQUEST_GET_INTERFACE) => {
            if buffer.is_empty() {
                return Err(UsbError::resource_exhausted());
            }
            buffer[0] = 0;
            Ok(SetupResponse::DataIn(min(1, setup.length as usize)))
        }
        (UsbDirection::Out, USB_REQUEST_SET_ADDRESS) => {
            runtime.pending_address = Some((setup.value & 0x7f) as u8);
            Ok(SetupResponse::AckIn)
        }
        (UsbDirection::Out, USB_REQUEST_SET_CONFIGURATION) => {
            let configuration = (setup.value & 0xff) as u8;
            if configuration > 1 {
                return Ok(SetupResponse::Stall);
            }
            runtime.active_configuration = configuration;
            Ok(SetupResponse::AckIn)
        }
        (UsbDirection::Out, USB_REQUEST_CLEAR_FEATURE)
        | (UsbDirection::Out, USB_REQUEST_SET_FEATURE)
        | (UsbDirection::Out, USB_REQUEST_SET_INTERFACE) => Ok(SetupResponse::AckIn),
        _ => Ok(SetupResponse::Stall),
    }
}

fn rp2350_usb_write_string_descriptor(
    descriptor_index: u8,
    language_id: u16,
    buffer: &mut [u8],
) -> Result<usize, UsbError> {
    if descriptor_index == USB_STRING_LANGUAGES_INDEX {
        if buffer.len() < 4 {
            return Err(UsbError::resource_exhausted());
        }
        buffer[0] = 4;
        buffer[1] = 0x03;
        buffer[2] = (USB_LANGUAGE_EN_US & 0xff) as u8;
        buffer[3] = (USB_LANGUAGE_EN_US >> 8) as u8;
        return Ok(4);
    }

    if language_id != 0 && language_id != USB_LANGUAGE_EN_US {
        return Ok(0);
    }

    let text = match descriptor_index {
        USB_STRING_MANUFACTURER_INDEX => RP2350_USB_MANUFACTURER_STRING,
        USB_STRING_PRODUCT_INDEX => RP2350_USB_PRODUCT_STRING,
        USB_STRING_SERIAL_INDEX => RP2350_USB_SERIAL_STRING,
        USB_STRING_INTERFACE_INDEX => RP2350_USB_INTERFACE_STRING,
        _ => return Ok(0),
    };

    let descriptor_len = 2 + (text.len() * 2);
    if buffer.len() < descriptor_len {
        return Err(UsbError::resource_exhausted());
    }

    buffer[0] = descriptor_len as u8;
    buffer[1] = 0x03;
    for (index, byte) in text.iter().copied().enumerate() {
        buffer[2 + (index * 2)] = byte;
        buffer[3 + (index * 2)] = 0;
    }

    Ok(descriptor_len)
}

fn rp2350_usb_handle_buffer_status(runtime: &mut Rp2350UsbRuntime) {
    let status = rp2350_usb_read_reg(USB_BUFF_STATUS_OFFSET);
    if (status & USB_BUFF_STATUS_EP0_IN_BITS) != 0 {
        rp2350_usb_write_reg(USB_BUFF_STATUS_OFFSET, USB_BUFF_STATUS_EP0_IN_BITS);
        rp2350_usb_handle_ep0_in_complete(runtime);
    }
    if (status & USB_BUFF_STATUS_EP0_OUT_BITS) != 0 {
        rp2350_usb_write_reg(USB_BUFF_STATUS_OFFSET, USB_BUFF_STATUS_EP0_OUT_BITS);
        rp2350_usb_handle_ep0_out_complete(runtime);
    }

    let unexpected = status & !(USB_BUFF_STATUS_EP0_IN_BITS | USB_BUFF_STATUS_EP0_OUT_BITS);
    if unexpected != 0 {
        rp2350_usb_write_reg(USB_BUFF_STATUS_OFFSET, unexpected);
    }
}

fn rp2350_usb_handle_ep0_in_complete(runtime: &mut Rp2350UsbRuntime) {
    if let Some(address) = runtime.pending_address.take() {
        runtime.device_address = address;
        rp2350_usb_write_reg(USB_ADDR_ENDP_OFFSET, u32::from(address));
        return;
    }

    if runtime.expect_status_out {
        rp2350_usb_arm_ep0_out(runtime);
    }
}

fn rp2350_usb_handle_ep0_out_complete(runtime: &mut Rp2350UsbRuntime) {
    runtime.expect_status_out = false;
}

fn rp2350_usb_start_ep0_in(
    runtime: &mut Rp2350UsbRuntime,
    payload: &[u8],
    expect_status_out: bool,
) {
    let len = min(payload.len(), RP2350_USB_EP0_MAX_PACKET_SIZE);
    if len != 0 {
        rp2350_usb_byte_copy_to_dpram(
            unsafe { (*rp2350_usb_dpram()).ep0_buf_a.as_mut_ptr() },
            payload,
        );
    }

    runtime.expect_status_out = expect_status_out;
    let mut value =
        (len as u32 & USB_BUF_CTRL_LEN_MASK) | USB_BUF_CTRL_LAST | USB_BUF_CTRL_RESET_SEL;
    value |= USB_BUF_CTRL_FULL;
    if runtime.ep0_in_data1 {
        value |= USB_BUF_CTRL_DATA1_PID;
    }
    runtime.ep0_in_data1 = !runtime.ep0_in_data1;

    rp2350_usb_write_buf_ctrl(
        unsafe { &mut (*rp2350_usb_dpram()).ep_buf_ctrl[0].in_ },
        value | USB_BUF_CTRL_AVAIL,
    );
}

fn rp2350_usb_arm_ep0_out(runtime: &mut Rp2350UsbRuntime) {
    let mut value = (RP2350_USB_EP0_MAX_PACKET_SIZE as u32 & USB_BUF_CTRL_LEN_MASK)
        | USB_BUF_CTRL_LAST
        | USB_BUF_CTRL_RESET_SEL;
    if runtime.ep0_out_data1 {
        value |= USB_BUF_CTRL_DATA1_PID;
    }
    runtime.ep0_out_data1 = !runtime.ep0_out_data1;
    rp2350_usb_write_buf_ctrl(
        unsafe { &mut (*rp2350_usb_dpram()).ep_buf_ctrl[0].out },
        value | USB_BUF_CTRL_AVAIL,
    );
}

fn rp2350_usb_stall_ep0() {
    rp2350_usb_write_reg(
        USB_EP_STALL_ARM_OFFSET,
        USB_EP_STALL_ARM_EP0_IN_BITS | USB_EP_STALL_ARM_EP0_OUT_BITS,
    );
    unsafe {
        (*rp2350_usb_dpram()).ep_buf_ctrl[0].in_ = USB_BUF_CTRL_STALL;
        (*rp2350_usb_dpram()).ep_buf_ctrl[0].out = USB_BUF_CTRL_STALL;
    }
}

fn rp2350_usb_reset_ep0(runtime: &mut Rp2350UsbRuntime) {
    runtime.ep0_in_data1 = true;
    runtime.ep0_out_data1 = true;
    runtime.expect_status_out = false;
    unsafe {
        (*rp2350_usb_dpram()).ep_buf_ctrl[0].in_ = USB_BUF_CTRL_DATA1_PID | USB_BUF_CTRL_RESET_SEL;
        (*rp2350_usb_dpram()).ep_buf_ctrl[0].out = USB_BUF_CTRL_DATA1_PID | USB_BUF_CTRL_RESET_SEL;
    }
}

fn rp2350_usb_device_state() -> Result<UsbDeviceState, UsbError> {
    RP2350_USB_RUNTIME.with(|runtime| {
        let sie_status = rp2350_usb_read_reg(USB_SIE_STATUS_OFFSET);
        if runtime.active_configuration != 0 {
            return UsbDeviceState::Configured;
        }

        if runtime.device_address != 0 {
            return UsbDeviceState::Addressed;
        }

        if runtime.bus_reset_seen {
            if (sie_status & USB_SIE_STATUS_SUSPENDED_BITS) != 0 {
                return UsbDeviceState::Suspended;
            }
            return UsbDeviceState::Default;
        }

        if (sie_status & USB_SIE_STATUS_CONNECTED_BITS) != 0 {
            UsbDeviceState::Powered
        } else {
            UsbDeviceState::Powered
        }
    })
}

fn rp2350_usb_connected() -> bool {
    if RP2350_USB_RUNTIME.state.load(Ordering::Acquire) != RP2350_USB_RUNTIME_READY {
        return false;
    }
    (rp2350_usb_read_reg(USB_SIE_STATUS_OFFSET) & USB_SIE_STATUS_CONNECTED_BITS) != 0
}

fn rp2350_usb_parse_setup_packet(bytes: [u8; 8]) -> UsbSetupPacket {
    let bm_request_type = bytes[0];
    UsbSetupPacket {
        direction: if (bm_request_type & 0x80) != 0 {
            UsbDirection::In
        } else {
            UsbDirection::Out
        },
        kind: match (bm_request_type >> 5) & 0x03 {
            0 => UsbRequestKind::Standard,
            1 => UsbRequestKind::Class,
            2 => UsbRequestKind::Vendor,
            _ => UsbRequestKind::Reserved,
        },
        recipient: match bm_request_type & 0x1f {
            0 => UsbRequestRecipient::Device,
            1 => UsbRequestRecipient::Interface,
            2 => UsbRequestRecipient::Endpoint,
            _ => UsbRequestRecipient::Other,
        },
        request: bytes[1],
        value: u16::from_le_bytes([bytes[2], bytes[3]]),
        index: u16::from_le_bytes([bytes[4], bytes[5]]),
        length: u16::from_le_bytes([bytes[6], bytes[7]]),
    }
}

fn rp2350_usb_dpram() -> *mut Rp2350UsbDeviceDpram {
    RP2350_USBCTRL_DPRAM_BASE as *mut Rp2350UsbDeviceDpram
}

fn rp2350_usb_byte_copy_to_dpram(dst: *mut u8, src: &[u8]) {
    for (index, byte) in src.iter().copied().enumerate() {
        unsafe { ptr::write_volatile(dst.add(index), byte) };
    }
}

fn rp2350_usb_read_setup_packet() -> UsbSetupPacket {
    let setup = unsafe { &(*rp2350_usb_dpram()).setup_packet };
    let mut bytes = [0_u8; 8];
    for (index, slot) in bytes.iter_mut().enumerate() {
        *slot = unsafe { ptr::read_volatile(setup.as_ptr().add(index)) };
    }
    rp2350_usb_parse_setup_packet(bytes)
}

fn rp2350_usb_write_buf_ctrl(register: &mut u32, value: u32) {
    let staged = value & !USB_BUF_CTRL_AVAIL;
    unsafe { ptr::write_volatile(register, staged) };
    if (value & USB_BUF_CTRL_AVAIL) != 0 {
        cortex_m::asm::delay(12);
        unsafe { ptr::write_volatile(register, value) };
    }
}

fn rp2350_usb_read_reg(offset: usize) -> u32 {
    unsafe { ptr::read_volatile((RP2350_USBCTRL_REGS_BASE + offset) as *const u32) }
}

fn rp2350_usb_write_reg(offset: usize, value: u32) {
    unsafe { ptr::write_volatile((RP2350_USBCTRL_REGS_BASE + offset) as *mut u32, value) };
}
