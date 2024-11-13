use crate::device::is_host;
use crate::layout::LAYOUT_CHANNEL;
use defmt::*;
use embassy_executor::Spawner;
use embassy_rp::peripherals::USB;
use embassy_rp::usb::Driver;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};
use embassy_usb::class::hid::{ReportId, RequestHandler};
use embassy_usb::control::OutResponse;

/// Only one report is sent at a time
const NB_REPORTS: usize = 64;
/// Channel to send HID keyboard reports to the HID writer
pub static HID_KB_CHANNEL: Channel<CriticalSectionRawMutex, KeyboardReport, NB_REPORTS> =
    Channel::new();

/// HID writer type
pub type HidWriter<'a, 'b> = embassy_usb::class::hid::HidWriter<'a, Driver<'b, USB>, 64>;

#[rustfmt::skip]
/// Keyboard HID report descriptor
pub const KB_REPORT_DESCRIPTOR: &[u8] = &[
    0x05, 0x01,        // Usage Page (Generic Desktop Ctrls)
    0x09, 0x06,        // Usage (Keyboard)
    0xA1, 0x01,        // Collection (Application)
    0x05, 0x07,        //   Usage Page (Kbrd/Keypad)
    0x19, 0xE0,        //   Usage Minimum (0xE0)
    0x29, 0xE7,        //   Usage Maximum (0xE7)
    0x15, 0x00,        //   Logical Minimum (0)
    0x25, 0x01,        //   Logical Maximum (1)
    0x75, 0x01,        //   Report Size (1)
    0x95, 0x08,        //   Report Count (8)
    0x81, 0x02,        //   Input (Data,Var,Abs,No Wrap,Linear,Preferred State,No Null Position)
    0x19, 0x00,        //   Usage Minimum (0x00)
    0x29, 0xFF,        //   Usage Maximum (0xFF)
    0x26, 0xFF, 0x00,  //   Logical Maximum (255)
    0x75, 0x08,        //   Report Size (8)
    0x95, 0x01,        //   Report Count (1)
    0x81, 0x03,        //   Input (Const,Var,Abs,No Wrap,Linear,Preferred State,No Null Position)
    0x05, 0x08,        //   Usage Page (LEDs)
    0x19, 0x01,        //   Usage Minimum (Num Lock)
    0x29, 0x05,        //   Usage Maximum (Kana)
    0x25, 0x01,        //   Logical Maximum (1)
    0x75, 0x01,        //   Report Size (1)
    0x95, 0x05,        //   Report Count (5)
    0x91, 0x02,        //   Output (Data,Var,Abs,No Wrap,Linear,Preferred State,No Null Position,Non-volatile)
    0x95, 0x03,        //   Report Count (3)
    0x91, 0x03,        //   Output (Const,Var,Abs,No Wrap,Linear,Preferred State,No Null Position,Non-volatile)
    0x05, 0x07,        //   Usage Page (Kbrd/Keypad)
    0x19, 0x00,        //   Usage Minimum (0x00)
    0x29, 0xDD,        //   Usage Maximum (0xDD)
    0x26, 0xFF, 0x00,  //   Logical Maximum (255)
    0x75, 0x08,        //   Report Size (8)
    0x95, 0x06,        //   Report Count (6)
    0x81, 0x00,        //   Input (Data,Array,Abs,No Wrap,Linear,Preferred State,No Null Position)
    0xC0,              // End Collection
// 69 bytes
];

#[rustfmt::skip]
/// Mouse HID report descriptor
pub const MOUSE_REPORT_DESCRIPTOR: &[u8] = &[
    0x05, 0x01,        // Usage Page (Generic Desktop Ctrls)
    0x09, 0x02,        // Usage (Mouse)
    0xA1, 0x01,        // Collection (Application)
    0x09, 0x01,        //   Usage (Pointer)
    0xA1, 0x00,        //   Collection (Physical)
    0x05, 0x09,        //     Usage Page (Button)
    0x19, 0x01,        //     Usage Minimum (0x01)
    0x29, 0x05,        //     Usage Maximum (0x05)
    0x15, 0x00,        //     Logical Minimum (0)
    0x25, 0x01,        //     Logical Maximum (1)
    0x95, 0x05,        //     Report Count (5)
    0x75, 0x01,        //     Report Size (1)
    0x81, 0x02,        //     Input (Data,Var,Abs,No Wrap,Linear,Preferred State,No Null Position)
    0x95, 0x01,        //     Report Count (1)
    0x75, 0x03,        //     Report Size (3)
    0x81, 0x01,        //     Input (Const,Array,Abs,No Wrap,Linear,Preferred State,No Null Position)
    0x05, 0x01,        //     Usage Page (Generic Desktop Ctrls)
    0x09, 0x30,        //     Usage (X)
    0x09, 0x31,        //     Usage (Y)
    0x16, 0x00, 0x80,  //     Logical Minimum (-32768)
    0x26, 0xFF, 0x7F,  //     Logical Maximum (32767)
    0x75, 0x10,        //     Report Size (16)
    0x95, 0x02,        //     Report Count (2)
    0x81, 0x06,        //     Input (Data,Var,Rel,No Wrap,Linear,Preferred State,No Null Position)
    0xC0,              //   End Collection
    0xA1, 0x00,        //   Collection (Physical)
    0x05, 0x01,        //     Usage Page (Generic Desktop Ctrls)
    0x09, 0x38,        //     Usage (Wheel)
    0x15, 0x81,        //     Logical Minimum (-127)
    0x25, 0x7F,        //     Logical Maximum (127)
    0x75, 0x08,        //     Report Size (8)
    0x95, 0x01,        //     Report Count (1)
    0x81, 0x06,        //     Input (Data,Var,Rel,No Wrap,Linear,Preferred State,No Null Position)
    0xC0,              //   End Collection
    0xA1, 0x00,        //   Collection (Physical)
    0x05, 0x0C,        //     Usage Page (Consumer)
    0x0A, 0x38, 0x02,  //     Usage (AC Pan)
    0x95, 0x01,        //     Report Count (1)
    0x75, 0x08,        //     Report Size (8)
    0x15, 0x81,        //     Logical Minimum (-127)
    0x25, 0x7F,        //     Logical Maximum (127)
    0x81, 0x06,        //     Input (Data,Var,Rel,No Wrap,Linear,Preferred State,No Null Position)
    0xC0,              //   End Collection
    0xC0,              // End Collection
// 87 bytes
];

