//! Goal of this example is to demonstrate full-duplex communication using PIO
//! state machines.
//! On the cnano, the TRRS jack is plucket at the end so that what is sent is
//! received back.
#![no_std]
#![no_main]

use defmt::info;
use embassy_executor::Spawner;
use embassy_rp::bind_interrupts;
use embassy_rp::clocks::clk_sys_freq;
use embassy_rp::gpio::{Level, Output};
use embassy_rp::peripherals::{PIN_1, PIN_29, PIO1};
use embassy_rp::pio::{self, Direction, FifoJoin, ShiftDirection, StateMachine};
use embassy_rp::pio::{InterruptHandler as PioInterruptHandler, Pio};
use embassy_time::Timer;
use fixed::{traits::ToFixed, types::U56F8};
use {defmt_rtt as _, panic_probe as _};

bind_interrupts!(struct PioIrq1 {
    PIO1_IRQ_0 => PioInterruptHandler<PIO1>;
});
const USART_SPEED: u64 = 460800;
const TX: usize = 0;
const RX: usize = 1;
pub type SmTx<'a> = StateMachine<'a, PIO1, { TX }>;
pub type SmRx<'a> = StateMachine<'a, PIO1, { RX }>;
pub type PioCommon<'a> = pio::Common<'a, PIO1>;
pub type PioPin<'a> = pio::Pin<'a, PIO1>;

pub async fn tx_loop<'a>(mut tx_sm: SmTx<'a>, status_led: &mut Output<'a>) {
    let b = u32::from_le_bytes([b'P', 3, 7, b'\n']);
    loop {
        Timer::after_secs(2).await;
        for n in [b, u32::min_value(), u32::max_value()].iter() {
            info!("sending event 0x{:x} 0b{:b}", n, n);
            status_led.set_low();
            tx_sm.tx().wait_push(*n).await;
            status_led.set_high();
        }
        Timer::after_secs(1).await;
    }
}

pub async fn rx_loop<'a>(mut rx_sm: SmRx<'a>) {
    info!("waiting for event");
    loop {
        let v = rx_sm.rx().wait_pull().await;
        info!("event received: 0x{:x} 0b{:b}", v, v);
    }
}

pub async fn full_duplex_comm<'a>(
    mut pio_common: PioCommon<'a>,
    sm0: SmTx<'a>,
    sm1: SmRx<'a>,
    gpio_pin_1: PIN_1,
    gpio_pin_29: PIN_29,
    status_led: &mut Output<'a>,
) {
    let mut pin_tx = pio_common.make_pio_pin(gpio_pin_1);
    let mut pin_rx = pio_common.make_pio_pin(gpio_pin_29);

    let tx_sm = task_tx(&mut pio_common, sm0, &mut pin_tx);
    let rx_sm = task_rx(&mut pio_common, sm1, &mut pin_rx);

    futures::future::join(tx_loop(tx_sm, status_led), rx_loop(rx_sm)).await;
}

fn pio_freq() -> fixed::FixedU32<fixed::types::extra::U8> {
    (U56F8::from_num(clk_sys_freq()) / (8 * USART_SPEED)).to_fixed()
}

fn task_tx<'a>(
    common: &mut PioCommon<'a>,
    mut sm_tx: SmTx<'a>,
    tx_pin: &mut PioPin<'a>,
) -> SmTx<'a> {
    let tx_prog = pio_proc::pio_file!("src/tx.pio");
    sm_tx.set_pins(Level::High, &[&tx_pin]);
    sm_tx.set_pin_dirs(Direction::Out, &[&tx_pin]);

    let mut cfg = embassy_rp::pio::Config::default();
    cfg.set_out_pins(&[tx_pin]);
    cfg.set_set_pins(&[&tx_pin]);
    cfg.use_program(&common.load_program(&tx_prog.program), &[]);
    cfg.shift_out.auto_fill = false;
    cfg.shift_out.direction = ShiftDirection::Right;
    cfg.shift_out.threshold = 32;
    cfg.fifo_join = FifoJoin::TxOnly;
    cfg.clock_divider = pio_freq();
    sm_tx.set_config(&cfg);
    sm_tx.set_enable(true);

    sm_tx
}

fn task_rx<'a>(
    common: &mut PioCommon<'a>,
    mut sm_rx: SmRx<'a>,
    rx_pin: &mut PioPin<'a>,
) -> SmRx<'a> {
    let rx_prog = pio_proc::pio_file!("src/rx.pio");

    let mut cfg = embassy_rp::pio::Config::default();
    cfg.use_program(&common.load_program(&rx_prog.program), &[]);

    sm_rx.set_pins(Level::High, &[&rx_pin]);
    cfg.set_in_pins(&[&rx_pin]);
    cfg.set_jmp_pin(&rx_pin);
    sm_rx.set_pin_dirs(Direction::In, &[&rx_pin]);

    cfg.clock_divider = pio_freq();
    cfg.shift_in.auto_fill = false;
    cfg.shift_in.direction = ShiftDirection::Right;
    cfg.shift_in.threshold = 32;
    cfg.fifo_join = FifoJoin::RxOnly;
    sm_rx.set_config(&cfg);
    sm_rx.set_enable(true);

    sm_rx
}

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    info!("Hello there!");

    let p = embassy_rp::init(Default::default());
    let pio1 = Pio::new(p.PIO1, PioIrq1);
    let mut status_led = Output::new(p.PIN_24, Level::Low);

    full_duplex_comm(
        pio1.common,
        pio1.sm0,
        pio1.sm1,
        p.PIN_1,
        p.PIN_29,
        &mut status_led,
    )
    .await;
}
