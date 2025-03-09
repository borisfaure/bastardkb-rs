use crate::device::is_host;
use crate::hid::MouseReport;
use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, channel::Channel};

/// Mouse move event
#[derive(Debug, defmt::Format)]
pub struct MouseMove {
    /// Delta X
    pub dx: i16,
    /// Delta Y
    pub dy: i16,
}

/// Maximum number of movements in the channel
pub const NB_MOVE: usize = 8;
/// Channel to send movement reports from the sensor
pub static MOUSE_MOVE_CHANNEL: Channel<ThreadModeRawMutex, MouseMove, NB_MOVE> = Channel::new();

/// Mouse handler
pub struct MouseHandler {
    /// Left click is pressed
    left_click: bool,
    /// Right click is pressed
    right_click: bool,
    /// Middle click is pressed
    wheel_click: bool,

    /// Moving the ball is actually moving the wheel
    ball_is_wheel: bool,

    /// Direction X
    dx: i16,
    /// Direction Y
    dy: i16,

    /// Whether the state has changed
    changed: bool,
}

/// Threshold to consider the movement as a wheel movement
const WHEEL_THRESHOLD: i16 = 16;

/// Empty mouse report
const MOUSE_REPORT_EMPTY: MouseReport = MouseReport {
    x: 0,
    y: 0,
    buttons: 0,
    wheel: 0,
    pan: 0,
};

impl MouseHandler {
    /// Create a new mouse handler
    pub fn new() -> Self {
        MouseHandler {
            left_click: false,
            right_click: false,
            wheel_click: false,
            ball_is_wheel: false,
            dx: 0,
            dy: 0,
            changed: false,
        }
    }

    /// On left click
    pub fn on_left_click(&mut self, is_pressed: bool) {
        self.left_click = is_pressed;
        self.changed = true;
    }

    /// On right click
    pub fn on_right_click(&mut self, is_pressed: bool) {
        self.right_click = is_pressed;
        self.changed = true;
    }

    /// On middle click
    pub fn on_middle_click(&mut self, is_pressed: bool) {
        self.wheel_click = is_pressed;
        self.changed = true;
    }

    /// On Ball is wheel
    pub fn on_ball_is_wheel(&mut self, is_pressed: bool) {
        self.ball_is_wheel = is_pressed;
        self.changed = true;
    }

    /// Handle a mouse movement event
    fn handle_move_event(&mut self, MouseMove { dx, dy }: MouseMove) {
        self.dx = dx;
        self.dy = dy;
        self.changed = true;
    }

    /// Compute the state of the mouse. Called every 1ms
    pub async fn tick(&mut self) -> Option<MouseReport> {
        if let Ok(event) = MOUSE_MOVE_CHANNEL.try_receive() {
            self.handle_move_event(event);
            self.changed = true;
        }
        if self.changed && is_host() {
            self.changed = false;
            let hid_report = self.generate_hid_report();
            Some(hid_report)
        } else {
            None
        }
    }

    /// Generate a HID report for the mouse
    fn generate_hid_report(&mut self) -> MouseReport {
        let mut report = MOUSE_REPORT_EMPTY;
        if self.ball_is_wheel {
            match self.dy {
                y if y > WHEEL_THRESHOLD => report.wheel = -1,
                y if y < -WHEEL_THRESHOLD => report.wheel = 1,
                _ => {}
            }
        } else {
            report.x = self.dx;
            report.y = self.dy;
            if self.left_click {
                report.buttons |= 1;
            }
            if self.right_click {
                report.buttons |= 2;
            }
            if self.wheel_click {
                report.buttons |= 4;
            }
        }
        report
    }
}
