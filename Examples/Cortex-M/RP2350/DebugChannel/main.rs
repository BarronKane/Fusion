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
//! - `C2C0` LE random-address command sent
//! - `C2CF` LE random-address command completed
//! - `C2D0` LE advertising-parameters command sent
//! - `C2DF` LE advertising-parameters command completed
//! - `C2E0` LE advertising-data command sent
//! - `C2EF` LE advertising-data command completed
//! - `C2F0` LE scan-response command sent
//! - `C2FF` LE scan-response command completed
//! - `C300` LE advertising-enable command sent
//! - `C30F` LE advertising-enable command completed
//! - `C31F` LE advertising enabled
//! - `C30F` legacy LE advertising enabled
//! - `E2xx` failure stage

use core::mem::MaybeUninit;
use core::panic::PanicInfo;
use core::sync::atomic::{
    AtomicU16,
    AtomicU8,
    AtomicU32,
    Ordering,
};

use cortex_m_rt::{
    ExceptionFrame,
    exception,
};
use fusion_example_rp2350_on_device::runtime::wait_for_runtime_progress;
use fusion_example_rp2350_on_device::seven_segment_timer::Rp2350TimerFourDigitSevenSegmentDisplay;
use fusion_firmware::sys::hal::drivers::bus::gpio::{
    SystemGpioPin,
    system_gpio,
};
use fusion_firmware::sys::hal::drivers::net::chipset::infineon::cyw43439::system_bluetooth_courier;
use fusion_hal::contract::drivers::bus::gpio::{
    GpioControlContract,
    GpioDriveStrength,
    GpioError,
    GpioErrorKind,
};
use fusion_hal::contract::drivers::net::bluetooth::{
    BLUETOOTH_HCI_EVENT_COMMAND_COMPLETE,
    BLUETOOTH_HCI_EVENT_LE_META,
    BLUETOOTH_HCI_OPCODE_READ_BD_ADDR,
    BLUETOOTH_HCI_OPCODE_READ_LOCAL_VERSION_INFORMATION,
    BLUETOOTH_HCI_OPCODE_READ_LOCAL_SUPPORTED_COMMANDS,
    BLUETOOTH_HCI_OPCODE_READ_LOCAL_SUPPORTED_FEATURES,
    BLUETOOTH_HCI_OPCODE_READ_BUFFER_SIZE,
    BLUETOOTH_HCI_OPCODE_SET_EVENT_MASK,
    BLUETOOTH_HCI_OPCODE_LE_SET_EVENT_MASK,
    BLUETOOTH_HCI_OPCODE_LE_READ_BUFFER_SIZE,
    BLUETOOTH_HCI_OPCODE_RESET,
    BLUETOOTH_HCI_OPCODE_LE_READ_LOCAL_SUPPORTED_FEATURES,
    BluetoothAdvertisingControlContract,
    BluetoothAdvertisingMode,
    BluetoothAdvertisingParameters,
    BluetoothCanonicalFrame,
    BluetoothCanonicalFrameControlContract,
    BluetoothHciCommandComplete,
    BluetoothErrorKind,
    BluetoothHciCommandFrame,
    BluetoothHciCommandHeader,
    BluetoothHciFrame,
    BluetoothHciFrameView,
    BluetoothHciPacketType,
    BluetoothLePhy,
    BluetoothRadioControlContract,
};

fusion_example_rp2350_on_device::fusion_rp2350_export_build_id!();

const DISPLAY_DATA_PIN: u8 = 12;
const DISPLAY_ENABLE_PIN: u8 = 13;
const DISPLAY_LATCH_PIN: u8 = 14;
const DISPLAY_SHIFT_CLOCK_PIN: u8 = 15;
const PANIC_LED_PIN: u8 = 11;

