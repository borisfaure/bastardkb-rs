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
    Peri,
};
use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, channel::Channel};
use embassy_time::{Duration, Ticker};
use fixed::{traits::ToFixed, types::U56F8};
use futures::future;
use utils::protocol::{Hardware, ReceivedOrTick, SideProtocol};
use utils::serde::Event;

use {defmt_rtt as _, panic_probe as _};

bind_interrupts!(struct PioIrq1 {
    PIO1_IRQ_0 => PioInterruptHandler<PIO1>;
});

/// Number of events in the channel to the other half of the keyboard
const NB_EVENTS: usize = 8;
/// Channel to send `utils::serde::event` events to the layout handler
pub static SIDE_CHANNEL: Channel<ThreadModeRawMutex, Event, NB_EVENTS> = Channel::new();

const TX: usize = 0;
const RX: usize = 1;

type SmRx<'a> = StateMachine<'a, PIO1, { RX }>;
type SmTx<'a> = StateMachine<'a, PIO1, { TX }>;
type PioCommon<'a> = Common<'a, PIO1>;
type PioPin<'a> = pio::Pin<'a, PIO1>;

const SPEED: u64 = 460_800;

struct SidesComms<'a, W: Sized + Hardware> {
    /// Protocol to communicate with the other side
    protocol: SideProtocol<W>,
    /// Status LED
    status_led: &'a mut Output<'static>,
}

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

        // Calculate delay needed: time for ~34 bits at current speed
        // 32 data bits + start bit + stop bit = ~34 bits
        // At 460800 bps: 34 bits = ~74 microseconds
        // Convert to CPU cycles at 125MHz: 74us * 125 = 9250 cycles
        // Add extra time for the other side to switch from RX to TX
        let sys_freq = clocks::clk_sys_freq() as u64;
        let bit_time_cycles = (sys_freq * 50) / SPEED; // Increased from 34 to 50 bits worth of time
        cortex_m::asm::delay(bit_time_cycles as u32);

        // Disable TX state machine before manipulating TX pin
        self.tx_sm.set_enable(false);

        // Set minimal drive strength on TX pin to avoid interfering with RX
        self.pin.set_drive_strength(Drive::_2mA);

        // Set pin as input
        self.rx_sm.set_pin_dirs(Direction::In, &[&self.pin]);
        self.tx_sm.set_pin_dirs(Direction::In, &[&self.pin]);

        // Clear RX FIFO before restarting
        while !self.rx_sm.rx().empty() {
            let _ = self.rx_sm.rx().try_pull();
        }

        // Restart RX state machine to prepare for reception
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

/// Process an event
fn process_event(event: Event) {
    defmt::info!("Process event: {:?}", event);
}

impl<'a, W: Sized + Hardware> SidesComms<'a, W> {
    /// Create a new event buffer
    pub fn new(
        name: &'static str,
        hw: W,
        status_led: &'a mut Output<'static>,
        is_master: bool,
    ) -> Self {
        Self {
            protocol: SideProtocol::new(hw, name, is_master),
            status_led,
        }
    }

    /// Run the communication between the two sides
    pub async fn run(&mut self) {
        // Wait for the other side to boot
        loop {
            //match select(SIDE_CHANNEL.receive(), self.protocol.hw.receive()).await {
            match select(SIDE_CHANNEL.receive(), self.protocol.receive()).await {
                Either::First(event) => {
                    self.protocol.queue_event(event).await;
                }
                Either::Second(x) => {
                    self.status_led.set_low();
                    process_event(x);
                    self.status_led.set_high();
                }
            }
        }
    }
}

fn pio_freq() -> fixed::FixedU32<fixed::types::extra::U8> {
    (U56F8::from_num(clocks::clk_sys_freq()) / (8 * SPEED)).to_fixed()
}

/// Task to send data
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

/// Task to receive data
fn task_rx<'a>(common: &mut PioCommon<'a>, mut sm: SmRx<'a>, pin: &PioPin<'a>) -> SmRx<'a> {
    let rx_prog = pio_asm!(
        ".wrap_target",
        "start:",
        // Wait for the line to go low to start receiving (start bit detected)
        "wait  0 pin, 0",
        // TX timing: each bit is 4 PIO cycles
        //   Start bit: "set x, 31 side 0 [3]" = 4 cycles
        //   Data bits: "out pins, 1" + "jmp x--, bitloop [2]" = 1+1+2 = 4 cycles
        // After wait completes, we're ~1 cycle into start bit
        // Need to wait 3 more cycles of start bit + 2 cycles to middle of first data bit = 5 total
        // But we get 2 from "set x, 31 [1]", so wait 3 more with nop
        "nop [2]",     // Wait 3 cycles
        "set   x, 31", // Wait 1 cycle, then start sampling
        "bitloop:",
        // Sample the bit in middle
        "in    pins, 1",
        // Wait 2 more cycles, then loop (total 4 cycles per bit: in[1] + jmp[1] + delay[2])
        "jmp   x--, bitloop [2]",
        // Push the received data to the FIFO
        "push block",
        // Wait for the line to go high before next transmission
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

async fn ping_pong<'a>(
    mut pio1_common: PioCommon<'a>,
    sm0: SmTx<'a>,
    sm1: SmRx<'a>,
    gpio_pin1: Peri<'static, PIN_1>,
    status_led: &mut Output<'static>,
    is_right: bool,
) {
    let mut pio_pin = pio1_common.make_pio_pin(gpio_pin1);
    pio_pin.set_pull(Pull::Up);
    let tx_sm = task_tx(&mut pio1_common, sm0, &mut pio_pin);
    let rx_sm = task_rx(&mut pio1_common, sm1, &pio_pin);

    let name = if is_right { "Right" } else { "Left" };
    let mut hw = Hw::new(tx_sm, rx_sm, pio_pin);
    hw.enter_rx().await;
    let mut sides_comms: SidesComms<'_, Hw<'_>> = SidesComms::new(name, hw, status_led, is_right);
    sides_comms.run().await;
}

async fn sender(is_right: bool) {
    let duration = if is_right {
        Duration::from_secs(3)
    } else {
        Duration::from_millis(200)
    };
    let mut ticker = Ticker::every(duration);
    ticker.next().await;
    let mut press_sent = false;
    loop {
        if is_right {
            SIDE_CHANNEL.send(Event::Ping).await;
        } else {
            if press_sent {
                SIDE_CHANNEL.send(Event::Press(0, 0)).await;
            } else {
                SIDE_CHANNEL.send(Event::Release(0, 0)).await;
            }
            press_sent = !press_sent;
        }
        ticker.next().await;
    }
}

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    defmt::info!("Hello there!");

    let p = embassy_rp::init(Default::default());
    let pio1 = Pio::new(p.PIO1, PioIrq1);

    let mut status_led = Output::new(p.PIN_17, Level::Low);
    let is_right = Input::new(p.PIN_29, Pull::Up).is_high();
    let pp_fut = ping_pong(
        pio1.common,
        pio1.sm0,
        pio1.sm1,
        p.PIN_1,
        &mut status_led,
        is_right,
    );
    let sender_fut = sender(is_right);

    future::join(pp_fut, sender_fut).await;
}
