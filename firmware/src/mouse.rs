use crate::device::is_host;
use crate::hid::MouseReport;
use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, channel::Channel};

/// Mouse move event
#[derive(Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct MouseMove {
    /// Delta X
    pub dx: i16,
    /// Delta Y
    pub dy: i16,
    /// Pressure (0-63 for trackpad, 0 for trackball)
    pub pressure: u8,
}

/// Maximum number of movements in the channel
pub const NB_MOVE: usize = 128;
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

    /// Wheel movement
    /// Positive is up, negative is Down
    /// 0 is no movement, reset on every tick
    wheel: i8,

    /// Whether the state has changed
    changed: bool,

    /// Current pressure value (0-63 for trackpad, 0 for trackball)
    pressure: u8,
}

/// Threshold to consider the movement as a wheel movement
const WHEEL_THRESHOLD: i16 = 16;

/// Minimum pressure threshold to maintain mouse mode (dilemma only)
/// Values range from 0-63
#[cfg(feature = "dilemma")]
const PRESSURE_NO_MVMT: u8 = 27;
#[cfg(feature = "dilemma")]
const MIN_PRESSURE_MVMT: u8 = 10;

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
            wheel: 0,
            changed: false,
            pressure: 0,
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

    /// On wheel
    #[cfg(feature = "dilemma")]
    pub fn on_wheel(&mut self, is_up: bool) {
        self.wheel = if is_up { 1 } else { -1 };
        self.changed = true;
    }

    /// Handle a mouse movement event
    fn handle_move_event(&mut self, MouseMove { dx, dy, pressure }: MouseMove) {
        self.dx = dx;
        self.dy = dy;
        self.pressure = pressure;
        self.changed = true;
    }

    /// Compute the state of the mouse. Called every 1ms
    /// Returns (MouseReport, has_pressure) where has_pressure indicates if there's
    /// sufficient pressure on the trackpad to maintain mouse mode without cursor movement
    pub async fn tick(&mut self) -> Option<(MouseReport, bool)> {
        if let Ok(event) = MOUSE_MOVE_CHANNEL.try_receive() {
            self.handle_move_event(event);
            self.changed = true;
        }
        if self.changed && is_host() {
            self.changed = false;
            let hid_report = self.generate_hid_report();
            #[cfg(feature = "dilemma")]
            {
                let res = match self.pressure {
                    // sufficient pressure to maintain mouse mode
                    p if p >= PRESSURE_NO_MVMT => Some((hid_report, true)),
                    // insufficient pressure, but allow movement
                    p if p >= MIN_PRESSURE_MVMT => Some((hid_report, false)),
                    // no pressure, could be wheel movement only
                    p if p == 0 && self.wheel != 0 => Some((hid_report, false)),
                    _ => None,
                };
                self.wheel = 0;
                return res;
            }
            #[cfg(not(feature = "dilemma"))]
            {
                self.wheel = 0;
                Some((hid_report, false))
            }
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
            report.wheel = self.wheel;
        }
        report
    }
}