/// Keyboard HID report
#[derive(Debug, Default, PartialEq, Clone, Copy, defmt::Format)]
pub struct KeyboardReport {
    /// Modifier keys, in the following order (from least significant bit):
    /// - Left Control
    /// - Left Shift
    /// - Left Alt
    /// - Left GUI
    /// - Right Control
    /// - Right Shift
    /// - Right Alt
    /// - Right GUI
    pub modifier: u8,
    /// Keycodes for up to 6 simultaneously pressed keys
    pub keycodes: [u8; 6],
}

impl KeyboardReport {
    /// Serialize the report
    pub fn serialize(&self) -> [u8; 8] {
        [
            self.modifier,
            0u8,
            self.keycodes[0],
            self.keycodes[1],
            self.keycodes[2],
            self.keycodes[3],
            self.keycodes[4],
            self.keycodes[5],
        ]
    }
}

/// Mouse HID report
#[derive(Debug, Default, PartialEq, Clone, Copy, defmt::Format)]
pub struct MouseReport {
    /// Buttons state
    /// Button 1 to 8 where Button1 is the LSB
    pub buttons: u8,
    /// x movement
    pub x: i16,
    /// y movement
    pub y: i16,
    /// Scroll down (negative) or up (positive) this many units
    pub wheel: i8,
    /// Scroll left (negative) or right (positive) this many units
    pub pan: i8,
}

impl MouseReport {
    /// Serialize the report
    pub fn serialize(&self) -> [u8; 7] {
        let x = self.x.to_le_bytes();
        let y = self.y.to_le_bytes();
        [
            self.buttons,
            x[0],
            x[1],
            y[0],
            y[1],
            self.wheel as u8,
            self.pan as u8,
        ]
    }
}

/// HID handler
pub struct HidRequestHandler<'a> {
    /// Spawner
    spawner: &'a Spawner,
    /// Num lock state
    num_lock: bool,
    /// Caps lock state
    caps_lock: bool,
}
impl<'a> HidRequestHandler<'a> {
    /// Create a new HID request handler
    pub fn new(spawner: &'a Spawner) -> Self {
        HidRequestHandler {
            spawner,
            num_lock: false,
            caps_lock: false,
        }
    }
}

impl RequestHandler for HidRequestHandler<'_> {
    fn get_report(&mut self, id: ReportId, _buf: &mut [u8]) -> Option<usize> {
        info!("Get report for {:?}", id);
        None
    }

    fn set_report(&mut self, id: ReportId, data: &[u8]) -> OutResponse {
        info!("Set report for {:?}: {=[u8]}", id, data);
        if let ReportId::Out(0) = id {
            self.num_lock(data[0] & 1 != 0);
            self.caps_lock(data[0] & 1 << 1 != 0);
        }
        OutResponse::Accepted
    }

    fn set_idle_ms(&mut self, id: Option<ReportId>, dur: u32) {
        info!("Set idle rate for {:?} to {:?}", id, dur);
    }

    fn get_idle_ms(&mut self, id: Option<ReportId>) -> Option<u32> {
        info!("Get idle rate for {:?}", id);
        None
    }
}

#[embassy_executor::task]
async fn caps_lock_change() {
    // send a key press and release event for the CapsLock key so that
    // the keymap can do something with it, like changing the default layer
    LAYOUT_CHANNEL
        .send(keyberon::layout::Event::Press(3, 4))
        .await;
    LAYOUT_CHANNEL
        .send(keyberon::layout::Event::Release(3, 4))
        .await;
}
#[embassy_executor::task]
async fn num_lock_change() {
    // send a key press and release event for the NumLock key so that
    // the keymap can do something with it, like changing the default layer
    LAYOUT_CHANNEL
        .send(keyberon::layout::Event::Press(3, 1))
        .await;
    LAYOUT_CHANNEL
        .send(keyberon::layout::Event::Release(3, 1))
        .await;
}

impl HidRequestHandler<'_> {
    /// Set the caps lock state. May not have changed.
    fn caps_lock(&mut self, caps_lock: bool) {
        if self.caps_lock != caps_lock {
            self.caps_lock = caps_lock;
            self.spawner.must_spawn(caps_lock_change());
        }
    }
    /// Set the num lock state. May not have changed.
    fn num_lock(&mut self, num_lock: bool) {
        if self.num_lock != num_lock {
            self.num_lock = num_lock;
            self.spawner.must_spawn(num_lock_change());
        }
    }
}

/// Loop to read HID KeyboardReport reports from the channel and send them over USB
pub async fn hid_kb_writer_handler<'a>(mut writer: HidWriter<'a, 'a>) {
    loop {
        let hid_report = HID_KB_CHANNEL.receive().await;
        if is_host() {
            let raw = hid_report.serialize();
            match writer.write(&raw).await {
                Ok(()) => {}
                Err(e) => warn!("Failed to send report: {:?}", e),
            }
        }
    }
}