const STATUS_STARTUP: u16 = 0xC100;
const STATUS_DISPLAY_READY: u16 = 0xC110;
const STATUS_ERROR_DISPLAY_INIT: u16 = 0xE110;
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
const STATUS_BLUETOOTH_HCI_ADV_PARAMS_SENT: u16 = 0xC2D0;
const STATUS_BLUETOOTH_HCI_ADV_PARAMS_COMPLETE: u16 = 0xC2DF;
const STATUS_BLUETOOTH_HCI_ADV_DATA_SENT: u16 = 0xC2E0;
const STATUS_BLUETOOTH_HCI_ADV_DATA_COMPLETE: u16 = 0xC2EF;
const STATUS_BLUETOOTH_HCI_SCAN_RESPONSE_SENT: u16 = 0xC2F0;
const STATUS_BLUETOOTH_HCI_SCAN_RESPONSE_COMPLETE: u16 = 0xC2FF;
const STATUS_BLUETOOTH_HCI_ADV_ENABLE_SENT: u16 = 0xC300;
const STATUS_BLUETOOTH_HCI_ADV_ENABLE_COMPLETE: u16 = 0xC30F;
const STATUS_BLUETOOTH_HCI_ADVERTISING_ENABLED: u16 = 0xC31F;
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
const STATUS_ERROR_BLUETOOTH_HCI_ADV_PARAMS: u16 = 0xE2DF;
const STATUS_ERROR_BLUETOOTH_HCI_ADV_DATA: u16 = 0xE2EF;
const STATUS_ERROR_BLUETOOTH_HCI_ADV_ENABLE: u16 = 0xE30F;
const BLUETOOTH_HCI_OPCODE_WRITE_LE_HOST_SUPPORT: u16 = 0x0C6D;
const BLUETOOTH_HCI_OPCODE_CYW43_SET_PUBLIC_BD_ADDR: u16 = 0xFC01;
const BLUETOOTH_HCI_OPCODE_SET_EVENT_MASK_PAGE_2: u16 = 0x0C63;

const PANIC_LED_UNINITIALIZED: u8 = 0;
const PANIC_LED_READY: u8 = 1;
const PANIC_LED_FAILED: u8 = 2;

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
#[unsafe(no_mangle)]
pub static DEBUG_CHANNEL_LAST_BLUETOOTH_ADV_PARAM_CANDIDATE: AtomicU32 = AtomicU32::new(0);

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

fn encode_gpio_error(error: GpioError) -> u32 {
    match error.kind() {
        GpioErrorKind::Unsupported => 1,
        GpioErrorKind::Invalid => 2,
        GpioErrorKind::Busy => 3,
        GpioErrorKind::ResourceExhausted => 4,
        GpioErrorKind::StateConflict => 5,
        GpioErrorKind::Platform(code) => 0x4000_0000_u32 | (code as u32),
    }
}

fn init_display() -> Result<Rp2350TimerFourDigitSevenSegmentDisplay, u32> {
    DEBUG_CHANNEL_PHASE.store(0x31, Ordering::Release);
    let display = Rp2350TimerFourDigitSevenSegmentDisplay::common_cathode(
        DISPLAY_DATA_PIN,
        DISPLAY_ENABLE_PIN,
        DISPLAY_LATCH_PIN,
        DISPLAY_SHIFT_CLOCK_PIN,
    )
    .map_err(encode_gpio_error)?;
    DEBUG_CHANNEL_PHASE.store(0x32, Ordering::Release);
    Ok(display)
}

fn set_status(display: &Rp2350TimerFourDigitSevenSegmentDisplay, code: u16) {
    DEBUG_CHANNEL_LAST_STATUS.store(code, Ordering::Release);
    let _ = display.set_hex(code);
}

fn fatal_without_display(code: u16) -> ! {
    let _ = set_panic_led(true);
    DEBUG_CHANNEL_LAST_STATUS.store(code, Ordering::Release);
    loop {
        wait_for_runtime_progress();
    }
}

