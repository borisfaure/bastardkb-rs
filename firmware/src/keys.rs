use crate::core::LAYOUT_CHANNEL;
use crate::device::is_host;
use crate::side::SIDE_CHANNEL;
use embassy_executor::Spawner;
use embassy_rp::gpio::{Input, Output};
use embassy_time::{Duration, Ticker};
use keyberon::debounce::Debouncer;
use keyberon::layout::Event as KBEvent;
use utils::log::error;
use utils::serde::Event;

/// Keyboard matrix rows
pub const ROWS: usize = 4;
/// Keyboard matrix columns
pub const COLS: usize = 5;
/// Full number of columns
pub const FULL_COLS: usize = 2 * COLS;
/// Keyboard matrix refresh rate, in Hz
const REFRESH_RATE: u16 = 1000;
/// Keyboard matrix debouncing time, in ms
const DEBOUNCE_TIME_MS: u16 = 5;
/// Keyboard bounce number
const NB_BOUNCE: u16 = REFRESH_RATE * DEBOUNCE_TIME_MS / 1000;

/// Pins for the keyboard matrix
pub struct Matrix<'a> {
    rows: [Input<'a>; ROWS],
    cols: [Output<'a>; COLS],
}

/// Keyboard matrix state
type MatrixState = [[bool; COLS]; ROWS];
/// Create a new keyboard matrix state
fn matrix_state_new() -> MatrixState {
    [[false; COLS]; ROWS]
}

impl<'a> Matrix<'a> {
    /// Create a new keyboard matrix
    pub fn new(rows: [Input<'a>; ROWS], cols: [Output<'a>; COLS]) -> Self {
        Self { rows, cols }
    }

    async fn scan(&mut self) -> MatrixState {
        let mut matrix_state = [[false; COLS]; ROWS];
        for (c, col) in self.cols.iter_mut().enumerate() {
            col.set_low();
            cortex_m::asm::delay(100);
            for (r, row) in self.rows.iter().enumerate() {
                if row.is_low() {
                    matrix_state[r][c] = true;
                }
            }
            col.set_high();
            cortex_m::asm::delay(50);
        }
        matrix_state
    }
}

/// Loop that scans the keyboard matrix
#[embassy_executor::task]
async fn matrix_scanner(
    mut matrix: Matrix<'static>,
    encoder_pins: Option<(Input<'static>, Input<'static>)>,
    is_right: bool,
) {
    let mut ticker = Ticker::every(Duration::from_hz(REFRESH_RATE.into()));
    let mut debouncer = Debouncer::new(matrix_state_new(), matrix_state_new(), NB_BOUNCE);

    #[cfg(feature = "cnano")]
    if encoder_pins.is_some() {
        error!("Encoder pins are not supported on the Cnano");
    }

    #[cfg(feature = "dilemma")]
    let (encoder_pin_a, encoder_pin_b) = encoder_pins.unwrap();
    #[cfg(feature = "dilemma")]
    let mut last_pin_a = encoder_pin_a.is_high();

    loop {
        let transform = if is_right {
            |e: KBEvent| {
                e.transform(|r, c| {
                    if r == 3 {
                        match c {
                            #[cfg(feature = "cnano")]
                            0 => (3, 5),
                            #[cfg(feature = "dilemma")]
                            0 => (3, 6),
                            #[cfg(feature = "dilemma")]
                            1 => (3, 5),
                            #[cfg(feature = "cnano")]
                            2 => (3, 6),
                            _ => panic!("Invalid key {:?}", (r, c)),
                        }
                    } else {
                        (r, 9 - c)
                    }
                })
            }
        } else {
            |e: KBEvent| {
                e.transform(|r, c| {
                    if r == 3 {
                        match c {
                            #[cfg(feature = "cnano")]
                            0 => (3, 4),
                            #[cfg(feature = "dilemma")]
                            0 => (3, 3),
                            #[cfg(feature = "dilemma")]
                            1 => (3, 4),
                            #[cfg(any(feature = "cnano", feature = "dilemma"))]
                            2 => (3, 2),
                            #[cfg(feature = "cnano")]
                            3 => (3, 3),
                            _ => panic!("Invalid key {:?}", (r, c)),
                        }
                    } else {
                        (r, c)
                    }
                })
            }
        };
        let is_host = is_host();

        for event in debouncer.events(matrix.scan().await).map(transform) {
            if is_host {
                if LAYOUT_CHANNEL.is_full() {
                    error!("Layout channel is full");
                }
                LAYOUT_CHANNEL.send(event).await;
            } else {
                match event {
                    KBEvent::Press(r, c) => {
                        if SIDE_CHANNEL.is_full() {
                            error!("Side channel is full");
                        }
                        SIDE_CHANNEL.send(Event::Press(r, c)).await;
                    }
                    KBEvent::Release(r, c) => {
                        if SIDE_CHANNEL.is_full() {
                            error!("Side channel is full");
                        }
                        SIDE_CHANNEL.send(Event::Release(r, c)).await;
                    }
                }
            }
        }
        #[cfg(feature = "dilemma")]
        if is_right && is_host {
            // Read the current state of the pins
            let current_a = encoder_pin_a.is_high();
            let current_b = encoder_pin_b.is_high();

            // Check for a transition on pin A
            if current_a != last_pin_a {
                if LAYOUT_CHANNEL.is_full() {
                    error!("Layout channel is full");
                }
                if current_b != current_a {
                    LAYOUT_CHANNEL.send(KBEvent::Press(3, 8)).await;
                    LAYOUT_CHANNEL.send(KBEvent::Release(3, 8)).await;
                } else {
                    LAYOUT_CHANNEL.send(KBEvent::Press(3, 9)).await;
                    LAYOUT_CHANNEL.send(KBEvent::Release(3, 9)).await;
                }
                last_pin_a = current_a;
            }
        }

        ticker.next().await;
    }
}

pub fn init(
    spawner: &Spawner,
    matrix: Matrix<'static>,
    encoder_pins: Option<(Input<'static>, Input<'static>)>,
    is_right: bool,
) {
    spawner.must_spawn(matrix_scanner(matrix, encoder_pins, is_right));
}
