#![no_std]
#![no_main]

//! Pico 2 W CYW43439 Bluetooth shared-bus smoke test with seven-segment status output.
//!
//! Board layout:
//! - `GP11` -> panic/fault LED (standalone red LED)
//! - `GP12` -> display serial data
//! - `GP13` -> display output enable
//! - `GP14` -> display latch
//! - `GP15` -> display shift clock
//! - `GP23` -> CYW43439 `WL_REG_ON`
//! - `GP24` -> CYW43439 shared data / host-wake line
//! - `GP25` -> CYW43439 chip select
//! - `GP29` -> CYW43439 shared clock
//!
//! Display shift protocol:
//! 1. shift digit byte first
//! 2. shift segment byte second
//! 3. pulse latch to update both banks together
//!
//! Status codes:
//! - `C100` startup
//! - `C110` display path alive
//! - `C220` Bluetooth driver bound
//! - `C230` Bluetooth power-on request succeeded
//! - `C240` HCI reset command sent
//! - `C24F` HCI reset completed
//! - `C250` HCI local-version query sent
//! - `C25F` HCI local-version query completed
//! - `C260` HCI BD_ADDR query sent
//! - `C26F` HCI BD_ADDR query completed
//! - `C270` HCI supported-commands query sent
//! - `C27F` HCI supported-commands query completed
//! - `C280` HCI supported-features query sent
//! - `C28F` HCI supported-features query completed
//! - `C290` LE supported-features query sent
//! - `C29F` LE supported-features query completed
//! - `C2A0` HCI buffer-size query sent
//! - `C2AF` HCI buffer-size query completed
//! - `C2B0` LE buffer-size query sent
//! - `C2BF` LE buffer-size query completed
//! - `C2C0` LE advertising-parameters command sent
//! - `C2CF` LE advertising-parameters command completed
//! - `C2D0` LE advertising-data command sent
//! - `C2DF` LE advertising-data command completed
//! - `C2E0` LE scan-response command sent
//! - `C2EF` LE scan-response command completed
//! - `C2F0` LE advertising-enable command sent
//! - `C2FF` LE advertising-enable command completed
//! - `C30F` legacy LE advertising enabled
//! - `E2xx` failure stage

use core::mem::MaybeUninit;
use core::panic::PanicInfo;
use core::pin::Pin;
use core::sync::atomic::{
    AtomicU16,
    AtomicU8,
    AtomicU32,
    Ordering,
};

use cortex_m_rt::{
    ExceptionFrame,
    entry,
    exception,
};
use fusion_example_rp2350_on_device::gpio::Rp2350FiberGpioService;
use fusion_example_rp2350_on_device::runtime::wait_for_runtime_progress;
use fusion_example_rp2350_on_device::seven_segment::{
    Rp2350FiberFourDigitSevenSegmentDisplay,
    Rp2350FiberFourDigitSevenSegmentDisplayService,
};
use fusion_example_rp2350_on_device::shift_register_74hc595::Rp2350FiberShiftRegister74hc595Service;
use fusion_firmware::module::StackDriverStorage;
use fusion_firmware::sys::hal::drivers::bus::gpio::{
    SystemGpioPin,
    system_gpio,
};
use fusion_firmware::sys::hal::drivers::net::chipset::infineon::cyw43439::system_bluetooth;
use fusion_hal::contract::drivers::bus::gpio::{
    GpioControlContract,
    GpioDriveStrength,
};
use fusion_hal::contract::drivers::net::bluetooth::{
    BLUETOOTH_HCI_EVENT_COMMAND_COMPLETE,
    BLUETOOTH_HCI_OPCODE_READ_BD_ADDR,
    BLUETOOTH_HCI_OPCODE_READ_LOCAL_VERSION_INFORMATION,
    BLUETOOTH_HCI_OPCODE_READ_LOCAL_SUPPORTED_COMMANDS,
    BLUETOOTH_HCI_OPCODE_READ_LOCAL_SUPPORTED_FEATURES,
    BLUETOOTH_HCI_OPCODE_READ_BUFFER_SIZE,
    BLUETOOTH_HCI_OPCODE_LE_SET_ADVERTISING_DATA,
    BLUETOOTH_HCI_OPCODE_LE_SET_ADVERTISING_ENABLE,
    BLUETOOTH_HCI_OPCODE_LE_SET_ADVERTISING_PARAMETERS,
    BLUETOOTH_HCI_OPCODE_LE_SET_SCAN_RESPONSE_DATA,
    BLUETOOTH_HCI_OPCODE_LE_READ_BUFFER_SIZE,
    BLUETOOTH_HCI_OPCODE_RESET,
    BLUETOOTH_HCI_OPCODE_LE_READ_LOCAL_SUPPORTED_FEATURES,
    BluetoothCanonicalFrame,
    BluetoothHciCommandComplete,
    BluetoothHciLeAdvertisingChannelMap,
    BluetoothHciLeAdvertisingData,
    BluetoothHciLeAdvertisingFilterPolicy,
    BluetoothHciLeAdvertisingParameters,
    BluetoothHciLeAdvertisingType,
    BluetoothHciLeOwnAddressType,
    BluetoothHciLePeerAddressType,
    BluetoothErrorKind,
    BluetoothAddress,
    BluetoothAddressKind,
    BluetoothHciCommandFrame,
    BluetoothHciCommandHeader,
    BluetoothHciFrame,
    BluetoothHciFrameView,
    BluetoothHciPacketType,
};
use fusion_hal::drivers::peripheral::SevenSegmentPolarity;

fusion_example_rp2350_on_device::fusion_rp2350_export_build_id!();

const DISPLAY_DATA_PIN: u8 = 12;
const DISPLAY_ENABLE_PIN: u8 = 13;
const DISPLAY_LATCH_PIN: u8 = 14;
const DISPLAY_SHIFT_CLOCK_PIN: u8 = 15;
const PANIC_LED_PIN: u8 = 11;

const GPIO_SERVICE_STACK_BYTES: usize = 4096;
const SHIFT_REGISTER_SERVICE_STACK_BYTES: usize = 4096;
const DISPLAY_SERVICE_STACK_BYTES: usize = 4096;
const BLUETOOTH_DRIVER_STORAGE_WORDS: usize = 256;

