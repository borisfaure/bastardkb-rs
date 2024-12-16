use crate::core::LAYOUT_CHANNEL;
use crate::rgb_leds::{AnimCommand, ANIM_CHANNEL};
use embassy_futures::select::{select, Either};
use embassy_rp::clocks::clk_sys_freq;
use embassy_rp::gpio::{Level, Output};
use embassy_rp::peripherals::{PIN_1, PIN_29, PIO1};
use embassy_rp::pio::{self, Direction, FifoJoin, ShiftDirection, StateMachine};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};
use embassy_time::Timer;
use fixed::{traits::ToFixed, types::U56F8};
use keyberon::layout::Event as KBEvent;
use utils::protocol::{Hardware, SideProtocol};
use utils::serde::Event;

pub const USART_SPEED: u64 = 57600;

/// Number of events in the channel to the other half of the keyboard
const NB_EVENTS: usize = 64;
/// Channel to send `utils::serde::event` events to the layout handler
pub static SIDE_CHANNEL: Channel<CriticalSectionRawMutex, Event, NB_EVENTS> = Channel::new();

const TX: usize = 0;
const RX: usize = 1;

pub type SmRx<'a> = StateMachine<'a, PIO1, { RX }>;
pub type SmTx<'a> = StateMachine<'a, PIO1, { TX }>;
pub type PioCommon<'a> = pio::Common<'a, PIO1>;
pub type PioPin<'a> = pio::Pin<'a, PIO1>;

struct SidesComms<'a, W: Sized + Hardware> {
    /// Protocol to communicate with the other side
    protocol: SideProtocol<W>,
    /// State machine to receive events
    rx_sm: SmRx<'a>,
    /// Status LED
    status_led: &'a mut Output<'static>,
}

struct SenderHw<'a> {
    /// State machine to send events
    tx: SmTx<'a>,
    // error state
    on_error: bool,
}

impl<'a> SenderHw<'a> {
    pub fn new(tx: SmTx<'a>) -> Self {
        Self {
            tx,
            on_error: false,
        }
    }
}

impl<'a> Hardware for SenderHw<'a> {
    async fn send(&mut self, msg: u32) {
        self.tx.tx().wait_push(msg).await;
    }

    async fn wait_a_bit(&mut self) {
        Timer::after_millis(5).await;
    }

    /// Process an event
    async fn process_event(&mut self, event: Event) {
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

impl<'a, W: Sized + Hardware> SidesComms<'a, W> {
    /// Create a new event buffer
    pub fn new(
        name: &'static str,
        sender_hw: W,
        rx_sm: SmRx<'a>,
        status_led: &'a mut Output<'static>,
    ) -> Self {
        Self {
            protocol: SideProtocol::new(sender_hw, name),
            rx_sm,
            status_led,
        }
    }

    /// Run the communication between the two sides
    pub async fn run(&mut self) {
        // Wait for the other side to boot
        loop {
            match select(SIDE_CHANNEL.receive(), self.rx_sm.rx().wait_pull()).await {
                Either::First(event) => {
                    self.protocol.queue_event(event).await;
                }
                Either::Second(x) => {
                    self.status_led.set_low();
                    self.protocol.receive(x).await;
                    self.status_led.set_high();
                }
            }
        }
    }
}

pub async fn full_duplex_comm<'a>(
    mut pio_common: PioCommon<'a>,
    sm0: SmTx<'a>,
    sm1: SmRx<'a>,
    gpio_pin_1: PIN_1,
    gpio_pin_29: PIN_29,
    status_led: &mut Output<'static>,
    is_right: bool,
) {
    let (mut pin_tx, mut pin_rx) = if is_right {
        (
            pio_common.make_pio_pin(gpio_pin_29),
            pio_common.make_pio_pin(gpio_pin_1),
        )
    } else {
        (
            pio_common.make_pio_pin(gpio_pin_1),
            pio_common.make_pio_pin(gpio_pin_29),
        )
    };
    // Ensure everything is stable before starting the communication
    Timer::after_secs(6).await;

    let tx_sm = task_tx(&mut pio_common, sm0, &mut pin_tx);
    let rx_sm = task_rx(&mut pio_common, sm1, &mut pin_rx);

    let name = if is_right { "Right" } else { "Left" };
    let sender_hw = SenderHw::new(tx_sm);
    let mut sides_comms: SidesComms<'_, SenderHw<'_>> =
        SidesComms::new(name, sender_hw, rx_sm, status_led);
    sides_comms.run().await;
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
    let rx_prog = pio_proc::pio_file!("src/rx.pio");

    let mut cfg = embassy_rp::pio::Config::default();
    cfg.use_program(&common.load_program(&rx_prog.program), &[]);

    sm_rx.set_pins(Level::High, &[rx_pin]);
    cfg.set_in_pins(&[rx_pin]);
    cfg.set_jmp_pin(rx_pin);
    sm_rx.set_pin_dirs(Direction::In, &[rx_pin]);

    cfg.clock_divider = pio_freq();
    cfg.shift_in.auto_fill = false;
    cfg.shift_in.direction = ShiftDirection::Right;
    cfg.shift_in.threshold = 32;
    cfg.fifo_join = FifoJoin::RxOnly;
    sm_rx.set_config(&cfg);
    sm_rx.set_enable(true);

    sm_rx
}