fn fatal_status(display: &Rp2350TimerFourDigitSevenSegmentDisplay, code: u16) -> ! {
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

fn record_observed_hci_frame(frame: BluetoothCanonicalFrame<'_>) {
    match frame {
        BluetoothCanonicalFrame::Hci(BluetoothHciFrameView::Event(event)) => {
            DEBUG_CHANNEL_LAST_BLUETOOTH_EVENT
                .store(event.header.event_code as u32, Ordering::Release);
            DEBUG_CHANNEL_LAST_BLUETOOTH_STATUS.store(
                event.parameters.first().copied().map_or(u32::MAX, u32::from),
                Ordering::Release,
            );
            if event.header.event_code == BLUETOOTH_HCI_EVENT_LE_META {
                DEBUG_CHANNEL_LAST_BLUETOOTH_OPCODE.store(
                    event.parameters.first().copied().map_or(u32::MAX, u32::from),
                    Ordering::Release,
                );
            }
        }
        BluetoothCanonicalFrame::Hci(BluetoothHciFrameView::Opaque(BluetoothHciFrame {
            packet_type: BluetoothHciPacketType::Event,
            bytes,
        })) if bytes.len() >= 2 => {
            DEBUG_CHANNEL_LAST_BLUETOOTH_EVENT.store(bytes[0] as u32, Ordering::Release);
            DEBUG_CHANNEL_LAST_BLUETOOTH_STATUS.store(
                bytes.get(2).copied().map_or(u32::MAX, u32::from),
                Ordering::Release,
            );
            if bytes[0] == BLUETOOTH_HCI_EVENT_LE_META {
                DEBUG_CHANNEL_LAST_BLUETOOTH_OPCODE.store(
                    bytes.get(2).copied().map_or(u32::MAX, u32::from),
                    Ordering::Release,
                );
            }
        }
        _ => {}
    }
}

trait BluetoothBenchControl:
    BluetoothCanonicalFrameControlContract + BluetoothRadioControlContract
{
}

impl<T> BluetoothBenchControl for T where
    T: BluetoothCanonicalFrameControlContract + BluetoothRadioControlContract
{
}

fn send_hci_command<B>(
    bluetooth: &mut B,
    opcode: u16,
    parameters: &[u8],
    scratch: &mut [u8],
) -> Result<(), fusion_hal::contract::drivers::net::bluetooth::BluetoothError>
where
    B: BluetoothBenchControl + ?Sized,
{
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

fn wait_for_command_complete_status_zero<B>(
    bluetooth: &mut B,
    opcode: u16,
    read_buffer: &mut [u8],
) -> Result<(), fusion_hal::contract::drivers::net::bluetooth::BluetoothError>
where
    B: BluetoothBenchControl + ?Sized,
{
    wait_for_command_complete(bluetooth, opcode, read_buffer, |command_complete| {
        command_complete
            .return_parameters
            .first()
            .copied()
            .filter(|status| *status == 0)
            .map(|_| ())
    })
}

fn record_supported_commands(commands: [u8; 64]) {
    for (index, chunk) in commands.chunks_exact(4).enumerate() {
        DEBUG_CHANNEL_LAST_BLUETOOTH_SUPPORTED_COMMANDS[index].store(
            u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]),
            Ordering::Release,
        );
    }
}

fn command_bitmap_bit_is_set(commands: &[u8; 64], byte_index: usize, bit_index: u8) -> bool {
    commands
        .get(byte_index)
        .map(|byte| (byte & (1_u8 << bit_index)) != 0)
        .unwrap_or(false)
}

fn write_le_host_supported_is_supported(commands: &[u8; 64]) -> bool {
    // HCI Read Local Supported Commands: byte 24, bit 6.
    command_bitmap_bit_is_set(commands, 24, 6)
}

fn set_event_mask_page_2_is_supported(commands: &[u8; 64]) -> bool {
    // HCI Read Local Supported Commands: byte 22, bit 2.
    command_bitmap_bit_is_set(commands, 22, 2)
}

fn classic_event_mask() -> [u8; 8] {
    [0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x3f]
}

fn event_mask_page_2() -> [u8; 8] {
    [0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00]
}