const STATUS_STARTUP: u16 = 0xC100;
const STATUS_DISPLAY_READY: u16 = 0xC110;
const STATUS_BLUETOOTH_BOUND: u16 = 0xC220;
const STATUS_BLUETOOTH_POWERED: u16 = 0xC230;
const STATUS_BLUETOOTH_HCI_RESET_SENT: u16 = 0xC240;
const STATUS_BLUETOOTH_HCI_RESET_COMPLETE: u16 = 0xC24F;
const STATUS_BLUETOOTH_HCI_VERSION_SENT: u16 = 0xC250;
const STATUS_BLUETOOTH_HCI_VERSION_COMPLETE: u16 = 0xC25F;
const STATUS_BLUETOOTH_HCI_BD_ADDR_SENT: u16 = 0xC260;
const STATUS_BLUETOOTH_HCI_BD_ADDR_COMPLETE: u16 = 0xC26F;
const STATUS_BLUETOOTH_HCI_COMMANDS_SENT: u16 = 0xC270;
const STATUS_BLUETOOTH_HCI_COMMANDS_COMPLETE: u16 = 0xC27F;
const STATUS_BLUETOOTH_HCI_FEATURES_SENT: u16 = 0xC280;
const STATUS_BLUETOOTH_HCI_FEATURES_COMPLETE: u16 = 0xC28F;
const STATUS_BLUETOOTH_HCI_LE_FEATURES_SENT: u16 = 0xC290;
const STATUS_BLUETOOTH_HCI_LE_FEATURES_COMPLETE: u16 = 0xC29F;
const STATUS_BLUETOOTH_HCI_BUFFER_SIZE_SENT: u16 = 0xC2A0;
const STATUS_BLUETOOTH_HCI_BUFFER_SIZE_COMPLETE: u16 = 0xC2AF;
const STATUS_BLUETOOTH_HCI_LE_BUFFER_SIZE_SENT: u16 = 0xC2B0;
const STATUS_BLUETOOTH_HCI_LE_BUFFER_SIZE_COMPLETE: u16 = 0xC2BF;
const STATUS_BLUETOOTH_HCI_ADV_PARAMS_SENT: u16 = 0xC2C0;
const STATUS_BLUETOOTH_HCI_ADV_PARAMS_COMPLETE: u16 = 0xC2CF;
const STATUS_BLUETOOTH_HCI_ADV_DATA_SENT: u16 = 0xC2D0;
const STATUS_BLUETOOTH_HCI_ADV_DATA_COMPLETE: u16 = 0xC2DF;
const STATUS_BLUETOOTH_HCI_SCAN_RESPONSE_SENT: u16 = 0xC2E0;
const STATUS_BLUETOOTH_HCI_SCAN_RESPONSE_COMPLETE: u16 = 0xC2EF;
const STATUS_BLUETOOTH_HCI_ADV_ENABLE_SENT: u16 = 0xC2F0;
const STATUS_BLUETOOTH_HCI_ADV_ENABLE_COMPLETE: u16 = 0xC2FF;
const STATUS_BLUETOOTH_HCI_ADVERTISING_ENABLED: u16 = 0xC30F;
const STATUS_ERROR_BLUETOOTH_BIND: u16 = 0xE220;
const STATUS_ERROR_BLUETOOTH_POWER: u16 = 0xE230;
const STATUS_ERROR_BLUETOOTH_HCI_SEND: u16 = 0xE240;
const STATUS_ERROR_BLUETOOTH_HCI_WAIT: u16 = 0xE24F;
const STATUS_ERROR_BLUETOOTH_HCI_VERSION: u16 = 0xE25F;
const STATUS_ERROR_BLUETOOTH_HCI_BD_ADDR: u16 = 0xE26F;
const STATUS_ERROR_BLUETOOTH_HCI_COMMANDS: u16 = 0xE27F;
const STATUS_ERROR_BLUETOOTH_HCI_FEATURES: u16 = 0xE28F;
const STATUS_ERROR_BLUETOOTH_HCI_LE_FEATURES: u16 = 0xE29F;
const STATUS_ERROR_BLUETOOTH_HCI_BUFFER_SIZE: u16 = 0xE2AF;
const STATUS_ERROR_BLUETOOTH_HCI_LE_BUFFER_SIZE: u16 = 0xE2BF;
const STATUS_ERROR_BLUETOOTH_HCI_ADV_PARAMS: u16 = 0xE2CF;
const STATUS_ERROR_BLUETOOTH_HCI_ADV_DATA: u16 = 0xE2DF;
const STATUS_ERROR_BLUETOOTH_HCI_SCAN_RESPONSE: u16 = 0xE2EF;
const STATUS_ERROR_BLUETOOTH_HCI_ADV_ENABLE: u16 = 0xE2FF;

const PANIC_LED_UNINITIALIZED: u8 = 0;
const PANIC_LED_READY: u8 = 1;
const PANIC_LED_FAILED: u8 = 2;

type PicoGpioService = Rp2350FiberGpioService<4>;
type PicoShiftRegisterService = Rp2350FiberShiftRegister74hc595Service<2>;
type PicoDisplayService = Rp2350FiberFourDigitSevenSegmentDisplayService;

static mut GPIO_SERVICE_STORAGE: MaybeUninit<PicoGpioService> = MaybeUninit::uninit();
static mut SHIFT_REGISTER_SERVICE_STORAGE: MaybeUninit<PicoShiftRegisterService> =
    MaybeUninit::uninit();
static mut DISPLAY_SERVICE_STORAGE: MaybeUninit<PicoDisplayService> = MaybeUninit::uninit();
static mut PANIC_LED_STORAGE: MaybeUninit<SystemGpioPin> = MaybeUninit::uninit();
static PANIC_LED_STATE: AtomicU8 = AtomicU8::new(PANIC_LED_UNINITIALIZED);
#[unsafe(no_mangle)]
pub static DEBUG_CHANNEL_PHASE: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static DEBUG_CHANNEL_LAST_STATUS: AtomicU16 = AtomicU16::new(0);
#[unsafe(no_mangle)]
pub static DEBUG_CHANNEL_LAST_ERROR_KIND: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static DEBUG_CHANNEL_LAST_BLUETOOTH_EVENT: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static DEBUG_CHANNEL_LAST_BLUETOOTH_OPCODE: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static DEBUG_CHANNEL_LAST_BLUETOOTH_STATUS: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static DEBUG_CHANNEL_LAST_BLUETOOTH_HCI_VERSION: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static DEBUG_CHANNEL_LAST_BLUETOOTH_HCI_REVISION: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static DEBUG_CHANNEL_LAST_BLUETOOTH_MANUFACTURER: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static DEBUG_CHANNEL_LAST_BLUETOOTH_SUBVERSION: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static DEBUG_CHANNEL_LAST_BLUETOOTH_ADDRESS_LOW: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static DEBUG_CHANNEL_LAST_BLUETOOTH_ADDRESS_HIGH: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static DEBUG_CHANNEL_LAST_BLUETOOTH_SUPPORTED_COMMANDS: [AtomicU32; 16] =
    [const { AtomicU32::new(0) }; 16];
