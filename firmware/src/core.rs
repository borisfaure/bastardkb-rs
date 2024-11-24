use crate::hid::{KeyboardReport, HID_KB_CHANNEL};
use crate::mouse::{MouseCommand, MOUSE_CMD_CHANNEL};
use crate::pmw3360::{SensorCommand, SENSOR_CMD_CHANNEL};
use crate::rgb_leds::{AnimCommand, ANIM_CHANNEL};
use crate::side::SIDE_CHANNEL;
use embassy_futures::select::{select, Either};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};
use embassy_time::{Duration, Ticker};
use keyberon::key_code::KeyCode;
use keyberon::layout::{CustomEvent as KbCustomEvent, Event as KBEvent, Layout};
use utils::serde::Event;

/// Basic layout for the keyboard
#[cfg(feature = "keymap_basic")]
use crate::keymap_basic::{KBLayout, LAYERS};

/// Keymap by Boris Faure
#[cfg(feature = "keymap_borisfaure")]
use crate::keymap_borisfaure::{KBLayout, LAYERS};

/// Test layout for the keyboard
#[cfg(feature = "keymap_test")]
use crate::keymap_test::{KBLayout, LAYERS};

/// Layout refresh rate, in ms
const REFRESH_RATE_MS: u64 = 1;
/// Number of events in the layout channel
const NB_EVENTS: usize = 64;
/// Channel to send `keyberon::layout::event` events to the layout handler
pub static LAYOUT_CHANNEL: Channel<CriticalSectionRawMutex, KBEvent, NB_EVENTS> = Channel::new();

/// Custom events for the layout, mostly mouse events
//#[allow(clippy::enum_variant_names)]
#[derive(Debug, PartialEq, defmt::Format)]
pub enum CustomEvent {
    /// Mouse left click
    MouseLeftClick,
    /// Mouse right click
    MouseRightClick,
    /// Mouse Wheel click
    MouseWheelClick,
    /// Ball is wheel
    BallIsWheel,
    /// Increase sensor CPI
    IncreaseCpi,
    /// Decrease sensor CPI
    DecreaseCpi,
    /// Next Animation of the RGB LEDs
    NextLedAnimation,
    /// Reset to usb mass storage
    ResetToUsbMassStorage,
}

pub struct Core {
    layout: KBLayout,
    current_layer: usize,
    kb_report: KeyboardReport,
    tick: u32,
}

impl Core {
    pub fn new() -> Self {
        Self {
            layout: Layout::new(&LAYERS),
            current_layer: 0,
            kb_report: KeyboardReport::default(),
            tick: 0,
        }
    }

    async fn tick(&mut self) {
        self.tick += 1;
        if self.tick % 2000 == 0 {
            defmt::info!("Tick: {} (every 2s?)", self.tick);
        }
        // Process all events in the channel if any
        while let Ok(event) = LAYOUT_CHANNEL.try_receive() {
            self.layout.event(event);
        }
        let custom_event = self.layout.tick();
        let new_layer = self.layout.current_layer();
        if new_layer != self.current_layer {
            defmt::info!("Layer: {}", new_layer);
            self.current_layer = new_layer;
            let layer = new_layer as u8;
            if SIDE_CHANNEL.is_full() {
                defmt::error!("Side channel is full");
            }
            SIDE_CHANNEL.send(Event::RgbAnimChangeLayer(layer)).await;
            if ANIM_CHANNEL.is_full() {
                defmt::error!("Anim channel is full");
            }
            ANIM_CHANNEL.send(AnimCommand::ChangeLayer(layer)).await;
        }

        self.process_custom_event(custom_event).await;
        let new_kb_report = generate_hid_kb_report(&mut self.layout);
        if new_kb_report != self.kb_report {
            self.kb_report = new_kb_report;
            if HID_KB_CHANNEL.is_full() {
                defmt::error!("HID KB channel is full");
            }
            HID_KB_CHANNEL.send(new_kb_report).await;
        }
    }

