//! Goal of this example is to demonstrate full-duplex communication using PIO
//! state machines.
//! On the cnano, the TRRS jack is plucket at the end so that what is sent is
//! received back.
#![no_std]
#![no_main]

use defmt::{error, info};
use embassy_executor::Spawner;
use embassy_rp::bind_interrupts;
use embassy_rp::clocks::clk_sys_freq;
use embassy_rp::gpio::{Level, Output};
use embassy_rp::peripherals::{PIN_1, PIN_29, PIO1};
use embassy_rp::pio::{
    self, program::pio_file, Direction, FifoJoin, InterruptHandler as PioInterruptHandler, Pio,
    ShiftDirection, StateMachine,
};
use embassy_time::Timer;
use fixed::{traits::ToFixed, types::U56F8};
use {defmt_rtt as _, panic_probe as _};

bind_interrupts!(struct PioIrq1 {
    PIO1_IRQ_0 => PioInterruptHandler<PIO1>;
});
const USART_SPEED: u64 = 57600;
const TX: usize = 0;
const RX: usize = 1;
pub type SmTx<'a> = StateMachine<'a, PIO1, { TX }>;
pub type SmRx<'a> = StateMachine<'a, PIO1, { RX }>;
pub type PioCommon<'a> = pio::Common<'a, PIO1>;
pub type PioPin<'a> = pio::Pin<'a, PIO1>;

const T0: u32 = u32::from_le_bytes([0, 0, 0, 0]);
const T1: u32 = u32::from_le_bytes([1, 1, 1, 1]);
const T2: u32 = u32::from_le_bytes([2, 2, 2, 2]);
const T3: u32 = u32::from_le_bytes([3, 3, 3, 3]);
const T4: u32 = u32::from_le_bytes([4, 4, 4, 4]);
const T5: u32 = u32::from_le_bytes([5, 5, 5, 5]);
const T6: u32 = u32::from_le_bytes([6, 6, 6, 6]);
const T7: u32 = u32::from_le_bytes([7, 7, 7, 7]);
const T8: u32 = u32::from_le_bytes([8, 8, 8, 8]);
const T9: u32 = u32::from_le_bytes([9, 9, 9, 9]);
const TA: u32 = u32::from_le_bytes([10, 10, 10, 10]);
const TB: u32 = u32::from_le_bytes([11, 11, 11, 11]);
const TC: u32 = u32::from_le_bytes([12, 12, 12, 12]);
const TD: u32 = u32::from_le_bytes([13, 13, 13, 13]);
const TE: u32 = u32::from_le_bytes([14, 14, 14, 14]);
const TF: u32 = u32::from_le_bytes([15, 15, 15, 15]);
const B: u32 = u32::from_le_bytes([b'P', 3, 7, b'\n']);
const C: u32 = u32::from_le_bytes([0x33, 0, 0, 0x33]);
const M: u32 = u32::from_le_bytes([0xff, 3, 7, 0xff]);
const MAX: u32 = u32::MAX;

const TEST_DATA: [u32; 25] = [
    T0, T1, T3, T7, TA, T0, T1, T2, T3, T4, T5, T6, T7, T8, T9, TA, TB, TC, TD, TE, TF, B, C, M,
    MAX,
];

pub async fn tx_loop(mut tx_sm: SmTx<'_>) {
    loop {
        Timer::after_secs(2).await;
        for n in TEST_DATA.iter() {
            info!("sending event 0x{:08x} 0b{:032b}", n, n);
            tx_sm.tx().wait_push(*n).await;
            Timer::after_millis(10).await;
        }
        Timer::after_secs(4).await;
    }
}

pub async fn rx_loop(mut rx_sm: SmRx<'_>, status_led: &mut Output<'_>) {
    info!("waiting for event");
    loop {
        for n in TEST_DATA.iter() {
            let v = rx_sm.rx().wait_pull().await;
            status_led.set_low();
            if v != *n {
                error!(
                    "event received failure: 0x{:08x} 0b{:032b}, expecting 0x{:08x} 0b{:032b}",
                    v, v, *n, *n
                );
            } else {
                info!("event received ok: 0x{:08x} 0b{:032b}", v, v);
            }
            Timer::after_millis(10).await;
            status_led.set_high();
            assert_eq!(v, *n);
        }
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
    let (mut pin_tx, mut pin_rx) = if false {
        (
            pio_common.make_pio_pin(gpio_pin_1),
            pio_common.make_pio_pin(gpio_pin_29),
        )
    } else {
        (
            pio_common.make_pio_pin(gpio_pin_29),
            pio_common.make_pio_pin(gpio_pin_1),
        )
    };

    let tx_sm = task_tx(&mut pio_common, sm0, &mut pin_tx);
    let rx_sm = task_rx(&mut pio_common, sm1, &mut pin_rx);

    futures::future::join(tx_loop(tx_sm), rx_loop(rx_sm, status_led)).await;
}

fn pio_freq() -> fixed::FixedU32<fixed::types::extra::U8> {
    (U56F8::from_num(clk_sys_freq()) / (8 * USART_SPEED)).to_fixed()
}

fn task_tx<'a>(
    common: &mut PioCommon<'a>,
    mut sm_tx: SmTx<'a>,
    tx_pin: &mut PioPin<'a>,
) -> SmTx<'a> {
    let tx_prog = pio_file!("src/tx.pio");
    sm_tx.set_pins(Level::High, &[tx_pin]);
    sm_tx.set_pin_dirs(Direction::Out, &[tx_pin]);

    let mut cfg = embassy_rp::pio::Config::default();
    cfg.set_out_pins(&[tx_pin]);
    cfg.set_set_pins(&[tx_pin]);
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
    let rx_prog = pio_file!("src/rx.pio");
    sm_rx.set_pins(Level::High, &[rx_pin]);
    sm_rx.set_pin_dirs(Direction::In, &[rx_pin]);

    let mut cfg = embassy_rp::pio::Config::default();
    cfg.set_in_pins(&[rx_pin]);
    cfg.set_jmp_pin(rx_pin);
    cfg.use_program(&common.load_program(&rx_prog.program), &[]);
    cfg.shift_in.auto_fill = false;
    cfg.shift_in.direction = ShiftDirection::Right;
    cfg.shift_in.threshold = 32;
    cfg.fifo_join = FifoJoin::RxOnly;
    cfg.clock_divider = pio_freq();
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
    status_led.set_high();

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