#[unsafe(no_mangle)]
pub static DEBUG_CHANNEL_LAST_BLUETOOTH_FEATURES_LOW: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static DEBUG_CHANNEL_LAST_BLUETOOTH_FEATURES_HIGH: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static DEBUG_CHANNEL_LAST_BLUETOOTH_LE_FEATURES_LOW: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static DEBUG_CHANNEL_LAST_BLUETOOTH_LE_FEATURES_HIGH: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static DEBUG_CHANNEL_LAST_BLUETOOTH_ACL_MAX_DATA_LENGTH: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static DEBUG_CHANNEL_LAST_BLUETOOTH_SCO_MAX_DATA_LENGTH: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static DEBUG_CHANNEL_LAST_BLUETOOTH_ACL_MAX_PACKET_COUNT: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static DEBUG_CHANNEL_LAST_BLUETOOTH_SCO_MAX_PACKET_COUNT: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static DEBUG_CHANNEL_LAST_BLUETOOTH_LE_ACL_MAX_DATA_LENGTH: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static DEBUG_CHANNEL_LAST_BLUETOOTH_LE_ACL_MAX_PACKET_COUNT: AtomicU32 = AtomicU32::new(0);

fn panic_led_on() -> ! {
    let _ = set_panic_led(true);
    loop {
        core::hint::spin_loop();
    }
}

fn panic_led_pin() -> Result<&'static mut SystemGpioPin, ()> {
    match PANIC_LED_STATE.load(Ordering::Acquire) {
        PANIC_LED_READY => unsafe {
            Ok((&mut *core::ptr::addr_of_mut!(PANIC_LED_STORAGE)).assume_init_mut())
        },
        PANIC_LED_FAILED => Err(()),
        _ => {
            let gpio = system_gpio().map_err(|_| ())?;
            let mut pin = gpio.take_pin(PANIC_LED_PIN).map_err(|_| ())?;
            pin.set_drive_strength(GpioDriveStrength::MilliAmps4)
                .map_err(|_| ())?;
            pin.configure_output(false).map_err(|_| ())?;
            unsafe {
                core::ptr::addr_of_mut!(PANIC_LED_STORAGE).write(MaybeUninit::new(pin));
                PANIC_LED_STATE.store(PANIC_LED_READY, Ordering::Release);
                Ok((&mut *core::ptr::addr_of_mut!(PANIC_LED_STORAGE)).assume_init_mut())
            }
        }
    }
}

fn set_panic_led(high: bool) -> Result<(), ()> {
    match panic_led_pin() {
        Ok(pin) => pin.set_level(high).map_err(|_| ()),
        Err(()) => {
            PANIC_LED_STATE.store(PANIC_LED_FAILED, Ordering::Release);
            Err(())
        }
    }
}

fn init_gpio_service() -> *mut PicoGpioService {
    unsafe {
        let service = core::ptr::addr_of_mut!(GPIO_SERVICE_STORAGE).cast::<PicoGpioService>();
        service.write(PicoGpioService::new().expect("gpio service should build"));
        service
    }
}

fn init_shift_register_service()
-> fusion_example_rp2350_on_device::shift_register_74hc595::Rp2350FiberShiftRegister74hc595<2> {
    let gpio_service = init_gpio_service();
    let data = unsafe {
        Pin::new_unchecked(&mut *gpio_service)
            .claim_output_pin(DISPLAY_DATA_PIN, GpioDriveStrength::MilliAmps4)
            .expect("display data pin should be claimable")
    };
    let shift_clock = unsafe {
        Pin::new_unchecked(&mut *gpio_service)
            .claim_output_pin(DISPLAY_SHIFT_CLOCK_PIN, GpioDriveStrength::MilliAmps4)
            .expect("display shift clock pin should be claimable")
    };
    let latch_clock = unsafe {
        Pin::new_unchecked(&mut *gpio_service)
            .claim_output_pin(DISPLAY_LATCH_PIN, GpioDriveStrength::MilliAmps4)
            .expect("display latch pin should be claimable")
    };
    let output_enable = unsafe {
        Pin::new_unchecked(&mut *gpio_service)
            .claim_output_pin(DISPLAY_ENABLE_PIN, GpioDriveStrength::MilliAmps4)
            .expect("display output-enable pin should be claimable")
    };
    unsafe {
        Pin::new_unchecked(&mut *gpio_service)
            .spawn::<GPIO_SERVICE_STACK_BYTES>()
            .expect("display gpio service should spawn");
    }

    let shift_service_ptr = unsafe {
        let service = core::ptr::addr_of_mut!(SHIFT_REGISTER_SERVICE_STORAGE)
            .cast::<PicoShiftRegisterService>();
        let shift_service =
            PicoShiftRegisterService::new(data, shift_clock, latch_clock, output_enable)
                .expect("display shift-register service should build");
        service.write(shift_service);
        service
    };
    let shift_register = unsafe { (&*shift_service_ptr).client_handle() };
    unsafe {
        Pin::new_unchecked(&mut *shift_service_ptr)
            .spawn::<SHIFT_REGISTER_SERVICE_STACK_BYTES>()
            .expect("display shift-register service should spawn");
    }
    shift_register
}

fn init_display_service(
    shift_register: fusion_example_rp2350_on_device::shift_register_74hc595::Rp2350FiberShiftRegister74hc595<2>,
) -> Rp2350FiberFourDigitSevenSegmentDisplay {
    let display_service_ptr = unsafe {
        let service = core::ptr::addr_of_mut!(DISPLAY_SERVICE_STORAGE).cast::<PicoDisplayService>();
        let display_service =
            PicoDisplayService::new(shift_register, SevenSegmentPolarity::common_cathode())
                .expect("display service should build");
        service.write(display_service);
        service
    };
    let display = unsafe { (&*display_service_ptr).client_handle() };
    unsafe {
        Pin::new_unchecked(&mut *display_service_ptr)
            .spawn::<DISPLAY_SERVICE_STACK_BYTES>()
            .expect("display service should spawn");
    }
    display
}

