//! This example test the RP Pico on board LED.
//!
//! It does not work with the RP Pico W board. See wifi_blinky.rs.

#![no_std]
#![no_main]

use embassy_rp::gpio::{Input, Pull};
use embassy_time::{Duration, Timer};
#[cfg(not(feature = "defmt"))]
use panic_halt as _;
use utils::log::info;
#[cfg(feature = "defmt")]
use {defmt_rtt as _, panic_probe as _};

#[embassy_executor::main]
async fn main(_spawner: embassy_executor::Spawner) {
    let p = embassy_rp::init(Default::default());

    // Initialize the GPIO pins for the rotary encoder
    let pin_a = Input::new(p.PIN_24, Pull::Up);
    let pin_b = Input::new(p.PIN_25, Pull::Up);

    // Variables to keep track of the encoder state
    let mut last_a = pin_a.is_high();
    #[cfg(feature = "defmt")]
    let mut position = 0;

    loop {
        // Read the current state of the pins
        let current_a = pin_a.is_high();
        let current_b = pin_b.is_high();

        // Check for a transition on pin A
        if current_a != last_a {
            if current_b != current_a {
                #[cfg(feature = "defmt")]
                {
                    position += 1;
                }
            } else {
                #[cfg(feature = "defmt")]
                {
                    position -= 1;
                }
            }
            info!("Position: {}", position);
        }

        // Update the last known state
        last_a = current_a;

        // Wait a short time before checking again
        Timer::after(Duration::from_millis(10)).await;
    }
}
