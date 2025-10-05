use crate::core::LAYOUT_CHANNEL;
use crate::device::is_host;
use crate::rgb_leds::RGB_CHANNEL;
use crate::side::SIDE_CHANNEL;
use embassy_executor::Spawner;
use embassy_rp::gpio::{Input, Output};
#[cfg(feature = "timing_logs")]
use embassy_time::Instant;
use embassy_time::{Duration, Ticker};
use keyberon::debounce::Debouncer;
use keyberon::layout::Event as KBEvent;
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
        defmt::error!("Encoder pins are not supported on the Cnano");
    }

    #[cfg(feature = "dilemma")]
    let (encoder_pin_a, encoder_pin_b) = encoder_pins.unwrap();
    #[cfg(feature = "dilemma")]
    let mut last_pin_a = encoder_pin_a.is_high();

    #[cfg(feature = "timing_logs")]
    let mut timing_tick_count: usize = 0;
    #[cfg(feature = "timing_logs")]
    let mut timing_total_us: u64 = 0;
    #[cfg(feature = "timing_logs")]
    let mut timing_max_us: u64 = 0;
    #[cfg(feature = "timing_logs")]
    let start_time = Instant::now();
    #[cfg(feature = "timing_logs")]
    let mut tick_lateness_count: u64 = 0;
    #[cfg(feature = "timing_logs")]
    let mut max_lateness_us: u64 = 0;
    #[cfg(feature = "timing_logs")]
    let mut total_lateness_us: u64 = 0;

    loop {
        #[cfg(feature = "timing_logs")]
        let start = Instant::now();
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
            //defmt::info!("Event: {:?}", defmt::Debug2Format(&event));
            if is_host {
                if LAYOUT_CHANNEL.is_full() {
                    defmt::error!("Layout channel is full");
                }
                LAYOUT_CHANNEL.send(event).await;
                if RGB_CHANNEL.is_full() {
                    defmt::error!("RGB channel is full");
                }
                RGB_CHANNEL.send(event).await;
            } else {
                match event {
                    KBEvent::Press(r, c) => {
                        if SIDE_CHANNEL.is_full() {
                            defmt::error!("Side channel is full");
                        }
                        SIDE_CHANNEL.send(Event::Press(r, c)).await;
                        if RGB_CHANNEL.is_full() {
                            defmt::error!("RGB channel is full");
                        }
                        RGB_CHANNEL.send(event).await;
                    }
                    KBEvent::Release(r, c) => {
                        if SIDE_CHANNEL.is_full() {
                            defmt::error!("Side channel is full");
                        }
                        SIDE_CHANNEL.send(Event::Release(r, c)).await;
                        if RGB_CHANNEL.is_full() {
                            defmt::error!("RGB channel is full");
                        }
                        RGB_CHANNEL.send(event).await;
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
                    defmt::error!("Layout channel is full");
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
                defmt::info!(
                    "[TIMING] matrix_scanner total={}ms max={}us (over {} scans in 5s)",
                    timing_total_us / 1000,
                    timing_max_us,
                    timing_tick_count
                );
                timing_tick_count = 0;
                timing_total_us = 0;
                timing_max_us = 0;
            }
        }

        ticker.next().await;

        #[cfg(feature = "timing_logs")]
        {
            let now = Instant::now();
            tick_lateness_count += 1;
            let expected = start_time
                + Duration::from_micros((tick_lateness_count * 1000000) / REFRESH_RATE as u64);
            if now > expected {
                let lateness_us = (now - expected).as_micros();
                total_lateness_us += lateness_us;
                if lateness_us > max_lateness_us {
                    max_lateness_us = lateness_us;
                }
                if lateness_us > 100 {
                    defmt::warn!(
                        "[TIMING] matrix_scanner ticker late by {}us (tick #{})",
                        lateness_us,
                        tick_lateness_count
                    );
                }
            }
            // Report lateness stats every 5000 ticks
            if tick_lateness_count % 5000 == 0 {
                let avg_lateness_us = total_lateness_us / 5000;
                defmt::info!(
                    "[TIMING] matrix_scanner ticker stats: avg={}us max={}us (over 5000 ticks)",
                    avg_lateness_us,
                    max_lateness_us
                );
                total_lateness_us = 0;
                max_lateness_us = 0;
            }
        }
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
