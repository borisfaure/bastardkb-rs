use crate::layout::CustomEvent;
use core::sync::atomic::{AtomicU32, Ordering};
use keyberon::layout::CustomEvent as KbCustomEvent;
use usbd_hid::descriptor::MouseReport;

static DX: AtomicU32 = AtomicU32::new(0u);

/// Mouse handler
#[derive(Debug, Default)]
pub struct MouseHandler {
    /// Left click is pressed
    left_click: bool,
    /// Right click is pressed
    right_click: bool,
    /// Middle click is pressed
    middle_click: bool,

    /// Wheel up
    wheel_up: bool,
    /// Wheel down
    wheel_down: bool,

    /// Is the mouse layer active
    is_mouse_layer: bool,
    /// Is the mouse wheel layer active
    is_mouse_wheel_layer: bool,
}

impl MouseHandler {
    /// Create a new mouse handler
    pub fn new() -> Self {
        MouseHandler {
            ..Default::default()
        }
    }
    /// Process a custom event
    pub fn process_event(&mut self, kb_cs_event: keyberon::layout::CustomEvent<CustomEvent>) {
        if let Some((event, is_pressed)) = match kb_cs_event {
            KbCustomEvent::Press(event) => Some((event, true)),
            KbCustomEvent::Release(event) => Some((event, false)),
            _ => None,
        } {
            self.has_changed = true;
            match event {
                CustomEvent::MouseLeftClick => self.left_click = is_pressed,
                CustomEvent::MouseRightClick => self.right_click = is_pressed,
            }
        }
    }

    /// Compute the state of the mouse. Called every 1ms
    pub fn tick(
        &mut self,
        is_mouse_layer: bool,
        is_mouse_wheel_layer: bool,
    ) -> Option<MouseReport> {
        // Return empty report if the mouse is not active
        if self.is_mouse_layer && !is_mouse_layer {
            self.is_mouse_layer = false;
            return Some(MouseReport::default());
        }
        // Return empty report if the mouse wheel is not active
        if self.is_mouse_wheel_layer && !is_mouse_wheel_layer {
            self.is_mouse_wheel_layer = false;
            return Some(MouseReport::default());
        }

        if is_mouse_layer || is_mouse_wheel_layer {
            self.update();
        }
        if self.has_changed {
            self.has_changed = false;
            Some(self.generate_hid_report())
        } else {
            None
        }
    }

    /// Generate a HID report for the mouse
    fn generate_hid_report(&mut self) -> MouseReport {
        let mut report = MouseReport::default();
        if self.up {
            report.y = self.rate_vertical;
        } else if self.down {
            report.y = -self.rate_vertical;
        }
        if self.left {
            report.x = -self.rate_horizontal;
        } else if self.right {
            report.x = self.rate_horizontal;
        }
        if self.left_click {
            report.buttons |= 1;
        }
        if self.right_click {
            report.buttons |= 2;
        }
        if self.middle_click {
            report.buttons |= 4;
        }
        if self.wheel_up {
            report.wheel = 1;
        } else if self.wheel_down {
            report.wheel = -1;
        }
        report
    }
}
