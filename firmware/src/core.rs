use crate::hid::{KeyboardReport, HID_KB_CHANNEL};
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
use utils::rgb_anims::MOUSE_COLOR_INDEX;
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
}

/// Timeout for the automouse feature: when the mouse is not used for this
/// amount of time, it will be considered inactive.
const AUTO_MOUSE_TIMEOUT: usize = 1000;

/// Delay in ticks before sending a left or right click in auto mouse mode.
/// If the other button is pressed during this delay, a middle click is sent instead.
const AUTO_MOUSE_CLICK_DELAY: usize = 5;

/// Check if the key event is a left click
fn event_is_left_click(i: u8, j: u8) -> bool {
    i == 3 && j == 5
}
/// Check if the key event is a right click
fn event_is_right_click(i: u8, j: u8) -> bool {
    i == 3 && j == 6
}

/// Core keyboard/mouse handler
pub struct Core<'a> {
    /// Keyboard layout
    layout: KBLayout,
    /// Current layer
    current_layer: usize,
    /// Keyboard HID report
    kb_report: KeyboardReport,
    /// Mouse handler
    mouse: MouseHandler,
    /// HID mouse writer
    hid_mouse_writer: HidWriter<'a, Driver<'a, USB>, 7>,
    /// Timeout for the automouse feature. When this is non-zero, the mouse
    /// will be considered active. Goes down to 0 every tick.
    auto_mouse_timeout: usize,
    /// Need to filter next left click release
    filter_left_button_release: bool,
    /// Need to filter next right click release
    filter_right_button_release: bool,
    /// Current color layer
    color_layer: u8,
    /// Pending left click: counts down to 0, then sends the click
    pending_left_click: usize,
    /// Pending right click: counts down to 0, then sends the click
    pending_right_click: usize,
}

impl<'a> Core<'a> {
    /// Create a new core
    pub fn new(hid_mouse_writer: HidWriter<'a, Driver<'a, USB>, 7>) -> Self {
        Self {
            layout: Layout::new(&LAYERS),
            current_layer: 0,
            kb_report: KeyboardReport::default(),
            mouse: MouseHandler::new(),
            hid_mouse_writer,
            auto_mouse_timeout: 0,
            filter_left_button_release: false,
            filter_right_button_release: false,
            color_layer: 0,
            pending_left_click: 0,
            pending_right_click: 0,
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
        self.auto_mouse_timeout = AUTO_MOUSE_TIMEOUT;
        self.set_color_layer(MOUSE_COLOR_INDEX).await;
    }

    /// When the mouse becomes inactive, reset the leds to the current layer
    /// color
    async fn on_mouse_inactive(&mut self) {
        info!("Mouse inactive");
        self.set_color_layer(self.current_layer as u8).await;
    }

    /// Process a key event
    async fn on_key_event(&mut self, event: KBEvent) {
        let (i, j) = event.coord();
        let is_press = event.is_press();
        info!(
            "Key event: {:?} {:?} auto_mouse_timeout: {}, filter_left_button_release: {}, filter_right_button_release: {}, pending_left: {}, pending_right: {}",
            is_press,
            (i, j),
            self.auto_mouse_timeout,
            self.filter_left_button_release,
            self.filter_right_button_release,
            self.pending_left_click,
            self.pending_right_click
        );
        if self.auto_mouse_timeout > 0 {
            if event_is_left_click(i, j) {
                if is_press {
                    // Left click pressed
                    if self.pending_right_click > 0 {
                        // Right click is pending, send middle click instead
                        info!("Sending middle click (left pressed while right pending)");
                        self.mouse.on_middle_click(true);
                        self.pending_right_click = 0;
                        self.filter_left_button_release = true;
                        self.filter_right_button_release = true;
                    } else {
                        // Start pending left click
                        self.pending_left_click = AUTO_MOUSE_CLICK_DELAY;
                        self.filter_left_button_release = true;
                    }
                    self.on_mouse_active().await;
                } else {
                    // Left click released
                    if self.pending_left_click > 0 {
                        // Release before delay expired, cancel pending click
                        self.pending_left_click = 0;
                    } else {
                        // Release actual left click
                        self.mouse.on_left_click(false);
                    }
                    // Release middle click if it was pressed
                    self.mouse.on_middle_click(false);
                }
            } else if event_is_right_click(i, j) {
                if is_press {
                    // Right click pressed
                    if self.pending_left_click > 0 {
                        // Left click is pending, send middle click instead
                        info!("Sending middle click (right pressed while left pending)");
                        self.mouse.on_middle_click(true);
                        self.pending_left_click = 0;
                        self.filter_left_button_release = true;
                        self.filter_right_button_release = true;
                    } else {
                        // Start pending right click
                        self.pending_right_click = AUTO_MOUSE_CLICK_DELAY;
                        self.filter_right_button_release = true;
                    }
                    self.on_mouse_active().await;
                } else {
                    // Right click released
                    if self.pending_right_click > 0 {
                        // Release before delay expired, cancel pending click
                        self.pending_right_click = 0;
                    } else {
                        // Release actual right click
                        self.mouse.on_right_click(false);
                    }
                    // Release middle click if it was pressed
                    self.mouse.on_middle_click(false);
                }
            } else {
                // Not a mouse event, process it as a keyboard event
                // and consider the mouse as inactive
                self.auto_mouse_timeout = 0;
                self.pending_left_click = 0;
                self.pending_right_click = 0;
                self.layout.event(event);
            }
        } else if self.filter_left_button_release && !is_press && event_is_left_click(i, j) {
            self.filter_left_button_release = false;
        } else if self.filter_right_button_release && !is_press && event_is_right_click(i, j) {
            self.filter_right_button_release = false;
        } else {
            self.layout.event(event);
        }
    }

    /// Process the state of the keyboard and mouse
    async fn tick(&mut self) {
        // Process pending click timeouts
        if self.pending_left_click > 0 {
            self.pending_left_click -= 1;
            if self.pending_left_click == 0 {
                // Delay expired, send the left click
                info!("Sending delayed left click");
                self.mouse.on_left_click(true);
            }
        }
        if self.pending_right_click > 0 {
            self.pending_right_click -= 1;
            if self.pending_right_click == 0 {
                // Delay expired, send the right click
                info!("Sending delayed right click");
                self.mouse.on_right_click(true);
            }
        }

        // Process all mouse events first since they are time sensitive
        while let Some(mouse_report) = self.mouse.tick().await {
            let raw = mouse_report.serialize();
            #[cfg(feature = "defmt")]
            if let Err(e) = self.hid_mouse_writer.write(&raw).await {
                error!("Failed to send mouse report: {:?}", e);
            }
            #[cfg(not(feature = "defmt"))]
            let _ = self.hid_mouse_writer.write(&raw).await;
            self.on_mouse_active().await;
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
        let new_kb_report = generate_hid_kb_report(&mut self.layout);
        if new_kb_report != self.kb_report {
            self.kb_report = new_kb_report;
            if HID_KB_CHANNEL.is_full() {
                error!("HID KB channel is full");
            }
            HID_KB_CHANNEL.send(new_kb_report).await;
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
