use crate::device::is_host;
use embassy_futures::select::select;
use embassy_rp::peripherals::USB;
use embassy_rp::usb::Driver;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};
use embassy_usb::class::hid::HidWriter;
use usbd_hid::descriptor::MouseReport;

#[derive(Debug)]
pub enum MouseCommand {
    PressRightClick = 1,
    ReleaseRightClick = 2,
    PressLeftClick = 3,
    ReleaseLeftClick = 4,
    PressWheelClick = 5,
    ReleaseWheelClick = 6,
    PressBallIsWheel = 7,
    ReleaseBallIsWheel = 8,
}

/// Maximum number of commands in the channel
pub const NB_CMD: usize = 64;

/// Channel to send commands to the mouse handler
pub static MOUSE_CMD_CHANNEL: Channel<CriticalSectionRawMutex, MouseCommand, NB_CMD> =
    Channel::new();

/// Mouse move event
#[derive(Debug)]
pub struct MouseMove {
    /// Delta X
    pub dx: i8,
    /// Delta Y
    pub dy: i8,
}

/// Maximum number of movements in the channel
pub const NB_MOVE: usize = 8;
/// Channel to send movement reports from the sensor
pub static MOUSE_MOVE_CHANNEL: Channel<CriticalSectionRawMutex, MouseMove, NB_MOVE> =
    Channel::new();

/// Mouse handler
pub struct MouseHandler<'a> {
    /// Left click is pressed
    left_click: bool,
    /// Right click is pressed
    right_click: bool,
    /// Middle click is pressed
    wheel_click: bool,

    /// Moving the ball is actually moving the wheel
    ball_is_wheel: bool,

    /// Direction X
    dx: i8,
    /// Direction Y
    dy: i8,

    /// HID writer
    hid_writer: HidWriter<'a, Driver<'a, USB>, 64>,
}

const MOUSE_REPORT_EMPTY: MouseReport = MouseReport {
    x: 0,
    y: 0,
    buttons: 0,
    wheel: 0,
    pan: 0,
};

impl<'a> MouseHandler<'a> {
    /// Create a new mouse handler
    pub fn new(hid_writer: HidWriter<'a, Driver<'a, USB>, 64>) -> Self {
        MouseHandler {
            left_click: false,
            right_click: false,
            wheel_click: false,
            ball_is_wheel: false,
            dx: 0,
            dy: 0,
            hid_writer,
        }
    }

    /// Compute the state of the mouse. Called every 1ms
    pub async fn run(&mut self) {
        loop {
            select(MOUSE_CMD_CHANNEL.receive(), MOUSE_MOVE_CHANNEL.receive()).await;
            if let Ok(event) = MOUSE_CMD_CHANNEL.try_receive() {
                match event {
                    MouseCommand::PressRightClick => self.right_click = true,
                    MouseCommand::ReleaseRightClick => self.right_click = false,
                    MouseCommand::PressLeftClick => self.left_click = true,
                    MouseCommand::ReleaseLeftClick => self.left_click = false,
                    MouseCommand::PressWheelClick => self.wheel_click = true,
                    MouseCommand::ReleaseWheelClick => self.wheel_click = false,
                    MouseCommand::PressBallIsWheel => self.ball_is_wheel = true,
                    MouseCommand::ReleaseBallIsWheel => self.ball_is_wheel = false,
                }
            }
            if let Ok(event) = MOUSE_MOVE_CHANNEL.try_receive() {
                self.dx = event.dx;
                self.dy = event.dy;
            }
            if is_host() {
                let hid_report = self.generate_hid_report();
                match self.hid_writer.write_serialize(&hid_report).await {
                    Ok(()) => {}
                    Err(e) => defmt::warn!("Failed to send report: {:?}", e),
                }
            }
        }
    }

    /// Generate a HID report for the mouse
    fn generate_hid_report(&mut self) -> MouseReport {
        let mut report = MOUSE_REPORT_EMPTY;
        if self.ball_is_wheel {
            match self.dy {
                y if y > 0 => report.pan = 1,
                y if y < 0 => report.pan = -1,
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
