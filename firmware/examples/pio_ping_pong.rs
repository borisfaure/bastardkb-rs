//! This is a simple example of a half-duplex communication between two RP2040
//! using PIO state machines.
//! The example is a simple ping-pong server that sends back the received
//! data.  The data are u32 values that are sent as bytes.
//!
//! The right side sends messages to the left side. The left side sends back
//! the received message.  The right side counts the number of errors.

#![no_std]
#![no_main]

use embassy_executor::Spawner;
use embassy_futures::{
    select::{select, Either},
    yield_now,
};
use embassy_rp::{
    bind_interrupts, clocks,
    gpio::{Drive, Input, Level, Output, Pull},
    peripherals::{PIN_1, PIO1},
    pio::{
        self, program::pio_asm, Common, Direction, FifoJoin,
        InterruptHandler as PioInterruptHandler, Pio, ShiftDirection, StateMachine,
    },
};
use embassy_time::{Duration, Ticker};
use fixed::{traits::ToFixed, types::U56F8};
use utils::protocol::{Hardware, ReceivedOrTick};

use {defmt_rtt as _, panic_probe as _};

bind_interrupts!(struct PioIrq1 {
    PIO1_IRQ_0 => PioInterruptHandler<PIO1>;
});

const TX: usize = 0;
const RX: usize = 1;

type SmRx<'a> = StateMachine<'a, PIO1, { RX }>;
type SmTx<'a> = StateMachine<'a, PIO1, { TX }>;
type PioCommon<'a> = Common<'a, PIO1>;
type PioPin<'a> = pio::Pin<'a, PIO1>;

const SPEED: u64 = 460_800;

struct Hw<'a> {
    /// State machine to send events
    tx_sm: SmTx<'a>,
    /// State machine to receive events
    rx_sm: SmRx<'a>,
    /// Pin used for communication
    pin: PioPin<'a>,
    // error state
    on_error: bool,
    // 1s ticker
    ticker: Ticker,
}

impl<'a> Hw<'a> {
    pub fn new(tx_sm: SmTx<'a>, rx_sm: SmRx<'a>, pin: PioPin<'a>) -> Self {
        Self {
            tx_sm,
            rx_sm,
            pin,
            on_error: false,
            ticker: Ticker::every(Duration::from_secs(1)),
        }
    }

    async fn enter_rx(&mut self) {
        // Wait for TX FIFO to empty
        while !self.tx_sm.tx().empty() {
            yield_now().await;
        }

        // Wait a bit after the last sent message
        cortex_m::asm::delay(250);

        // Disable TX state machine before manipulating TX pin
        self.tx_sm.set_enable(false);

        // Set minimal drive strength on TX pin to avoid interfering with RX
        self.pin.set_drive_strength(Drive::_2mA);

        // Set pin as input
        self.rx_sm.set_pin_dirs(Direction::In, &[&self.pin]);
        self.tx_sm.set_pin_dirs(Direction::In, &[&self.pin]);

        // Ensure the pin is HIGH
        self.rx_sm.set_pins(Level::High, &[&self.pin]);
        self.tx_sm.set_pins(Level::High, &[&self.pin]);

        // Restart RX state machine to prepare for transmission
        self.rx_sm.restart();
        // Enable RX state machine
        // This allows it to start receiving data from the line
        // while benefiting from the pull-up drive
        self.rx_sm.set_enable(true);
    }

    fn enter_tx(&mut self) {
        // Disable RX state machine to prevent receiving transmitted data
        self.rx_sm.set_enable(false);

        // Increase drive strength for better signal integrity
        self.pin.set_drive_strength(Drive::_12mA);

        // Set TX pin as output (to drive the line)
        self.tx_sm.set_pin_dirs(Direction::Out, &[&self.pin]);
        self.rx_sm.set_pin_dirs(Direction::Out, &[&self.pin]);

        // Set pin High
        self.tx_sm.set_pins(Level::High, &[&self.pin]);
        self.rx_sm.set_pins(Level::High, &[&self.pin]);

        // Restart TX state machine to prepare for transmission
        self.tx_sm.restart();

        // Enable TX state machine
        self.tx_sm.set_enable(true);
    }
}

