use crate::hid::{ConsumerReport, KeyboardReport, HID_CONSUMER_CHANNEL, HID_KB_CHANNEL};
use crate::mouse::MouseHandler;
use crate::rgb_leds::{AnimCommand, ANIM_CHANNEL};
use crate::side::SIDE_CHANNEL;
#[cfg(feature = "cnano")]
use crate::trackball::{SensorCommand, SENSOR_CMD_CHANNEL};
#[cfg(feature = "defmt")]
use defmt::Debug2Format;
use embassy_futures::select::{select, Either};
use embassy_rp::peripherals::USB;
use embassy_rp::usb::Driver;
use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, channel::Channel};
use embassy_time::{Duration, Ticker};
use embassy_usb::class::hid::HidWriter;
use keyberon::key_code::KeyCode;
use keyberon::layout::{CustomEvent as KbCustomEvent, Event as KBEvent, Layout};
use utils::log::{error, info};
use utils::serde::Event;

/// Basic layout for the keyboard
#[cfg(feature = "keymap_basic")]
use crate::keymap_basic::{KBLayout, LAYERS, VIRTUAL_MOUSE_KEY};

/// Keymap by Boris Faure
#[cfg(feature = "keymap_borisfaure")]
use crate::keymap_borisfaure::{KBLayout, LAYERS, VIRTUAL_MOUSE_KEY};

/// Test layout for the keyboard
#[cfg(feature = "keymap_test")]
use crate::keymap_test::{KBLayout, LAYERS, VIRTUAL_MOUSE_KEY};

/// Layout refresh rate, in ms
const REFRESH_RATE_MS: u64 = 1;
/// Number of events in the layout channel
const NB_EVENTS: usize = 128;
/// Channel to send `keyberon::layout::event` events to the layout handler
pub static LAYOUT_CHANNEL: Channel<ThreadModeRawMutex, KBEvent, NB_EVENTS> = Channel::new();

/// Custom events for the layout, mostly mouse events
//#[allow(clippy::enum_variant_names)]
#[derive(Debug, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
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
    #[cfg(feature = "cnano")]
    IncreaseCpi,
    /// Decrease sensor CPI
    #[cfg(feature = "cnano")]
    DecreaseCpi,
    /// Next Animation of the RGB LEDs
    NextLedAnimation,
    /// Reset to usb mass storage
    ResetToUsbMassStorage,
    /// Wheel up
    #[cfg(feature = "dilemma")]
    WheelUp,
    /// Wheel down
    #[cfg(feature = "dilemma")]
    WheelDown,
    /// Stop the automouse feature
    NoMouseAction,
}

/// Timeout for the automouse feature: when the mouse is not used for this
/// amount of time, it will be considered inactive.
#[cfg(feature = "dilemma")]
const AUTO_MOUSE_TIMEOUT: usize = 10;
#[cfg(feature = "cnano")]
const AUTO_MOUSE_TIMEOUT: usize = 10;

/// Core keyboard/mouse handler
pub struct Core<'a> {
    /// Keyboard layout
    layout: KBLayout,
    /// Current layer
    current_layer: usize,
    /// Keyboard HID report
    kb_report: KeyboardReport,
    /// Consumer Control HID report
    consumer_report: ConsumerReport,
    /// Mouse handler
    mouse: MouseHandler,
    /// HID mouse writer
    hid_mouse_writer: HidWriter<'a, Driver<'a, USB>, 7>,
    /// Timeout for the automouse feature. When this is non-zero, the mouse
    /// will be considered active. Goes down to 0 every tick.
    auto_mouse_timeout: usize,
    /// Current color layer
    color_layer: u8,
    /// Is mouse active
    mouse_active: bool,
}

impl<'a> Core<'a> {
    /// Create a new core
    pub fn new(hid_mouse_writer: HidWriter<'a, Driver<'a, USB>, 7>) -> Self {
        Self {
            layout: Layout::new(&LAYERS),
            current_layer: 0,
            kb_report: KeyboardReport::default(),
            consumer_report: ConsumerReport::default(),
            mouse: MouseHandler::new(),
            hid_mouse_writer,
            auto_mouse_timeout: 0,
            color_layer: 0,
            mouse_active: false,
        }
    }

    /// Set the color layer of the RGB LEDs
    async fn set_color_layer(&mut self, layer: u8) {
        if self.color_layer != layer {
            info!("Setting color layer to {}", layer);
            self.color_layer = layer;
            if SIDE_CHANNEL.is_full() {
                error!("Side channel is full");
            }
            SIDE_CHANNEL.send(Event::RgbAnimChangeLayer(layer)).await;
            if ANIM_CHANNEL.is_full() {
                error!("Anim channel is full");
            }
            ANIM_CHANNEL.send(AnimCommand::ChangeLayer(layer)).await;
        }
    }