fn all_le_events_mask() -> [u8; 8] {
    // Matches BTstack's legacy LE event mask without Enhanced Connection Complete. That is boring,
    // conservative, and known-good enough to stop this bench from inventing its own theology.
    [0xff, 0xfd, 0xff, 0xff, 0x07, 0xfc, 0x7f, 0x00]
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

fn wait_for_command_complete<B, T>(
    bluetooth: &mut B,
    expected_opcode: u16,
    read_buffer: &mut [u8],
    parser: impl Fn(BluetoothHciCommandComplete<'_>) -> Option<T>,
) -> Result<T, fusion_hal::contract::drivers::net::bluetooth::BluetoothError>
where
    B: BluetoothBenchControl + ?Sized,
{
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
    }
    Err(fusion_hal::contract::drivers::net::bluetooth::BluetoothError::timed_out())
}

#[fusion_firmware::fusion_firmware_main]
fn main() -> ! {
    DEBUG_CHANNEL_PHASE.store(4, Ordering::Release);
    let display = match init_display() {
        Ok(display) => display,
        Err(error) => {
            DEBUG_CHANNEL_LAST_ERROR_KIND.store(error, Ordering::Release);
            fatal_without_display(STATUS_ERROR_DISPLAY_INIT);
        }
    };
    set_status(&display, STATUS_STARTUP);
    set_status(&display, STATUS_DISPLAY_READY);

    DEBUG_CHANNEL_PHASE.store(5, Ordering::Release);
    DEBUG_CHANNEL_PHASE.store(6, Ordering::Release);
    let mut bluetooth = match system_bluetooth_courier() {
        Ok(bluetooth) => bluetooth,
        Err(error) => {
            record_bluetooth_error(error);
            fatal_status(&display, STATUS_ERROR_BLUETOOTH_BIND)
        }
    };
    DEBUG_CHANNEL_PHASE.store(7, Ordering::Release);
    set_status(&display, STATUS_BLUETOOTH_BOUND);

    DEBUG_CHANNEL_PHASE.store(8, Ordering::Release);
    if let Err(error) = bluetooth.set_powered(true) {
        record_bluetooth_error(error);
        fatal_status(&display, STATUS_ERROR_BLUETOOTH_POWER);
    }
    DEBUG_CHANNEL_PHASE.store(9, Ordering::Release);
    set_status(&display, STATUS_BLUETOOTH_POWERED);

    DEBUG_CHANNEL_PHASE.store(10, Ordering::Release);
    let mut write_scratch = [0_u8; 16];
    let mut read_buffer = [0_u8; 272];
    if let Err(error) = send_hci_command(
        &mut bluetooth,
        BLUETOOTH_HCI_OPCODE_RESET,
        &[],
        &mut write_scratch,
    ) {
        record_bluetooth_error(error);
        fatal_status(&display, STATUS_ERROR_BLUETOOTH_HCI_SEND);
    }
    DEBUG_CHANNEL_PHASE.store(11, Ordering::Release);
    set_status(&display, STATUS_BLUETOOTH_HCI_RESET_SENT);

    DEBUG_CHANNEL_PHASE.store(12, Ordering::Release);
    if let Err(error) =
        wait_for_command_complete(
            &mut bluetooth,
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

    DEBUG_CHANNEL_PHASE.store(14, Ordering::Release);
    if let Err(error) = send_hci_command(
        &mut bluetooth,
        BLUETOOTH_HCI_OPCODE_READ_LOCAL_VERSION_INFORMATION,
        &[],
        &mut write_scratch,
    ) {
        record_bluetooth_error(error);
        fatal_status(&display, STATUS_ERROR_BLUETOOTH_HCI_SEND);
    }
    DEBUG_CHANNEL_PHASE.store(15, Ordering::Release);
    set_status(&display, STATUS_BLUETOOTH_HCI_VERSION_SENT);

    DEBUG_CHANNEL_PHASE.store(16, Ordering::Release);
    let local_version = match wait_for_command_complete(
        &mut bluetooth,
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

    DEBUG_CHANNEL_PHASE.store(18, Ordering::Release);
    if let Err(error) = send_hci_command(
        &mut bluetooth,
        BLUETOOTH_HCI_OPCODE_READ_BD_ADDR,
        &[],
        &mut write_scratch,
    ) {
        record_bluetooth_error(error);
        fatal_status(&display, STATUS_ERROR_BLUETOOTH_HCI_SEND);
    }
    DEBUG_CHANNEL_PHASE.store(19, Ordering::Release);
    set_status(&display, STATUS_BLUETOOTH_HCI_BD_ADDR_SENT);

    DEBUG_CHANNEL_PHASE.store(20, Ordering::Release);
    let bd_addr = match wait_for_command_complete(
        &mut bluetooth,
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

    DEBUG_CHANNEL_PHASE.store(20_1, Ordering::Release);
    if let Err(error) = send_hci_command(
        &mut bluetooth,
        BLUETOOTH_HCI_OPCODE_CYW43_SET_PUBLIC_BD_ADDR,
        &bd_addr.bytes,
        &mut write_scratch,
    ) {
        record_bluetooth_error(error);
        fatal_status(&display, STATUS_ERROR_BLUETOOTH_HCI_SEND);
    }
    DEBUG_CHANNEL_PHASE.store(20_2, Ordering::Release);
    if let Err(error) = wait_for_command_complete_status_zero(
        &mut bluetooth,
        BLUETOOTH_HCI_OPCODE_CYW43_SET_PUBLIC_BD_ADDR,
        &mut read_buffer,
    ) {
        record_bluetooth_error(error);
        fatal_status(&display, STATUS_ERROR_BLUETOOTH_HCI_BD_ADDR);
    }

    DEBUG_CHANNEL_PHASE.store(22, Ordering::Release);
    if let Err(error) = send_hci_command(
        &mut bluetooth,
        BLUETOOTH_HCI_OPCODE_READ_LOCAL_SUPPORTED_COMMANDS,
        &[],
        &mut write_scratch,
    ) {
        record_bluetooth_error(error);
        fatal_status(&display, STATUS_ERROR_BLUETOOTH_HCI_SEND);
    }
    DEBUG_CHANNEL_PHASE.store(23, Ordering::Release);
    set_status(&display, STATUS_BLUETOOTH_HCI_COMMANDS_SENT);

    DEBUG_CHANNEL_PHASE.store(24, Ordering::Release);
    let supported_commands = match wait_for_command_complete(
        &mut bluetooth,
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

    DEBUG_CHANNEL_PHASE.store(25_0, Ordering::Release);
    let classic_event_mask = classic_event_mask();
    if let Err(error) = send_hci_command(
        &mut bluetooth,
        BLUETOOTH_HCI_OPCODE_SET_EVENT_MASK,
        &classic_event_mask,
        &mut write_scratch,
    ) {
        record_bluetooth_error(error);
        fatal_status(&display, STATUS_ERROR_BLUETOOTH_HCI_SEND);
    }
    if let Err(error) = wait_for_command_complete_status_zero(
        &mut bluetooth,
        BLUETOOTH_HCI_OPCODE_SET_EVENT_MASK,
        &mut read_buffer,
    ) {
        record_bluetooth_error(error);
        fatal_status(&display, STATUS_ERROR_BLUETOOTH_HCI_FEATURES);
    }

    if set_event_mask_page_2_is_supported(&supported_commands.bytes) {
        DEBUG_CHANNEL_PHASE.store(25_0_1, Ordering::Release);
        let event_mask_2 = event_mask_page_2();
        if let Err(error) = send_hci_command(
            &mut bluetooth,
            BLUETOOTH_HCI_OPCODE_SET_EVENT_MASK_PAGE_2,
            &event_mask_2,
            &mut write_scratch,
        ) {
            record_bluetooth_error(error);
            fatal_status(&display, STATUS_ERROR_BLUETOOTH_HCI_SEND);
        }
        if let Err(error) = wait_for_command_complete_status_zero(
            &mut bluetooth,
            BLUETOOTH_HCI_OPCODE_SET_EVENT_MASK_PAGE_2,
            &mut read_buffer,
        ) {
            record_bluetooth_error(error);
            fatal_status(&display, STATUS_ERROR_BLUETOOTH_HCI_FEATURES);
        }
    }

    if write_le_host_supported_is_supported(&supported_commands.bytes) {
        DEBUG_CHANNEL_PHASE.store(25_1, Ordering::Release);
        if let Err(error) = send_hci_command(
            &mut bluetooth,
            BLUETOOTH_HCI_OPCODE_WRITE_LE_HOST_SUPPORT,
            &[1, 0],
            &mut write_scratch,
        ) {
            record_bluetooth_error(error);
            fatal_status(&display, STATUS_ERROR_BLUETOOTH_HCI_SEND);
        }
        DEBUG_CHANNEL_PHASE.store(25_2, Ordering::Release);
        if let Err(error) = wait_for_command_complete_status_zero(
            &mut bluetooth,
            BLUETOOTH_HCI_OPCODE_WRITE_LE_HOST_SUPPORT,
            &mut read_buffer,
        ) {
            record_bluetooth_error(error);
            fatal_status(&display, STATUS_ERROR_BLUETOOTH_HCI_LE_FEATURES);
        }
    }

    DEBUG_CHANNEL_PHASE.store(25_3, Ordering::Release);
    let le_event_mask = all_le_events_mask();
    if let Err(error) = send_hci_command(
        &mut bluetooth,
        BLUETOOTH_HCI_OPCODE_LE_SET_EVENT_MASK,
        &le_event_mask,
        &mut write_scratch,
    ) {
        record_bluetooth_error(error);
        fatal_status(&display, STATUS_ERROR_BLUETOOTH_HCI_SEND);
    }
    DEBUG_CHANNEL_PHASE.store(25_4, Ordering::Release);
    if let Err(error) = wait_for_command_complete_status_zero(
        &mut bluetooth,
        BLUETOOTH_HCI_OPCODE_LE_SET_EVENT_MASK,
        &mut read_buffer,
    ) {
        record_bluetooth_error(error);
        fatal_status(&display, STATUS_ERROR_BLUETOOTH_HCI_LE_FEATURES);
    }

    DEBUG_CHANNEL_PHASE.store(26, Ordering::Release);
    if let Err(error) = send_hci_command(
        &mut bluetooth,
        BLUETOOTH_HCI_OPCODE_READ_LOCAL_SUPPORTED_FEATURES,
        &[],
        &mut write_scratch,
    ) {
        record_bluetooth_error(error);
        fatal_status(&display, STATUS_ERROR_BLUETOOTH_HCI_SEND);
    }
    DEBUG_CHANNEL_PHASE.store(27, Ordering::Release);
    set_status(&display, STATUS_BLUETOOTH_HCI_FEATURES_SENT);

    DEBUG_CHANNEL_PHASE.store(28, Ordering::Release);
    let features = match wait_for_command_complete(
        &mut bluetooth,
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

    DEBUG_CHANNEL_PHASE.store(30, Ordering::Release);
    if let Err(error) = send_hci_command(
        &mut bluetooth,
        BLUETOOTH_HCI_OPCODE_LE_READ_LOCAL_SUPPORTED_FEATURES,
        &[],
        &mut write_scratch,
    ) {
        record_bluetooth_error(error);
        fatal_status(&display, STATUS_ERROR_BLUETOOTH_HCI_SEND);
    }
    DEBUG_CHANNEL_PHASE.store(31, Ordering::Release);
    set_status(&display, STATUS_BLUETOOTH_HCI_LE_FEATURES_SENT);

    DEBUG_CHANNEL_PHASE.store(32, Ordering::Release);
    let le_features = match wait_for_command_complete(
        &mut bluetooth,
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

    DEBUG_CHANNEL_PHASE.store(34, Ordering::Release);
    if let Err(error) = send_hci_command(
        &mut bluetooth,
        BLUETOOTH_HCI_OPCODE_READ_BUFFER_SIZE,
        &[],
        &mut write_scratch,
    ) {
        record_bluetooth_error(error);
        fatal_status(&display, STATUS_ERROR_BLUETOOTH_HCI_SEND);
    }
    DEBUG_CHANNEL_PHASE.store(35, Ordering::Release);
    set_status(&display, STATUS_BLUETOOTH_HCI_BUFFER_SIZE_SENT);

    DEBUG_CHANNEL_PHASE.store(36, Ordering::Release);
    let buffer_size = match wait_for_command_complete(
        &mut bluetooth,
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

    DEBUG_CHANNEL_PHASE.store(38, Ordering::Release);
    if let Err(error) = send_hci_command(
        &mut bluetooth,
        BLUETOOTH_HCI_OPCODE_LE_READ_BUFFER_SIZE,
        &[],
        &mut write_scratch,
    ) {
        record_bluetooth_error(error);
        fatal_status(&display, STATUS_ERROR_BLUETOOTH_HCI_SEND);
    }
    DEBUG_CHANNEL_PHASE.store(39, Ordering::Release);
    set_status(&display, STATUS_BLUETOOTH_HCI_LE_BUFFER_SIZE_SENT);

    DEBUG_CHANNEL_PHASE.store(40, Ordering::Release);
    let le_buffer_size = match wait_for_command_complete(
        &mut bluetooth,
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

    const LEGACY_ADVERTISING_DATA: [u8; 14] = [
        0x02, 0x01, 0x06, 0x0a, 0x09, b'F', b'u', b's', b'i', b'o', b'n', b' ', b'D', b'B',
    ];
    const LEGACY_SCAN_RESPONSE_DATA: [u8; 10] =
        [0x09, 0x09, b'P', b'i', b'c', b'o', b'2', b'W', b' ', b'B'];
    let advertising_parameters = BluetoothAdvertisingParameters {
        mode: BluetoothAdvertisingMode::ConnectableUndirected,
        connectable: true,
        scannable: true,
        discoverable: true,
        anonymous: false,
        interval_min_units: 0x0800,
        interval_max_units: 0x0800,
        primary_phy: BluetoothLePhy::Le1M,
        secondary_phy: None,
    };

    DEBUG_CHANNEL_LAST_BLUETOOTH_ADV_PARAM_CANDIDATE.store(1, Ordering::Release);
    DEBUG_CHANNEL_PHASE.store(47, Ordering::Release);
    set_status(&display, STATUS_BLUETOOTH_HCI_ADV_PARAMS_SENT);
    DEBUG_CHANNEL_PHASE.store(48, Ordering::Release);
    set_status(&display, STATUS_BLUETOOTH_HCI_ADV_PARAMS_COMPLETE);
    DEBUG_CHANNEL_PHASE.store(51, Ordering::Release);
    set_status(&display, STATUS_BLUETOOTH_HCI_ADV_DATA_SENT);
    DEBUG_CHANNEL_PHASE.store(53, Ordering::Release);
    set_status(&display, STATUS_BLUETOOTH_HCI_ADV_DATA_COMPLETE);
    DEBUG_CHANNEL_PHASE.store(55, Ordering::Release);
    set_status(&display, STATUS_BLUETOOTH_HCI_SCAN_RESPONSE_SENT);
    DEBUG_CHANNEL_PHASE.store(57, Ordering::Release);
    set_status(&display, STATUS_BLUETOOTH_HCI_SCAN_RESPONSE_COMPLETE);
    DEBUG_CHANNEL_PHASE.store(59, Ordering::Release);
    set_status(&display, STATUS_BLUETOOTH_HCI_ADV_ENABLE_SENT);
    if let Err(error) = bluetooth.start_advertising(
        advertising_parameters,
        &LEGACY_ADVERTISING_DATA,
        Some(&LEGACY_SCAN_RESPONSE_DATA),
    ) {
        record_bluetooth_error(error);
        match error.kind() {
            BluetoothErrorKind::Invalid => fatal_status(&display, STATUS_ERROR_BLUETOOTH_HCI_ADV_DATA),
            BluetoothErrorKind::Unsupported | BluetoothErrorKind::StateConflict => {
                fatal_status(&display, STATUS_ERROR_BLUETOOTH_HCI_ADV_PARAMS)
            }
            BluetoothErrorKind::Busy => fatal_status(&display, STATUS_ERROR_BLUETOOTH_HCI_ADV_ENABLE),
            _ => fatal_status(&display, STATUS_ERROR_BLUETOOTH_HCI_ADV_ENABLE),
        }
    }
    DEBUG_CHANNEL_PHASE.store(61, Ordering::Release);
    set_status(&display, STATUS_BLUETOOTH_HCI_ADV_ENABLE_COMPLETE);

    DEBUG_CHANNEL_PHASE.store(62, Ordering::Release);
    match bluetooth.is_powered() {
        Ok(true) => {
            DEBUG_CHANNEL_PHASE.store(63, Ordering::Release);
        }
        Ok(false) => fatal_status(&display, STATUS_ERROR_BLUETOOTH_HCI_ADV_ENABLE),
        Err(error) => {
            record_bluetooth_error(error);
            fatal_status(&display, STATUS_ERROR_BLUETOOTH_HCI_ADV_ENABLE)
        }
    }
    set_status(&display, STATUS_BLUETOOTH_HCI_ADVERTISING_ENABLED);

    DEBUG_CHANNEL_PHASE.store(0x40, Ordering::Release);
    loop {
        match bluetooth.wait_frame(Some(1_000)) {
            Ok(true) => match bluetooth.recv_frame(&mut read_buffer) {
                Ok(Some(frame)) => record_observed_hci_frame(frame),
                Ok(None) => {}
                Err(error) => record_bluetooth_error(error),
            },
            Ok(false) => {}
            Err(error) => record_bluetooth_error(error),
        }
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