impl Hardware for Hw<'_> {
    async fn send(&mut self, msg: u32) {
        self.enter_tx();
        self.tx_sm.tx().wait_push(msg).await;
        self.enter_rx().await;
    }

    async fn receive(&mut self) -> ReceivedOrTick {
        match select(self.rx_sm.rx().wait_pull(), self.ticker.next()).await {
            Either::First(x) => {
                self.ticker.reset();
                ReceivedOrTick::Some(x)
            }
            Either::Second(_) => {
                self.ticker.reset();
                ReceivedOrTick::Tick
            }
        }
    }

    // Set error state
    async fn set_error_state(&mut self, error: bool) {
        if error && !self.on_error {
            self.on_error = true;
        }
        if !error && self.on_error {
            self.on_error = false;
        }
    }
}

fn pio_freq() -> fixed::FixedU32<fixed::types::extra::U8> {
    (U56F8::from_num(clocks::clk_sys_freq()) / (8 * SPEED)).to_fixed()
}

fn task_tx<'a>(common: &mut PioCommon<'a>, mut sm: SmTx<'a>, pin: &mut PioPin<'a>) -> SmTx<'a> {
    sm.set_pins(Level::High, &[pin]);
    sm.set_pin_dirs(Direction::Out, &[pin]);
    pin.set_slew_rate(embassy_rp::gpio::SlewRate::Fast);
    pin.set_schmitt(true);

    let tx_prog = pio_asm!(
        ".wrap_target",
        ".side_set 1 opt",
        // Pull the data to send and keep line high
        "pull  block  side 1 [3]",
        // Set the counter to 31 bits and set line low to start sending
        "set   x, 31  side 0 [3]",
        "bitloop:"
        // Output a bit at a time, starting from the LSB
        "out   pins, 1",
        // Loop until all bits are sent and wait 2 cycles
        "jmp   x--, bitloop [2]",
        ".wrap"
    );
    let mut cfg = embassy_rp::pio::Config::default();

    cfg.use_program(&common.load_program(&tx_prog.program), &[pin]);
    cfg.set_set_pins(&[pin]);
    cfg.set_out_pins(&[pin]);
    cfg.clock_divider = pio_freq();
    cfg.shift_in.auto_fill = false;
    cfg.shift_in.direction = ShiftDirection::Right;
    cfg.shift_in.threshold = 32;
    cfg.shift_out.auto_fill = false;
    cfg.shift_out.direction = ShiftDirection::Right;
    cfg.shift_out.threshold = 32;
    cfg.fifo_join = FifoJoin::TxOnly;
    cfg.shift_in.auto_fill = false;
    sm.set_config(&cfg);

    sm.set_enable(true);
    sm
}

fn task_rx<'a>(common: &mut PioCommon<'a>, mut sm: SmRx<'a>, pin: &PioPin<'a>) -> SmRx<'a> {
    let rx_prog = pio_asm!(
        ".wrap_target",
        "start:",
        // Wait for the line to go low to start receiving
        "wait  0 pin, 0",
        // Set the counter to 31 bits to receive and wait 4 cycles
        "set   x, 31    [4]",
        "bitloop:",
        // Read a bit at a time, starting from the LSB
        "in    pins, 1",
        // Loop until all bits are received and wait 2 cycles
        "jmp   x--, bitloop [2]",
        // Push the received data to the FIFO
        "push block",
        // Wait for the line to go high to stop receiving the next byte
        "wait  1 pin, 0",
        ".wrap"
    );
    let mut cfg = embassy_rp::pio::Config::default();
    cfg.use_program(&common.load_program(&rx_prog.program), &[]);
    cfg.set_in_pins(&[pin]);
    cfg.set_jmp_pin(pin);
    cfg.clock_divider = pio_freq();
    cfg.shift_out.auto_fill = false;
    cfg.shift_out.direction = ShiftDirection::Right;
    cfg.shift_out.threshold = 32;
    cfg.shift_in.auto_fill = false;
    cfg.shift_in.direction = ShiftDirection::Right;
    cfg.shift_in.threshold = 32;
    cfg.fifo_join = FifoJoin::RxOnly;
    sm.set_config(&cfg);

    sm.set_enable(true);
    sm
}