fn set_status(display: &Rp2350FiberFourDigitSevenSegmentDisplay, code: u16) {
    DEBUG_CHANNEL_LAST_STATUS.store(code, Ordering::Release);
    let _ = display.set_hex(code);
}

fn pump_runtime(turns: usize) {
    for _ in 0..turns {
        wait_for_runtime_progress();
    }
}

fn fatal_status(display: &Rp2350FiberFourDigitSevenSegmentDisplay, code: u16) -> ! {
    let _ = set_panic_led(true);
    set_status(display, code);
    loop {
        wait_for_runtime_progress();
    }
}

fn record_bluetooth_error(error: fusion_hal::contract::drivers::net::bluetooth::BluetoothError) {
    let encoded = match error.kind() {
        BluetoothErrorKind::Unsupported => 1,
        BluetoothErrorKind::Invalid => 2,
        BluetoothErrorKind::Busy => 3,
        BluetoothErrorKind::ResourceExhausted => 4,
        BluetoothErrorKind::StateConflict => 5,
        BluetoothErrorKind::Disconnected => 6,
        BluetoothErrorKind::TimedOut => 7,
        BluetoothErrorKind::PermissionDenied => 8,
        BluetoothErrorKind::Platform(code) => 0x8000_0000_u32 | (code as u32),
    };
    DEBUG_CHANNEL_LAST_ERROR_KIND.store(encoded, Ordering::Release);
}

fn record_hci_event_metadata(event_code: u8, opcode: u16, status: Option<u8>) {
    DEBUG_CHANNEL_LAST_BLUETOOTH_EVENT.store(event_code as u32, Ordering::Release);
    DEBUG_CHANNEL_LAST_BLUETOOTH_OPCODE.store(opcode as u32, Ordering::Release);
    DEBUG_CHANNEL_LAST_BLUETOOTH_STATUS.store(status.map_or(u32::MAX, u32::from), Ordering::Release);
}

fn decode_command_complete_from_opaque(bytes: &[u8]) -> Option<BluetoothHciCommandComplete<'_>> {
    if bytes.len() < 5 || bytes[0] != BLUETOOTH_HCI_EVENT_COMMAND_COMPLETE {
        return None;
    }
    let parameter_length = usize::from(bytes[1]);
    if bytes.len() != 2 + parameter_length || parameter_length < 3 {
        return None;
    }
    Some(BluetoothHciCommandComplete {
        num_hci_command_packets: bytes[2],
        opcode: u16::from_le_bytes([bytes[3], bytes[4]]),
        return_parameters: &bytes[5..],
    })
}

fn frame_command_complete(frame: BluetoothCanonicalFrame<'_>) -> Option<BluetoothHciCommandComplete<'_>> {
    match frame {
        BluetoothCanonicalFrame::Hci(BluetoothHciFrameView::Event(event)) => event.as_command_complete(),
        BluetoothCanonicalFrame::Hci(BluetoothHciFrameView::Opaque(BluetoothHciFrame {
            packet_type: BluetoothHciPacketType::Event,
            bytes,
        })) => decode_command_complete_from_opaque(bytes),
        _ => None,
    }
}

fn send_hci_command(
    bluetooth: &mut dyn fusion_hal::contract::drivers::net::bluetooth::BluetoothAdapterContract,
    opcode: u16,
    parameters: &[u8],
    scratch: &mut [u8],
) -> Result<(), fusion_hal::contract::drivers::net::bluetooth::BluetoothError> {
    bluetooth.send_frame(
        BluetoothCanonicalFrame::Hci(BluetoothHciFrameView::Command(BluetoothHciCommandFrame {
            header: BluetoothHciCommandHeader {
                opcode,
                parameter_length: parameters.len() as u8,
            },
            parameters,
        })),
        scratch,
    )
}

fn wait_for_command_complete_status_zero(
    bluetooth: &mut dyn fusion_hal::contract::drivers::net::bluetooth::BluetoothAdapterContract,
    opcode: u16,
    read_buffer: &mut [u8],
) -> Result<(), fusion_hal::contract::drivers::net::bluetooth::BluetoothError> {
    wait_for_command_complete(bluetooth, opcode, read_buffer, |command_complete| {
        command_complete
            .return_parameters
            .first()
            .copied()
            .filter(|status| *status == 0)
            .map(|_| ())
    })
}

fn legacy_advertising_parameters() -> [u8; BluetoothHciLeAdvertisingParameters::ENCODED_LEN] {
    BluetoothHciLeAdvertisingParameters {
        interval_min: 0x00a0,
        interval_max: 0x00a0,
        advertising_type: BluetoothHciLeAdvertisingType::ConnectableUndirected,
        own_address_type: BluetoothHciLeOwnAddressType::PublicDevice,
        peer_address_type: BluetoothHciLePeerAddressType::PublicDevice,
        peer_address: BluetoothAddress {
            bytes: [0; 6],
            kind: BluetoothAddressKind::Public,
        },
        channel_map: BluetoothHciLeAdvertisingChannelMap::ALL,
        filter_policy: BluetoothHciLeAdvertisingFilterPolicy::ProcessAll,
    }
    .encode()
}

fn legacy_advertising_data() -> [u8; BluetoothHciLeAdvertisingData::ENCODED_LEN] {
    let payload = [
        0x02, 0x01, 0x06, // Flags: general discoverable, BR/EDR not supported
        0x0a, 0x09, b'F', b'u', b's', b'i', b'o', b'n', b' ', b'D', b'B',
    ];
    BluetoothHciLeAdvertisingData { bytes: &payload }
        .encode()
        .expect("legacy advertising payload should fit")
}

fn legacy_scan_response_data() -> [u8; BluetoothHciLeAdvertisingData::ENCODED_LEN] {
    let payload = [
        0x09, 0x09, b'P', b'i', b'c', b'o', b'2', b'W', b' ', b'B',
    ];
    BluetoothHciLeAdvertisingData { bytes: &payload }
        .encode()
        .expect("legacy scan-response payload should fit")
}

fn record_supported_commands(commands: [u8; 64]) {
    for (index, chunk) in commands.chunks_exact(4).enumerate() {
        DEBUG_CHANNEL_LAST_BLUETOOTH_SUPPORTED_COMMANDS[index].store(
            u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]),
            Ordering::Release,
        );
    }
}