    /// (Re)Set mouse active timeout
    /// Also set the leds to the mouse active color
    async fn on_mouse_active(&mut self) {
        if !self.mouse_active {
            self.mouse_active = true;
            info!("Set Mouse Active");
            self.layout
                .event(KBEvent::Press(VIRTUAL_MOUSE_KEY.0, VIRTUAL_MOUSE_KEY.1));
            self.auto_mouse_timeout = AUTO_MOUSE_TIMEOUT;
        }
    }

    /// When the mouse becomes inactive, reset the leds to the current layer
    /// color
    async fn on_mouse_inactive(&mut self) {
        info!("On Mouse Inactive");
        if self.mouse_active {
            self.mouse_active = false;
            info!("Set Mouse Inactive");
            self.layout
                .event(KBEvent::Release(VIRTUAL_MOUSE_KEY.0, VIRTUAL_MOUSE_KEY.1));
        }
    }

    /// Process a key event
    async fn on_key_event(&mut self, event: KBEvent) {
        self.layout.event(event);
    }

    /// Process the state of the keyboard and mouse
    async fn tick(&mut self) {
        // Process all mouse events first since they are time sensitive
        while let Some(mouse_report) = self.mouse.tick().await {
            let pending_mouse_clicks = mouse_report.buttons != 0;
            // Don't consider wheel movement as mouse activity since it may
            // just be scrolling and not actual mouse movement
            let mouse_moved = mouse_report.x != 0 || mouse_report.y != 0;
            let raw = mouse_report.serialize();
            #[cfg(feature = "defmt")]
            if let Err(e) = self.hid_mouse_writer.write(&raw).await {
                error!("Failed to send mouse report: {:?}", e);
            }
            let _ = self.hid_mouse_writer.write(&raw).await;
            if mouse_moved || pending_mouse_clicks {
                self.auto_mouse_timeout = AUTO_MOUSE_TIMEOUT;
                self.on_mouse_active().await;
            }
        }
        if self.auto_mouse_timeout > 0 {
            self.auto_mouse_timeout -= 1;
            if self.auto_mouse_timeout == 0 {
                self.on_mouse_inactive().await;
            }
        }

        // Process all events in the layout channel if any
        // This is where the keymap is processed
        while let Ok(event) = LAYOUT_CHANNEL.try_receive() {
            self.on_key_event(event).await;
        }
        let custom_event = self.layout.tick();
        let new_layer = self.layout.current_layer();
        self.process_custom_event(custom_event).await;
        let (new_kb_report, new_consumer_report) = generate_hid_reports(&mut self.layout);
        if new_kb_report != self.kb_report {
            self.kb_report = new_kb_report;
            if HID_KB_CHANNEL.is_full() {
                error!("HID KB channel is full");
            }
            HID_KB_CHANNEL.send(new_kb_report).await;
        }
        if new_consumer_report != self.consumer_report {
            self.consumer_report = new_consumer_report;
            if HID_CONSUMER_CHANNEL.is_full() {
                error!("HID Consumer channel is full");
            }
            HID_CONSUMER_CHANNEL.send(new_consumer_report).await;
        }
        if new_layer != self.current_layer {
            info!("Layer: {}", new_layer);
            self.current_layer = new_layer;
            self.set_color_layer(new_layer as u8).await;
        }
    }

