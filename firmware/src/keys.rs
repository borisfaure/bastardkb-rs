use crate::device::is_host;
use crate::layout::LAYOUT_CHANNEL;
use crate::side::SIDE_CHANNEL;
use embassy_rp::gpio::{Input, Output};
use embassy_time::{Duration, Ticker};
use keyberon::debounce::Debouncer;
use keyberon::layout::Event as KBEvent;
use utils::serde::Event;

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
        for (c, col) in self.cols.iter_mut().enumerate() {
            col.set_low();
            cortex_m::asm::delay(100);
            for (r, row) in self.rows.iter().enumerate() {
                if row.is_low() {
                    matrix_state[r][c] = true;
                }
            }
            col.set_high();
        }
        matrix_state
    }
}

/// Loop that scans the keyboard matrix
pub async fn matrix_scanner(mut matrix: Matrix<'_>, is_right: bool) {
    let mut ticker = Ticker::every(Duration::from_hz(REFRESH_RATE.into()));
    let mut debouncer = Debouncer::new(matrix_state_new(), matrix_state_new(), NB_BOUNCE);

    loop {
        let transform = if is_right {
            |e: KBEvent| e.transform(|i, j| (i, 9 - j))
        } else {
            |e| e
        };
        let is_host = is_host();

        for event in debouncer.events(matrix.scan()).map(transform) {
            defmt::info!("Event: {:?}", defmt::Debug2Format(&event));
            if is_host {
                LAYOUT_CHANNEL.send(event).await;
            } else {
                match event {
                    KBEvent::Press(i, j) => {
                        SIDE_CHANNEL.send(Event::Press(i, j)).await;
                    }
                    KBEvent::Release(i, j) => {
                        SIDE_CHANNEL.send(Event::Release(i, j)).await;
                    }
                }
            };
        }

        ticker.next().await;
    }
}
