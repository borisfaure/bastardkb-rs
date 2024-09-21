//use crate::hid::{HID_KB_CHANNEL, HID_MOUSE_CHANNEL};
//use crate::mouse::MouseHandler;
use embassy_futures::select::{select, Either};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};
use embassy_time::{Duration, Ticker};
use keyberon::layout::{Event, Layout};
use usbd_hid::descriptor::KeyboardReport;

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
pub static LAYOUT_CHANNEL: Channel<CriticalSectionRawMutex, Event, NB_EVENTS> = Channel::new();

#[derive(Debug)]
/// Custom events for the layout, mostly mouse events
#[allow(clippy::enum_variant_names)]
#[allow(dead_code)]
pub enum CustomEvent {
    /// Mouse move up
    MouseUp,
    /// Mouse move Right
    MouseRight,
    /// Mouse move down
    MouseDown,
    /// Mouse move left
    MouseLeft,
    /// Mouse left click
    MouseLeftClick,
    /// Mouse right click
    MouseRightClick,
    /// Mouse middle click
    MouseMiddleClick,
    /// Mouse scroll up
    MouseScrollUp,
    /// Mouse scroll down
    MouseScrollDown,
}

/// Set a report as an error based on keycode `kc`
fn keyboard_report_set_error(report: &mut KeyboardReport, kc: keyberon::key_code::KeyCode) {
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

/// Keyboard layout handler
/// Handles layout events into the keymap and sends HID reports to the HID handler
pub async fn layout_handler() {
    let mut layout = Layout::new(&LAYERS);
    //let mut mouse = MouseHandler::new();
    let mut old_kb_report = KeyboardReport::default();
    let mut ticker = Ticker::every(Duration::from_millis(REFRESH_RATE_MS));
    loop {
        match select(ticker.next(), LAYOUT_CHANNEL.receive()).await {
            Either::First(_) => {
                // Process all events in the channel if any
                while let Ok(event) = LAYOUT_CHANNEL.try_receive() {
                    layout.event(event);
                }
                let custom_event = layout.tick();
                let kb_report = generate_hid_kb_report(&mut layout);
                if kb_report != old_kb_report {
                    defmt::info!("KB Report: {:?}", defmt::Debug2Format(&kb_report));
                    //HID_KB_CHANNEL.send(kb_report).await;
                    old_kb_report = kb_report;
                }
                //mouse.process_event(custom_event);
                //if let Some(mouse_report) = mouse.tick() {
                //    defmt::info!("Mouse Report: {:?}", defmt::Debug2Format(&mouse_report));
                //    HID_MOUSE_CHANNEL.send(mouse_report).await;
                //}
            }
            Either::Second(event) => {
                layout.event(event);
            }
        };
    }
}