    /// Process a custom event from the layout
    async fn process_custom_event(&mut self, event: KbCustomEvent<CustomEvent>) {
        match event {
            KbCustomEvent::Press(CustomEvent::MouseLeftClick) => {
                self.mouse.on_left_click(true);
            }
            KbCustomEvent::Release(CustomEvent::MouseLeftClick) => {
                self.mouse.on_left_click(false);
            }
            KbCustomEvent::Press(CustomEvent::MouseRightClick) => {
                self.mouse.on_right_click(true);
            }
            KbCustomEvent::Release(CustomEvent::MouseRightClick) => {
                self.mouse.on_right_click(false);
            }
            KbCustomEvent::Press(CustomEvent::MouseWheelClick) => {
                self.mouse.on_middle_click(true);
            }
            KbCustomEvent::Release(CustomEvent::MouseWheelClick) => {
                self.mouse.on_middle_click(false);
            }
            KbCustomEvent::Press(CustomEvent::BallIsWheel) => {
                self.mouse.on_ball_is_wheel(true);
            }
            KbCustomEvent::Release(CustomEvent::BallIsWheel) => {
                self.mouse.on_ball_is_wheel(false);
            }
            #[cfg(feature = "dilemma")]
            KbCustomEvent::Press(CustomEvent::WheelUp) => {
                self.mouse.on_wheel(true);
            }
            #[cfg(feature = "dilemma")]
            KbCustomEvent::Release(CustomEvent::WheelUp) => {}
            #[cfg(feature = "dilemma")]
            KbCustomEvent::Press(CustomEvent::WheelDown) => {
                self.mouse.on_wheel(false);
            }
            #[cfg(feature = "dilemma")]
            KbCustomEvent::Release(CustomEvent::WheelDown) => {}

            #[cfg(feature = "cnano")]
            KbCustomEvent::Press(CustomEvent::IncreaseCpi) => {
                if SENSOR_CMD_CHANNEL.is_full() {
                    error!("Sensor channel is full");
                }
                SENSOR_CMD_CHANNEL.send(SensorCommand::IncreaseCpi).await;
            }
            #[cfg(feature = "cnano")]
            KbCustomEvent::Release(CustomEvent::IncreaseCpi) => {}
            #[cfg(feature = "cnano")]
            KbCustomEvent::Press(CustomEvent::DecreaseCpi) => {
                if SENSOR_CMD_CHANNEL.is_full() {
                    error!("Sensor channel is full");
                }
                SENSOR_CMD_CHANNEL.send(SensorCommand::DecreaseCpi).await;
            }
            #[cfg(feature = "cnano")]
            KbCustomEvent::Release(CustomEvent::DecreaseCpi) => {}

            KbCustomEvent::Press(CustomEvent::NextLedAnimation) => {
                if ANIM_CHANNEL.is_full() {
                    error!("Anim channel is full");
                }
                ANIM_CHANNEL.send(AnimCommand::Next).await;
            }
            KbCustomEvent::Release(CustomEvent::NextLedAnimation) => {}

            KbCustomEvent::Press(CustomEvent::ResetToUsbMassStorage) => {
                embassy_rp::rom_data::reset_to_usb_boot(0, 0);
            }
            KbCustomEvent::Release(CustomEvent::ResetToUsbMassStorage) => {}

            KbCustomEvent::Press(CustomEvent::NoMouseAction) => {
                if self.auto_mouse_timeout != 0 {
                    self.auto_mouse_timeout = 0;
                    self.on_mouse_inactive().await;
                }
            }
            KbCustomEvent::Release(CustomEvent::NoMouseAction) => {}

            KbCustomEvent::NoEvent => (),
        }
    }
}

#[embassy_executor::task]
/// Keyboard layout handler
/// Handles layout events into the keymap and sends HID reports to the HID handler
pub async fn run(mut core: Core<'static>) {
    let mut ticker = Ticker::every(Duration::from_millis(REFRESH_RATE_MS));

    loop {
        match select(ticker.next(), LAYOUT_CHANNEL.receive()).await {
            Either::First(_) => {
                core.tick().await;
            }
            Either::Second(event) => {
                core.on_key_event(event).await;
            }
        };
    }
}

/// Set a report as an error based on keycode `kc`
fn keyboard_report_set_error(report: &mut KeyboardReport, kc: KeyCode) {
    report.modifier = 0;
    report.keycodes = [kc as u8; 6];
    error!("Error: {:?}", Debug2Format(&kc));
}

/// Generate HID reports (keyboard and consumer) from the current layout
fn generate_hid_reports(layout: &mut KBLayout) -> (KeyboardReport, ConsumerReport) {
    let mut kb_report = KeyboardReport::default();
    let mut consumer_report = ConsumerReport::default();

    for kc in layout.keycodes() {
        use keyberon::key_code::KeyCode::*;
        match kc {
            No => (),
            ErrorRollOver | PostFail | ErrorUndefined => {
                keyboard_report_set_error(&mut kb_report, kc)
            }
            kc if kc.is_modifier() => kb_report.modifier |= kc.as_modifier_bit(),
            // Consumer control keys (>= 0xE8)
            // Map them to consumer usage codes
            MediaNextSong => consumer_report.usage = 0x00B5,
            MediaPreviousSong => consumer_report.usage = 0x00B6,
            MediaPlayPause => consumer_report.usage = 0x00CD,
            Mute => consumer_report.usage = 0x00E2,
            VolUp => consumer_report.usage = 0x00E9,
            VolDown => consumer_report.usage = 0x00EA,
            // Regular keyboard keys (< 0xE8)
            _ => {
                // Only add to keyboard report if it's a valid keycode < 0xE8
                let kc_value = kc as u8;
                if kc_value < 0xE8 {
                    kb_report.keycodes[..]
                        .iter_mut()
                        .find(|c| **c == 0)
                        .map(|c| *c = kc_value)
                        .unwrap_or_else(|| {
                            keyboard_report_set_error(&mut kb_report, ErrorRollOver)
                        });
                }
            }
        }
    }
    (kb_report, consumer_report)
}
