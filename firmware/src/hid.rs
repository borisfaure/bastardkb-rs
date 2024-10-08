use crate::device::is_host;
use crate::layout::LAYOUT_CHANNEL;
use defmt::*;
use embassy_executor::Spawner;
use embassy_rp::peripherals::USB;
use embassy_rp::usb::Driver;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};
use embassy_usb::class::hid::{ReportId, RequestHandler};
use embassy_usb::control::OutResponse;
use usbd_hid::descriptor::KeyboardReport;

/// Only one report is sent at a time
const NB_REPORTS: usize = 64;
/// Channel to send HID keyboard reports to the HID writer
pub static HID_KB_CHANNEL: Channel<CriticalSectionRawMutex, KeyboardReport, NB_REPORTS> =
    Channel::new();

/// HID writer type
pub type HidWriter<'a, 'b> = embassy_usb::class::hid::HidWriter<'a, Driver<'b, USB>, 64>;

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
            match writer.write_serialize(&hid_report).await {
                Ok(()) => {}
                Err(e) => warn!("Failed to send report: {:?}", e),
            }
        }
    }
}