fn record_feature_set(
    low: &AtomicU32,
    high: &AtomicU32,
    features: fusion_hal::contract::drivers::net::bluetooth::BluetoothHciFeatureSet,
) {
    let raw = features.bytes;
    low.store(
        u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]),
        Ordering::Release,
    );
    high.store(
        u32::from_le_bytes([raw[4], raw[5], raw[6], raw[7]]),
        Ordering::Release,
    );
}

fn record_buffer_sizes(
    acl_max_data_length: u16,
    sco_max_data_length: u8,
    acl_max_packet_count: u16,
    sco_max_packet_count: u16,
) {
    DEBUG_CHANNEL_LAST_BLUETOOTH_ACL_MAX_DATA_LENGTH
        .store(acl_max_data_length as u32, Ordering::Release);
    DEBUG_CHANNEL_LAST_BLUETOOTH_SCO_MAX_DATA_LENGTH
        .store(sco_max_data_length as u32, Ordering::Release);
    DEBUG_CHANNEL_LAST_BLUETOOTH_ACL_MAX_PACKET_COUNT
        .store(acl_max_packet_count as u32, Ordering::Release);
    DEBUG_CHANNEL_LAST_BLUETOOTH_SCO_MAX_PACKET_COUNT
        .store(sco_max_packet_count as u32, Ordering::Release);
}

fn record_le_buffer_sizes(le_acl_max_data_length: u16, le_acl_max_packet_count: u8) {
    DEBUG_CHANNEL_LAST_BLUETOOTH_LE_ACL_MAX_DATA_LENGTH
        .store(le_acl_max_data_length as u32, Ordering::Release);
    DEBUG_CHANNEL_LAST_BLUETOOTH_LE_ACL_MAX_PACKET_COUNT
        .store(le_acl_max_packet_count as u32, Ordering::Release);
}