// Test values in hex format
const T0: u32 = 0x00000000;
const T1: u32 = 0x11111111;
const T2: u32 = 0x22222222;
const T3: u32 = 0x33333333;
const T4: u32 = 0x44444444;
const T5: u32 = 0x55555555;
const T6: u32 = 0x66666666;
const T7: u32 = 0x77777777;
const T8: u32 = 0x88888888;
const T9: u32 = 0x99999999;
const TA: u32 = 0xaaaaaaaa;
const TB: u32 = 0xbbbbbbbb;
const TC: u32 = 0xcccccccc;
const TD: u32 = 0xdddddddd;
const TE: u32 = 0xeeeeeeee;
const TF: u32 = 0xffffffff;
const B: u32 = 0x5033070a; // b'P', 3, 7, b'\n'
const C: u32 = 0x33000033;
const M: u32 = 0xff0307ff;

const TEST_DATA: [u32; 24] = [
    T0, T1, T3, T7, TA, T0, T1, T2, T3, T4, T5, T6, T7, T8, T9, TA, TB, TC, TD, TE, TF, B, C, M,
];

async fn ping_pong<'a>(
    mut pio1_common: PioCommon<'a>,
    sm0: SmTx<'a>,
    sm1: SmRx<'a>,
    gpio_pin1: PIN_1,
    status_led: &mut Output<'static>,
    is_right: bool,
) {
    let mut pio_pin = pio1_common.make_pio_pin(gpio_pin1);
    pio_pin.set_pull(Pull::Up);
    let tx_sm = task_tx(&mut pio1_common, sm0, &mut pio_pin);
    let rx_sm = task_rx(&mut pio1_common, sm1, &pio_pin);

    let mut hw = Hw::new(tx_sm, rx_sm, pio_pin);
    hw.enter_rx().await;

    let mut ticker = Ticker::every(Duration::from_millis(50));
    let mut idx = 0;
    let mut state = false;
    status_led.set_high();
    let mut num: u32 = 0;
    let mut errors: u32 = 0;
    loop {
        match select(ticker.next(), hw.receive()).await {
            Either::First(_n) => {
                if is_right {
                    idx = (idx + 1) % TEST_DATA.len();
                    num += 1;
                    let x = TEST_DATA[idx];
                    defmt::info!(
                        "[{}/{}] sending byte: 0b{:032b} 0x{:04x}",
                        errors,
                        num,
                        x,
                        x
                    );
                    hw.send(x).await;
                }
            }
            Either::Second(x) => {
                match x {
                    ReceivedOrTick::Some(x) => {
                        if state {
                            status_led.set_high();
                        } else {
                            status_led.set_low();
                        }
                        state = !state;
                        defmt::info!("[{}] got byte: 0b{:032b} 0x{:04x}", num, x, x);
                        if !is_right {
                            // Send back the received byte
                            hw.send(x).await;
                        } else if x != TEST_DATA[idx] {
                            defmt::error!(
                                "[{}] got byte: 0b{:032b} 0x{:04x}, expecting 0b{:032b} 0x{:04x}",
                                num,
                                x,
                                x,
                                TEST_DATA[idx],
                                TEST_DATA[idx]
                            );
                            errors += 1;
                        }
                    }
                    ReceivedOrTick::Tick => {
                        defmt::info!("[{}] tick", num);
                    }
                }
            }
        }
    }
}

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    defmt::info!("Hello there!");

    let p = embassy_rp::init(Default::default());
    let pio1 = Pio::new(p.PIO1, PioIrq1);

    let mut status_led = Output::new(p.PIN_17, Level::Low);
    let is_right = Input::new(p.PIN_29, Pull::Up).is_high();
    ping_pong(
        pio1.common,
        pio1.sm0,
        pio1.sm1,
        p.PIN_1,
        &mut status_led,
        is_right,
    )
    .await;
}
