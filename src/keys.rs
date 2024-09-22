use crate::layout::LAYOUT_CHANNEL;
use crate::side::{is_host, is_right};
use embassy_rp::gpio::{Input, Output};
use embassy_time::{Duration, Ticker};
use keyberon::debounce::Debouncer;
use keyberon::layout::Event;

/// Keyboard matrix rows
const ROWS: usize = 4;
/// Keyboard matrix columns
const COLS: usize = 5;
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

    fn scan(&mut self) -> MatrixState {
        let mut matrix_state = [[false; COLS]; ROWS];
        for col in 0..COLS {
            self.cols[col].set_low();
            for row in 0..ROWS {
                if self.rows[row].is_low() {
                    matrix_state[row][col] = true;
                }
            }
            self.cols[col].set_high();
        }
        matrix_state
    }
}

/// Loop that scans the keyboard matrix
pub async fn matrix_scanner(mut matrix: Matrix<'_>) {
    let mut ticker = Ticker::every(Duration::from_hz(REFRESH_RATE.into()));
    let mut debouncer = Debouncer::new(matrix_state_new(), matrix_state_new(), NB_BOUNCE);

    loop {
        let transform = if is_right() {
            |e: Event| e.transform(|i, j| (i, 9 - j))
        } else {
            |e| e
        };
        let is_host = is_host();

        for event in debouncer.events(matrix.scan()).map(transform) {
            defmt::info!("Event: {:?}", defmt::Debug2Format(&event));
            LAYOUT_CHANNEL.send(event).await;
            if is_host {
                LAYOUT_CHANNEL.send(event).await;
            } else {
                //SIDE_CHANNEL.send(event).await;
            };
        }

        ticker.next().await;
    }
}
