use crate::hid::{KeyboardReport, HID_KB_CHANNEL};
use crate::mouse::MouseHandler;
use crate::rgb_leds::{AnimCommand, ANIM_CHANNEL};
use crate::side::SIDE_CHANNEL;
#[cfg(feature = "cnano")]
use crate::trackball::{SensorCommand, SENSOR_CMD_CHANNEL};
use embassy_futures::select::{select, Either};
use embassy_rp::peripherals::USB;
use embassy_rp::usb::Driver;
use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, channel::Channel};
#[cfg(feature = "timing_logs")]
use embassy_time::Instant;
use embassy_time::{Duration, Ticker};
use embassy_usb::class::hid::HidWriter;
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
const NB_EVENTS: usize = 8;
/// Channel to send `keyberon::layout::event` events to the layout handler
pub static LAYOUT_CHANNEL: Channel<ThreadModeRawMutex, KBEvent, NB_EVENTS> = Channel::new();

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

/// Debug tick counter: every 5s
#[cfg(feature = "debug_tick")]
const TICK_DEBUG: usize = 5000;

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
    /// Debug tick counter
    #[cfg(feature = "debug_tick")]
    debug_tick: usize,
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
            #[cfg(feature = "debug_tick")]
            debug_tick: TICK_DEBUG,
        }
    }

    /// Set the color layer of the RGB LEDs
    async fn set_color_layer(&mut self, layer: u8) {
        if SIDE_CHANNEL.is_full() {
            defmt::error!("Side channel is full");
        }
        SIDE_CHANNEL.send(Event::RgbAnimChangeLayer(layer)).await;
        if ANIM_CHANNEL.is_full() {
            defmt::error!("Anim channel is full");
        }
        ANIM_CHANNEL.send(AnimCommand::ChangeLayer(layer)).await;
    }

    /// On key event
    fn on_key_event(&mut self, event: KBEvent) {
        //defmt::info!("Event: {:?}", defmt::Debug2Format(&event));
        self.layout.event(event);
    }

    /// Process the state of the keyboard and mouse
    async fn tick(&mut self) {
        #[cfg(feature = "timing_logs")]
        let start = Instant::now();

        #[cfg(feature = "debug_tick")]
        {
            self.debug_tick -= 1;
            if self.debug_tick == 0 {
                defmt::info!(
                    "Tick every {}s",
                    TICK_DEBUG / 1000 / REFRESH_RATE_MS as usize
                );
                self.debug_tick = TICK_DEBUG;
            }
        }
        // Process all mouse events first since they are time sensitive
        while let Some(mouse_report) = self.mouse.tick().await {
            let raw = mouse_report.serialize();
            if let Err(e) = self.hid_mouse_writer.write(&raw).await {
                defmt::error!("Failed to send mouse report: {:?}", e);
            }
        }

        // Process all events in the layout channel if any
        // This is where the keymap is processed
        while let Ok(event) = LAYOUT_CHANNEL.try_receive() {
            self.on_key_event(event);
        }
        let custom_event = self.layout.tick();
        let new_layer = self.layout.current_layer();
        self.process_custom_event(custom_event).await;
        let new_kb_report = generate_hid_kb_report(&mut self.layout);
        if new_kb_report != self.kb_report {
            self.kb_report = new_kb_report;
            if HID_KB_CHANNEL.is_full() {
                defmt::error!("HID KB channel is full");
            }
            HID_KB_CHANNEL.send(new_kb_report).await;
        }
        if new_layer != self.current_layer {
            defmt::info!("Layer: {}", new_layer);
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
                    defmt::error!("Sensor channel is full");
                }
                SENSOR_CMD_CHANNEL.send(SensorCommand::IncreaseCpi).await;
            }
            #[cfg(feature = "cnano")]
            KbCustomEvent::Release(CustomEvent::IncreaseCpi) => {}
            #[cfg(feature = "cnano")]
            KbCustomEvent::Press(CustomEvent::DecreaseCpi) => {
                if SENSOR_CMD_CHANNEL.is_full() {
                    defmt::error!("Sensor channel is full");
                }
                SENSOR_CMD_CHANNEL.send(SensorCommand::DecreaseCpi).await;
            }
            #[cfg(feature = "cnano")]
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
}

#[embassy_executor::task]
/// Keyboard layout handler
/// Handles layout events into the keymap and sends HID reports to the HID handler
pub async fn run(mut core: Core<'static>) {
    let mut ticker = Ticker::every(Duration::from_millis(REFRESH_RATE_MS));
    #[cfg(feature = "timing_logs")]
    let start_time = Instant::now();
    #[cfg(feature = "timing_logs")]
    let mut tick_count: u64 = 0;
    #[cfg(feature = "timing_logs")]
    let mut max_lateness_us: u64 = 0;
    #[cfg(feature = "timing_logs")]
    let mut total_lateness_us: u64 = 0;
    #[cfg(feature = "timing_logs")]
    let mut timing_tick_count: usize = 0;
    #[cfg(feature = "timing_logs")]
    let mut timing_total_us: u64 = 0;
    #[cfg(feature = "timing_logs")]
    let mut timing_max_us: u64 = 0;

    loop {
        match select(ticker.next(), LAYOUT_CHANNEL.receive()).await {
            Either::First(_) => {
                #[cfg(feature = "timing_logs")]
                let start = Instant::now();
                #[cfg(feature = "timing_logs")]
                {
                    tick_count += 1;
                    let expected = start_time + Duration::from_millis(tick_count * REFRESH_RATE_MS);
                    if start > expected {
                        let lateness_us = (start - expected).as_micros();
                        total_lateness_us += lateness_us;
                        if lateness_us > max_lateness_us {
                            max_lateness_us = lateness_us;
                        }
                        if lateness_us > 100 {
                            defmt::warn!(
                                "[TIMING] core ticker late by {}us (tick #{})",
                                lateness_us,
                                tick_count
                            );
                        }
                    }
                    // Report lateness stats every 5000 ticks
                    if tick_count % 5000 == 0 {
                        let avg_lateness_us = total_lateness_us / 5000;
                        defmt::info!(
                            "[TIMING] core ticker stats: avg={}us max={}us (over 5000 ticks)",
                            avg_lateness_us,
                            max_lateness_us
                        );
                        total_lateness_us = 0;
                        max_lateness_us = 0;
                    }
                }
                core.tick().await;

                #[cfg(feature = "timing_logs")]
                {
                    let elapsed_us = start.elapsed().as_micros();
                    timing_total_us += elapsed_us;
                    timing_tick_count += 1;
                    if elapsed_us > timing_max_us {
                        timing_max_us = elapsed_us;
                    }
                    // Log every 5 seconds
                    if timing_tick_count >= 5000 {
                        if elapsed_us > 1100 {
                            defmt::warn!(
                                "[TIMING] core::tick took {}us which is too long!",
                                elapsed_us
                            );
                        }
                        defmt::info!(
                            "[TIMING] core::tick total={}ms max={}us (over {} ticks in 5s)",
                            timing_total_us / 1000,
                            timing_max_us,
                            timing_tick_count
                        );
                        timing_tick_count = 0;
                        timing_total_us = 0;
                        timing_max_us = 0;
                    }
                }
            }
            Either::Second(event) => {
                core.on_key_event(event);
            }
        };
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
