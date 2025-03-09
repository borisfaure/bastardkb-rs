use crate::core::LAYOUT_CHANNEL;
use crate::rgb_leds::{AnimCommand, ANIM_CHANNEL};
use embassy_executor::Spawner;
use embassy_futures::{
    select::{select, Either},
    yield_now,
};
use embassy_rp::{
    clocks,
    gpio::{Drive, Level, Output, Pull},
    peripherals::{PIN_1, PIO1},
    pio::{self, Direction, FifoJoin, ShiftDirection, StateMachine},
};
use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, channel::Channel};
use fixed::{traits::ToFixed, types::U56F8};
use keyberon::layout::Event as KBEvent;
use utils::protocol::{Hardware, SideProtocol};
use utils::serde::Event;

const SPEED: u64 = 460_800;

/// Number of events in the channel to the other half of the keyboard
const NB_EVENTS: usize = 16;
/// Channel to send `utils::serde::event` events to the layout handler
pub static SIDE_CHANNEL: Channel<ThreadModeRawMutex, Event, NB_EVENTS> = Channel::new();

const TX: usize = 0;
const RX: usize = 1;

pub type SmRx<'a> = StateMachine<'a, PIO1, { RX }>;
pub type SmTx<'a> = StateMachine<'a, PIO1, { TX }>;
pub type PioCommon<'a> = pio::Common<'a, PIO1>;
pub type PioPin<'a> = pio::Pin<'a, PIO1>;

struct SidesComms<W: Sized + Hardware> {
    /// Protocol to communicate with the other side
    protocol: SideProtocol<W>,
    /// Status LED
    status_led: Output<'static>,
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
}

impl<'a> Hw<'a> {
    pub fn new(tx_sm: SmTx<'a>, rx_sm: SmRx<'a>, pin: PioPin<'a>) -> Self {
        Self {
            tx_sm,
            rx_sm,
            pin,
            on_error: false,
        }
    }

    async fn enter_rx(&mut self) {
        // Wait for TX FIFO to empty
        while !self.tx_sm.tx().empty() {
            yield_now().await;
        }

        // Wait a bit after the last sent message
        cortex_m::asm::delay(100);

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

    async fn receive(&mut self) -> u32 {
        self.rx_sm.rx().wait_pull().await
    }

    // Set error state
    async fn set_error_state(&mut self, error: bool) {
        if error && !self.on_error {
            self.on_error = true;
            if ANIM_CHANNEL.is_full() {
                defmt::error!("Anim channel is full");
            }
            ANIM_CHANNEL.send(AnimCommand::Error).await;
        }
        if !error && self.on_error {
            self.on_error = false;
            if ANIM_CHANNEL.is_full() {
                defmt::error!("Anim channel is full");
            }
            ANIM_CHANNEL.send(AnimCommand::Fixed).await;
        }
    }
}

/// Process an event
async fn process_event(event: Event) {
    match event {
        Event::Noop => {}
        Event::Press(i, j) => {
            if LAYOUT_CHANNEL.is_full() {
                defmt::error!("Layout channel is full");
            }
            LAYOUT_CHANNEL.send(KBEvent::Press(i, j)).await;
        }
        Event::Release(i, j) => {
            if LAYOUT_CHANNEL.is_full() {
                defmt::error!("Layout channel is full");
            }
            LAYOUT_CHANNEL.send(KBEvent::Release(i, j)).await;
        }
        Event::RgbAnim(anim) => {
            if ANIM_CHANNEL.is_full() {
                defmt::error!("Anim channel is full");
            }
            ANIM_CHANNEL.send(AnimCommand::Set(anim)).await;
        }
        Event::RgbAnimChangeLayer(layer) => {
            if ANIM_CHANNEL.is_full() {
                defmt::error!("Anim channel is full");
            }
            ANIM_CHANNEL.send(AnimCommand::ChangeLayer(layer)).await;
        }
        Event::SeedRng(seed) => {
            todo!("Seed random {}", seed);
        }
        _ => {
            defmt::warn!("Unhandled event {:?}", defmt::Debug2Format(&event));
        }
    }
}

impl<W: Sized + Hardware> SidesComms<W> {
    /// Create a new event buffer
    pub fn new(name: &'static str, hw: W, status_led: Output<'static>) -> Self {
        Self {
            protocol: SideProtocol::new(hw, name),
            status_led,
        }
    }

    /// Run the communication between the two sides
    pub async fn run(&mut self) {
        // Wait for the other side to boot
        loop {
            match select(SIDE_CHANNEL.receive(), self.protocol.receive()).await {
                Either::First(event) => {
                    self.protocol.queue_event(event).await;
                }
                Either::Second(x) => {
                    self.status_led.set_low();
                    process_event(x).await;
                    self.status_led.set_high();
                }
            }
        }
    }
}

/// Frequency of the PIO state machine
fn pio_freq() -> fixed::FixedU32<fixed::types::extra::U8> {
    (U56F8::from_num(clocks::clk_sys_freq()) / (8 * SPEED)).to_fixed()
}

/// Task to send data
fn task_tx<'a>(common: &mut PioCommon<'a>, mut sm: SmTx<'a>, pin: &mut PioPin<'a>) -> SmTx<'a> {
    sm.set_pins(Level::High, &[pin]);
    sm.set_pin_dirs(Direction::Out, &[pin]);
    pin.set_slew_rate(embassy_rp::gpio::SlewRate::Fast);
    pin.set_schmitt(true);

    let tx_prog = pio_proc::pio_asm!(
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
    let rx_prog = pio_proc::pio_asm!(
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

#[embassy_executor::task]
async fn run(mut sides_comms: SidesComms<Hw<'static>>) {
    sides_comms.run().await;
}

pub async fn init(
    spawner: &Spawner,
    mut pio_common: PioCommon<'static>,
    sm0: SmTx<'static>,
    sm1: SmRx<'static>,
    gpio_pin1: PIN_1,
    status_led: Output<'static>,
    is_right: bool,
) {
    let mut pio_pin = pio_common.make_pio_pin(gpio_pin1);
    pio_pin.set_pull(Pull::Up);
    let tx_sm = task_tx(&mut pio_common, sm0, &mut pio_pin);
    let rx_sm = task_rx(&mut pio_common, sm1, &pio_pin);

    let name = if is_right { "Right" } else { "Left" };
    let mut hw = Hw::new(tx_sm, rx_sm, pio_pin);
    hw.enter_rx().await;
    let comms = SidesComms::new(name, hw, status_led);
    spawner.must_spawn(run(comms));
}
