#![no_std]
#![no_main]

use crate::keys::{matrix_scanner, Matrix};
use embassy_executor::Spawner;
use embassy_rp::gpio::{Input, Level, Output, Pull};
use {defmt_rtt as _, panic_probe as _};

mod keymap_basic;
mod keys;
mod layout;

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = embassy_rp::init(Default::default());
    defmt::info!("Hello World!");

    let rows = [
        Input::new(p.PIN_26, Pull::Up), // R2
        Input::new(p.PIN_5, Pull::Up),  // R3
        Input::new(p.PIN_4, Pull::Up),  // R4
        Input::new(p.PIN_9, Pull::Up),  // R5
    ];
    let cols = [
        Output::new(p.PIN_28, Level::High), // C2
        Output::new(p.PIN_21, Level::High), // C3
        Output::new(p.PIN_6, Level::High),  // C4
        Output::new(p.PIN_7, Level::High),  // C5
        Output::new(p.PIN_8, Level::High),  // C6
    ];
    let matrix = Matrix::new(rows, cols);

    let layout_fut = layout::layout_handler();
    let matrix_fut = matrix_scanner(matrix);
    futures::join!(layout_fut, matrix_fut);
}