fn wait_for_command_complete<T>(
    bluetooth: &mut dyn fusion_hal::contract::drivers::net::bluetooth::BluetoothAdapterContract,
    expected_opcode: u16,
    read_buffer: &mut [u8],
    parser: impl Fn(BluetoothHciCommandComplete<'_>) -> Option<T>,
) -> Result<T, fusion_hal::contract::drivers::net::bluetooth::BluetoothError> {
    for _ in 0..4_096 {
        if bluetooth.wait_frame(Some(0))? {
            if let Some(frame) = bluetooth.recv_frame(read_buffer)? {
                if let Some(command_complete) = frame_command_complete(frame) {
                    let status = command_complete.return_parameters.first().copied();
                    record_hci_event_metadata(
                        BLUETOOTH_HCI_EVENT_COMMAND_COMPLETE,
                        command_complete.opcode,
                        status,
                    );
                    if command_complete.opcode == expected_opcode {
                        return parser(command_complete).ok_or_else(
                            fusion_hal::contract::drivers::net::bluetooth::BluetoothError::invalid,
                        );
                    }
                }
            }
        }
        pump_runtime(8);
    }
    Err(fusion_hal::contract::drivers::net::bluetooth::BluetoothError::timed_out())
}

#[entry]
fn main() -> ! {
    DEBUG_CHANNEL_PHASE.store(1, Ordering::Release);
    let _ = set_panic_led(false);
    DEBUG_CHANNEL_PHASE.store(2, Ordering::Release);
    let shift_register = init_shift_register_service();
    DEBUG_CHANNEL_PHASE.store(3, Ordering::Release);
    let display = init_display_service(shift_register);
    DEBUG_CHANNEL_PHASE.store(4, Ordering::Release);
    pump_runtime(128);
    set_status(&display, STATUS_STARTUP);
    pump_runtime(128);
    set_status(&display, STATUS_DISPLAY_READY);
    pump_runtime(128);

    DEBUG_CHANNEL_PHASE.store(5, Ordering::Release);
    let mut bluetooth_storage = StackDriverStorage::<BLUETOOTH_DRIVER_STORAGE_WORDS>::new();
    DEBUG_CHANNEL_PHASE.store(6, Ordering::Release);
    let mut bluetooth = match system_bluetooth(bluetooth_storage.slot()) {
        Ok(bluetooth) => bluetooth,
        Err(error) => {
            record_bluetooth_error(error);
            fatal_status(&display, STATUS_ERROR_BLUETOOTH_BIND)
        }
    };
    DEBUG_CHANNEL_PHASE.store(7, Ordering::Release);
    set_status(&display, STATUS_BLUETOOTH_BOUND);
    pump_runtime(128);

    DEBUG_CHANNEL_PHASE.store(8, Ordering::Release);
    if let Err(error) = bluetooth.adapter_mut().set_powered(true) {
        record_bluetooth_error(error);
        fatal_status(&display, STATUS_ERROR_BLUETOOTH_POWER);
    }
    DEBUG_CHANNEL_PHASE.store(9, Ordering::Release);
    set_status(&display, STATUS_BLUETOOTH_POWERED);
    pump_runtime(128);

    DEBUG_CHANNEL_PHASE.store(10, Ordering::Release);
    let mut write_scratch = [0_u8; 16];
    let mut read_buffer = [0_u8; 272];
    if let Err(error) = send_hci_command(
        bluetooth.adapter_mut(),
        BLUETOOTH_HCI_OPCODE_RESET,
        &[],
        &mut write_scratch,
    ) {
        record_bluetooth_error(error);
        fatal_status(&display, STATUS_ERROR_BLUETOOTH_HCI_SEND);
    }
    DEBUG_CHANNEL_PHASE.store(11, Ordering::Release);
    set_status(&display, STATUS_BLUETOOTH_HCI_RESET_SENT);
    pump_runtime(128);

    DEBUG_CHANNEL_PHASE.store(12, Ordering::Release);
    if let Err(error) =
        wait_for_command_complete(
            bluetooth.adapter_mut(),
            BLUETOOTH_HCI_OPCODE_RESET,
            &mut read_buffer,
            |command_complete| command_complete
                .return_parameters
                .first()
                .copied()
                .filter(|status| *status == 0),
        )
    {
        record_bluetooth_error(error);
        fatal_status(&display, STATUS_ERROR_BLUETOOTH_HCI_WAIT);
    }
    DEBUG_CHANNEL_PHASE.store(13, Ordering::Release);
    set_status(&display, STATUS_BLUETOOTH_HCI_RESET_COMPLETE);
    pump_runtime(128);

    DEBUG_CHANNEL_PHASE.store(14, Ordering::Release);
    if let Err(error) = send_hci_command(
        bluetooth.adapter_mut(),
        BLUETOOTH_HCI_OPCODE_READ_LOCAL_VERSION_INFORMATION,
        &[],
        &mut write_scratch,
    ) {
        record_bluetooth_error(error);
        fatal_status(&display, STATUS_ERROR_BLUETOOTH_HCI_SEND);
    }
    DEBUG_CHANNEL_PHASE.store(15, Ordering::Release);
    set_status(&display, STATUS_BLUETOOTH_HCI_VERSION_SENT);
    pump_runtime(128);

    DEBUG_CHANNEL_PHASE.store(16, Ordering::Release);
    let local_version = match wait_for_command_complete(
        bluetooth.adapter_mut(),
        BLUETOOTH_HCI_OPCODE_READ_LOCAL_VERSION_INFORMATION,
        &mut read_buffer,
        |command_complete| {
            command_complete
                .local_version_information()
                .filter(|info| info.status == 0)
        },
    ) {
        Ok(local_version) => Ok(local_version),
        Err(error) => Err(error),
    };
    let local_version = match local_version {
        Ok(local_version) => local_version,
        Err(error) => {
            record_bluetooth_error(error);
            fatal_status(&display, STATUS_ERROR_BLUETOOTH_HCI_VERSION);
        }
    };
    DEBUG_CHANNEL_LAST_BLUETOOTH_HCI_VERSION
        .store(local_version.hci_version as u32, Ordering::Release);
    DEBUG_CHANNEL_LAST_BLUETOOTH_HCI_REVISION
        .store(local_version.hci_revision as u32, Ordering::Release);
    DEBUG_CHANNEL_LAST_BLUETOOTH_MANUFACTURER
        .store(local_version.manufacturer_name as u32, Ordering::Release);
    DEBUG_CHANNEL_LAST_BLUETOOTH_SUBVERSION
        .store(local_version.lmp_pal_subversion as u32, Ordering::Release);
    DEBUG_CHANNEL_PHASE.store(17, Ordering::Release);
    set_status(&display, STATUS_BLUETOOTH_HCI_VERSION_COMPLETE);
    pump_runtime(128);

    DEBUG_CHANNEL_PHASE.store(18, Ordering::Release);
    if let Err(error) = send_hci_command(
        bluetooth.adapter_mut(),
        BLUETOOTH_HCI_OPCODE_READ_BD_ADDR,
        &[],
        &mut write_scratch,
    ) {
        record_bluetooth_error(error);
        fatal_status(&display, STATUS_ERROR_BLUETOOTH_HCI_SEND);
    }
    DEBUG_CHANNEL_PHASE.store(19, Ordering::Release);
    set_status(&display, STATUS_BLUETOOTH_HCI_BD_ADDR_SENT);
    pump_runtime(128);

    DEBUG_CHANNEL_PHASE.store(20, Ordering::Release);
    let bd_addr = match wait_for_command_complete(
        bluetooth.adapter_mut(),
        BLUETOOTH_HCI_OPCODE_READ_BD_ADDR,
        &mut read_buffer,
        |command_complete| command_complete.bd_addr().filter(|(status, _)| *status == 0),
    ) {
        Ok(bd_addr) => Ok(bd_addr),
        Err(error) => Err(error),
    };
    let (_, bd_addr) = match bd_addr {
        Ok(bd_addr) => bd_addr,
        Err(error) => {
            record_bluetooth_error(error);
            fatal_status(&display, STATUS_ERROR_BLUETOOTH_HCI_BD_ADDR);
        }
    };
    DEBUG_CHANNEL_LAST_BLUETOOTH_ADDRESS_LOW.store(
        u32::from_le_bytes([bd_addr.bytes[0], bd_addr.bytes[1], bd_addr.bytes[2], bd_addr.bytes[3]]),
        Ordering::Release,
    );
    DEBUG_CHANNEL_LAST_BLUETOOTH_ADDRESS_HIGH.store(
        u32::from_le_bytes([bd_addr.bytes[4], bd_addr.bytes[5], 0, 0]),
        Ordering::Release,
    );

    DEBUG_CHANNEL_PHASE.store(21, Ordering::Release);
    set_status(&display, STATUS_BLUETOOTH_HCI_BD_ADDR_COMPLETE);
    pump_runtime(128);

    DEBUG_CHANNEL_PHASE.store(22, Ordering::Release);
    if let Err(error) = send_hci_command(
        bluetooth.adapter_mut(),
        BLUETOOTH_HCI_OPCODE_READ_LOCAL_SUPPORTED_COMMANDS,
        &[],
        &mut write_scratch,
    ) {
        record_bluetooth_error(error);
        fatal_status(&display, STATUS_ERROR_BLUETOOTH_HCI_SEND);
    }
    DEBUG_CHANNEL_PHASE.store(23, Ordering::Release);
    set_status(&display, STATUS_BLUETOOTH_HCI_COMMANDS_SENT);
    pump_runtime(128);

    DEBUG_CHANNEL_PHASE.store(24, Ordering::Release);
    let supported_commands = match wait_for_command_complete(
        bluetooth.adapter_mut(),
        BLUETOOTH_HCI_OPCODE_READ_LOCAL_SUPPORTED_COMMANDS,
        &mut read_buffer,
        |command_complete| command_complete
            .local_supported_commands()
            .filter(|(status, _)| *status == 0)
            .map(|(_, commands)| commands),
    ) {
        Ok(commands) => commands,
        Err(error) => {
            record_bluetooth_error(error);
            fatal_status(&display, STATUS_ERROR_BLUETOOTH_HCI_COMMANDS);
        }
    };
    record_supported_commands(supported_commands.bytes);
    DEBUG_CHANNEL_PHASE.store(25, Ordering::Release);
    set_status(&display, STATUS_BLUETOOTH_HCI_COMMANDS_COMPLETE);
    pump_runtime(128);

    DEBUG_CHANNEL_PHASE.store(26, Ordering::Release);
    if let Err(error) = send_hci_command(
        bluetooth.adapter_mut(),
        BLUETOOTH_HCI_OPCODE_READ_LOCAL_SUPPORTED_FEATURES,
        &[],
        &mut write_scratch,
    ) {
        record_bluetooth_error(error);
        fatal_status(&display, STATUS_ERROR_BLUETOOTH_HCI_SEND);
    }
    DEBUG_CHANNEL_PHASE.store(27, Ordering::Release);
    set_status(&display, STATUS_BLUETOOTH_HCI_FEATURES_SENT);
    pump_runtime(128);

    DEBUG_CHANNEL_PHASE.store(28, Ordering::Release);
    let features = match wait_for_command_complete(
        bluetooth.adapter_mut(),
        BLUETOOTH_HCI_OPCODE_READ_LOCAL_SUPPORTED_FEATURES,
        &mut read_buffer,
        |command_complete| command_complete
            .local_supported_features()
            .filter(|(status, _)| *status == 0)
            .map(|(_, features)| features),
    ) {
        Ok(features) => features,
        Err(error) => {
            record_bluetooth_error(error);
            fatal_status(&display, STATUS_ERROR_BLUETOOTH_HCI_FEATURES);
        }
    };
    record_feature_set(
        &DEBUG_CHANNEL_LAST_BLUETOOTH_FEATURES_LOW,
        &DEBUG_CHANNEL_LAST_BLUETOOTH_FEATURES_HIGH,
        features,
    );
    DEBUG_CHANNEL_PHASE.store(29, Ordering::Release);
    set_status(&display, STATUS_BLUETOOTH_HCI_FEATURES_COMPLETE);
    pump_runtime(128);

    DEBUG_CHANNEL_PHASE.store(30, Ordering::Release);
    if let Err(error) = send_hci_command(
        bluetooth.adapter_mut(),
        BLUETOOTH_HCI_OPCODE_LE_READ_LOCAL_SUPPORTED_FEATURES,
        &[],
        &mut write_scratch,
    ) {
        record_bluetooth_error(error);
        fatal_status(&display, STATUS_ERROR_BLUETOOTH_HCI_SEND);
    }
    DEBUG_CHANNEL_PHASE.store(31, Ordering::Release);
    set_status(&display, STATUS_BLUETOOTH_HCI_LE_FEATURES_SENT);
    pump_runtime(128);

    DEBUG_CHANNEL_PHASE.store(32, Ordering::Release);
    let le_features = match wait_for_command_complete(
        bluetooth.adapter_mut(),
        BLUETOOTH_HCI_OPCODE_LE_READ_LOCAL_SUPPORTED_FEATURES,
        &mut read_buffer,
        |command_complete| command_complete
            .le_local_supported_features()
            .filter(|(status, _)| *status == 0)
            .map(|(_, features)| features),
    ) {
        Ok(features) => features,
        Err(error) => {
            record_bluetooth_error(error);
            fatal_status(&display, STATUS_ERROR_BLUETOOTH_HCI_LE_FEATURES);
        }
    };
    record_feature_set(
        &DEBUG_CHANNEL_LAST_BLUETOOTH_LE_FEATURES_LOW,
        &DEBUG_CHANNEL_LAST_BLUETOOTH_LE_FEATURES_HIGH,
        le_features,
    );
    DEBUG_CHANNEL_PHASE.store(33, Ordering::Release);
    set_status(&display, STATUS_BLUETOOTH_HCI_LE_FEATURES_COMPLETE);
    pump_runtime(128);

    DEBUG_CHANNEL_PHASE.store(34, Ordering::Release);
    if let Err(error) = send_hci_command(
        bluetooth.adapter_mut(),
        BLUETOOTH_HCI_OPCODE_READ_BUFFER_SIZE,
        &[],
        &mut write_scratch,
    ) {
        record_bluetooth_error(error);
        fatal_status(&display, STATUS_ERROR_BLUETOOTH_HCI_SEND);
    }
    DEBUG_CHANNEL_PHASE.store(35, Ordering::Release);
    set_status(&display, STATUS_BLUETOOTH_HCI_BUFFER_SIZE_SENT);
    pump_runtime(128);

    DEBUG_CHANNEL_PHASE.store(36, Ordering::Release);
    let buffer_size = match wait_for_command_complete(
        bluetooth.adapter_mut(),
        BLUETOOTH_HCI_OPCODE_READ_BUFFER_SIZE,
        &mut read_buffer,
        |command_complete| command_complete.buffer_size().filter(|buffer_size| buffer_size.status == 0),
    ) {
        Ok(buffer_size) => buffer_size,
        Err(error) => {
            record_bluetooth_error(error);
            fatal_status(&display, STATUS_ERROR_BLUETOOTH_HCI_BUFFER_SIZE);
        }
    };
    record_buffer_sizes(
        buffer_size.acl_max_data_length,
        buffer_size.sco_max_data_length,
        buffer_size.acl_max_packet_count,
        buffer_size.sco_max_packet_count,
    );
    DEBUG_CHANNEL_PHASE.store(37, Ordering::Release);
    set_status(&display, STATUS_BLUETOOTH_HCI_BUFFER_SIZE_COMPLETE);
    pump_runtime(128);

    DEBUG_CHANNEL_PHASE.store(38, Ordering::Release);
    if let Err(error) = send_hci_command(
        bluetooth.adapter_mut(),
        BLUETOOTH_HCI_OPCODE_LE_READ_BUFFER_SIZE,
        &[],
        &mut write_scratch,
    ) {
        record_bluetooth_error(error);
        fatal_status(&display, STATUS_ERROR_BLUETOOTH_HCI_SEND);
    }
    DEBUG_CHANNEL_PHASE.store(39, Ordering::Release);
    set_status(&display, STATUS_BLUETOOTH_HCI_LE_BUFFER_SIZE_SENT);
    pump_runtime(128);

    DEBUG_CHANNEL_PHASE.store(40, Ordering::Release);
    let le_buffer_size = match wait_for_command_complete(
        bluetooth.adapter_mut(),
        BLUETOOTH_HCI_OPCODE_LE_READ_BUFFER_SIZE,
        &mut read_buffer,
        |command_complete| command_complete
            .le_buffer_size()
            .filter(|buffer_size| buffer_size.status == 0),
    ) {
        Ok(buffer_size) => buffer_size,
        Err(error) => {
            record_bluetooth_error(error);
            fatal_status(&display, STATUS_ERROR_BLUETOOTH_HCI_LE_BUFFER_SIZE);
        }
    };
    record_le_buffer_sizes(
        le_buffer_size.le_acl_max_data_length,
        le_buffer_size.le_acl_max_packet_count,
    );
    DEBUG_CHANNEL_PHASE.store(41, Ordering::Release);
    set_status(&display, STATUS_BLUETOOTH_HCI_LE_BUFFER_SIZE_COMPLETE);
    pump_runtime(128);

    DEBUG_CHANNEL_PHASE.store(42, Ordering::Release);
    let advertising_parameters = legacy_advertising_parameters();
    if let Err(error) = send_hci_command(
        bluetooth.adapter_mut(),
        BLUETOOTH_HCI_OPCODE_LE_SET_ADVERTISING_PARAMETERS,
        &advertising_parameters,
        &mut write_scratch,
    ) {
        record_bluetooth_error(error);
        fatal_status(&display, STATUS_ERROR_BLUETOOTH_HCI_SEND);
    }
    DEBUG_CHANNEL_PHASE.store(43, Ordering::Release);
    set_status(&display, STATUS_BLUETOOTH_HCI_ADV_PARAMS_SENT);
    pump_runtime(128);

    DEBUG_CHANNEL_PHASE.store(44, Ordering::Release);
    if let Err(error) = wait_for_command_complete_status_zero(
        bluetooth.adapter_mut(),
        BLUETOOTH_HCI_OPCODE_LE_SET_ADVERTISING_PARAMETERS,
        &mut read_buffer,
    ) {
        record_bluetooth_error(error);
        fatal_status(&display, STATUS_ERROR_BLUETOOTH_HCI_ADV_PARAMS);
    }
    DEBUG_CHANNEL_PHASE.store(45, Ordering::Release);
    set_status(&display, STATUS_BLUETOOTH_HCI_ADV_PARAMS_COMPLETE);
    pump_runtime(128);

    DEBUG_CHANNEL_PHASE.store(46, Ordering::Release);
    let advertising_data = legacy_advertising_data();
    if let Err(error) = send_hci_command(
        bluetooth.adapter_mut(),
        BLUETOOTH_HCI_OPCODE_LE_SET_ADVERTISING_DATA,
        &advertising_data,
        &mut write_scratch,
    ) {
        record_bluetooth_error(error);
        fatal_status(&display, STATUS_ERROR_BLUETOOTH_HCI_SEND);
    }
    DEBUG_CHANNEL_PHASE.store(47, Ordering::Release);
    set_status(&display, STATUS_BLUETOOTH_HCI_ADV_DATA_SENT);
    pump_runtime(128);

    DEBUG_CHANNEL_PHASE.store(48, Ordering::Release);
    if let Err(error) = wait_for_command_complete_status_zero(
        bluetooth.adapter_mut(),
        BLUETOOTH_HCI_OPCODE_LE_SET_ADVERTISING_DATA,
        &mut read_buffer,
    ) {
        record_bluetooth_error(error);
        fatal_status(&display, STATUS_ERROR_BLUETOOTH_HCI_ADV_DATA);
    }
    DEBUG_CHANNEL_PHASE.store(49, Ordering::Release);
    set_status(&display, STATUS_BLUETOOTH_HCI_ADV_DATA_COMPLETE);
    pump_runtime(128);

    DEBUG_CHANNEL_PHASE.store(50, Ordering::Release);
    let scan_response_data = legacy_scan_response_data();
    if let Err(error) = send_hci_command(
        bluetooth.adapter_mut(),
        BLUETOOTH_HCI_OPCODE_LE_SET_SCAN_RESPONSE_DATA,
        &scan_response_data,
        &mut write_scratch,
    ) {
        record_bluetooth_error(error);
        fatal_status(&display, STATUS_ERROR_BLUETOOTH_HCI_SEND);
    }
    DEBUG_CHANNEL_PHASE.store(51, Ordering::Release);
    set_status(&display, STATUS_BLUETOOTH_HCI_SCAN_RESPONSE_SENT);
    pump_runtime(128);

    DEBUG_CHANNEL_PHASE.store(52, Ordering::Release);
    if let Err(error) = wait_for_command_complete_status_zero(
        bluetooth.adapter_mut(),
        BLUETOOTH_HCI_OPCODE_LE_SET_SCAN_RESPONSE_DATA,
        &mut read_buffer,
    ) {
        record_bluetooth_error(error);
        fatal_status(&display, STATUS_ERROR_BLUETOOTH_HCI_SCAN_RESPONSE);
    }
    DEBUG_CHANNEL_PHASE.store(53, Ordering::Release);
    set_status(&display, STATUS_BLUETOOTH_HCI_SCAN_RESPONSE_COMPLETE);
    pump_runtime(128);

    DEBUG_CHANNEL_PHASE.store(54, Ordering::Release);
    if let Err(error) = send_hci_command(
        bluetooth.adapter_mut(),
        BLUETOOTH_HCI_OPCODE_LE_SET_ADVERTISING_ENABLE,
        &[1],
        &mut write_scratch,
    ) {
        record_bluetooth_error(error);
        fatal_status(&display, STATUS_ERROR_BLUETOOTH_HCI_SEND);
    }
    DEBUG_CHANNEL_PHASE.store(55, Ordering::Release);
    set_status(&display, STATUS_BLUETOOTH_HCI_ADV_ENABLE_SENT);
    pump_runtime(128);

    DEBUG_CHANNEL_PHASE.store(56, Ordering::Release);
    if let Err(error) = wait_for_command_complete_status_zero(
        bluetooth.adapter_mut(),
        BLUETOOTH_HCI_OPCODE_LE_SET_ADVERTISING_ENABLE,
        &mut read_buffer,
    ) {
        record_bluetooth_error(error);
        fatal_status(&display, STATUS_ERROR_BLUETOOTH_HCI_ADV_ENABLE);
    }
    DEBUG_CHANNEL_PHASE.store(57, Ordering::Release);
    set_status(&display, STATUS_BLUETOOTH_HCI_ADV_ENABLE_COMPLETE);
    pump_runtime(128);

    DEBUG_CHANNEL_PHASE.store(58, Ordering::Release);
    match bluetooth.adapter().is_powered() {
        Ok(true) => {
            DEBUG_CHANNEL_PHASE.store(59, Ordering::Release);
        }
        Ok(false) => fatal_status(&display, STATUS_ERROR_BLUETOOTH_HCI_ADV_ENABLE),
        Err(error) => {
            record_bluetooth_error(error);
            fatal_status(&display, STATUS_ERROR_BLUETOOTH_HCI_ADV_ENABLE)
        }
    }
    set_status(&display, STATUS_BLUETOOTH_HCI_ADVERTISING_ENABLED);
    pump_runtime(128);

    DEBUG_CHANNEL_PHASE.store(60, Ordering::Release);
    loop {
        wait_for_runtime_progress();
    }
}

#[exception]
unsafe fn HardFault(_frame: &ExceptionFrame) -> ! {
    panic_led_on()
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    panic_led_on()
}