    /// Process a custom event from the layout
    async fn process_custom_event(&mut self, event: KbCustomEvent<CustomEvent>) {
        match event {
            KbCustomEvent::Press(CustomEvent::MouseLeftClick) => {
                if MOUSE_CMD_CHANNEL.is_full() {
                    defmt::error!("Mouse channel is full");
                }
                MOUSE_CMD_CHANNEL.send(MouseCommand::PressLeftClick).await;
            }
            KbCustomEvent::Release(CustomEvent::MouseLeftClick) => {
                if MOUSE_CMD_CHANNEL.is_full() {
                    defmt::error!("Mouse channel is full");
                }
                MOUSE_CMD_CHANNEL.send(MouseCommand::ReleaseLeftClick).await;
            }
            KbCustomEvent::Press(CustomEvent::MouseRightClick) => {
                if MOUSE_CMD_CHANNEL.is_full() {
                    defmt::error!("Mouse channel is full");
                }
                MOUSE_CMD_CHANNEL.send(MouseCommand::PressRightClick).await;
            }
            KbCustomEvent::Release(CustomEvent::MouseRightClick) => {
                if MOUSE_CMD_CHANNEL.is_full() {
                    defmt::error!("Mouse channel is full");
                }
                MOUSE_CMD_CHANNEL
                    .send(MouseCommand::ReleaseRightClick)
                    .await;
            }
            KbCustomEvent::Press(CustomEvent::MouseWheelClick) => {
                if MOUSE_CMD_CHANNEL.is_full() {
                    defmt::error!("Mouse channel is full");
                }
                MOUSE_CMD_CHANNEL.send(MouseCommand::PressWheelClick).await;
            }
            KbCustomEvent::Release(CustomEvent::MouseWheelClick) => {
                if MOUSE_CMD_CHANNEL.is_full() {
                    defmt::error!("Mouse channel is full");
                }
                MOUSE_CMD_CHANNEL
                    .send(MouseCommand::ReleaseWheelClick)
                    .await;
            }
            KbCustomEvent::Press(CustomEvent::BallIsWheel) => {
                if MOUSE_CMD_CHANNEL.is_full() {
                    defmt::error!("Mouse channel is full");
                }
                MOUSE_CMD_CHANNEL.send(MouseCommand::PressBallIsWheel).await;
            }
            KbCustomEvent::Release(CustomEvent::BallIsWheel) => {
                if MOUSE_CMD_CHANNEL.is_full() {
                    defmt::error!("Mouse channel is full");
                }
                MOUSE_CMD_CHANNEL
                    .send(MouseCommand::ReleaseBallIsWheel)
                    .await;
            }
            KbCustomEvent::Press(CustomEvent::IncreaseCpi) => {
                if SENSOR_CMD_CHANNEL.is_full() {
                    defmt::error!("Sensor channel is full");
                }
                SENSOR_CMD_CHANNEL.send(SensorCommand::IncreaseCpi).await;
            }
            KbCustomEvent::Release(CustomEvent::IncreaseCpi) => {}
            KbCustomEvent::Press(CustomEvent::DecreaseCpi) => {
                if SENSOR_CMD_CHANNEL.is_full() {
                    defmt::error!("Sensor channel is full");
                }
                SENSOR_CMD_CHANNEL.send(SensorCommand::DecreaseCpi).await;
            }
            KbCustomEvent::Release(CustomEvent::DecreaseCpi) => {}

            KbCustomEvent::Press(CustomEvent::NextLedAnimation) => {
                if ANIM_CHANNEL.is_full() {
                    defmt::error!("Anim channel is full");
                }
                ANIM_CHANNEL.send(AnimCommand::Next).await;
            }
            KbCustomEvent::Release(CustomEvent::NextLedAnimation) => {}

            KbCustomEvent::Press(CustomEvent::ResetToUsbMassStorage) => {
                embassy_rp::rom_data::reset_to_usb_boot(0, 0);
            }
            KbCustomEvent::Release(CustomEvent::ResetToUsbMassStorage) => {}

            KbCustomEvent::NoEvent => (),
        }
    }

    /// Keyboard layout handler
    /// Handles layout events into the keymap and sends HID reports to the HID handler
    pub async fn run(&mut self) {
        let mut ticker = Ticker::every(Duration::from_millis(REFRESH_RATE_MS));
        loop {
            match select(ticker.next(), LAYOUT_CHANNEL.receive()).await {
                Either::First(_) => {
                    self.tick().await;
                }
                Either::Second(event) => {
                    self.layout.event(event);
                }
            };
        }
    }
}

/// Set a report as an error based on keycode `kc`
fn keyboard_report_set_error(report: &mut KeyboardReport, kc: KeyCode) {
    report.modifier = 0;
    report.keycodes = [kc as u8; 6];
    defmt::error!("Error: {:?}", defmt::Debug2Format(&kc));
}

/// Generate a HID report from the current layout
fn generate_hid_kb_report(layout: &mut KBLayout) -> KeyboardReport {
    let mut report = KeyboardReport::default();
    for kc in layout.keycodes() {
        use keyberon::key_code::KeyCode::*;
        match kc {
            No => (),
            ErrorRollOver | PostFail | ErrorUndefined => keyboard_report_set_error(&mut report, kc),
            kc if kc.is_modifier() => report.modifier |= kc.as_modifier_bit(),
            _ => report.keycodes[..]
                .iter_mut()
                .find(|c| **c == 0)
                .map(|c| *c = kc as u8)
                .unwrap_or_else(|| keyboard_report_set_error(&mut report, ErrorRollOver)),
        }
    }
    report
}
