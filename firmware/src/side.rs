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
use utils::serde::{deserialize, serialize, Event};

pub const USART_SPEED: u64 = 57600;

/// Number of events in the channel to the other half of the keyboard
const NB_EVENTS: usize = 64;
/// Channel to send `utils::serde::event` events to the layout handler
pub static SIDE_CHANNEL: Channel<CriticalSectionRawMutex, Event, NB_EVENTS> = Channel::new();

const TX: usize = 0;
const RX: usize = 1;

const SAVED_EVENTS: usize = 32;

pub type SmRx<'a> = StateMachine<'a, PIO1, { RX }>;
pub type SmTx<'a> = StateMachine<'a, PIO1, { TX }>;
pub type PioCommon<'a> = pio::Common<'a, PIO1>;
pub type PioPin<'a> = pio::Pin<'a, PIO1>;

struct EventBuffer<'a> {
    /// Buffer of events sent to the other half of the keyboard
    buffer: [u32; SAVED_EVENTS],
    /// Current sequence id
    last_sid: u8,

    /// State machine to send events
    sm: SmTx<'a>,
}

impl<'a> EventBuffer<'a> {
    /// Create a new event buffer
    pub fn new(sm: SmTx<'a>) -> Self {
        let mut buffer = [0; SAVED_EVENTS];
        for (i, n) in buffer.iter_mut().enumerate() {
            *n = serialize(Event::Hello, i as u8).unwrap();
        }
        Self {
            buffer,
            last_sid: SAVED_EVENTS as u8 - 1,
            sm,
        }
    }

    /// Set the current sequence id
    pub fn set_sequence_id(&mut self, sid: u8) {
        self.last_sid = sid;
    }

    /// Replay the event at the given sequence id
    async fn replay_once(&mut self, sid: u8) {
        let b = self.buffer[sid as usize];
        defmt::info!("Replaying event {}", deserialize(b).unwrap());
        self.sm.tx().wait_push(b).await;
    }

    // Replay all faulty events
    async fn replay_from(&mut self, first_sid: u8) {
        let start = first_sid as usize;
        let end = self.last_sid as usize;
        // The buffer is a circular buffer, so we need to iterate from sid
        // to the end of the buffer and then from the beginning of the buffer
        // to `self.last_sid` excluded.
        defmt::info!("Replaying from {} to {}", start, end);
        if start <= end {
            for b in self.buffer[start..=end].iter() {
                Timer::after_millis(50).await;
                defmt::info!("Replaying event {}", deserialize(*b).unwrap());
                self.sm.tx().wait_push(*b).await;
            }
        } else {
            for b in self.buffer[start..]
                .iter()
                .chain(self.buffer[..=end].iter())
            {
                Timer::after_millis(50).await;
                defmt::info!("Replaying event {}", deserialize(*b).unwrap());
                self.sm.tx().wait_push(*b).await;
            }
        }
    }

    /// Send an event to the buffer and return its serialized value
    pub async fn send(&mut self, event: Event) {
        self.last_sid += 1;
        if self.last_sid == SAVED_EVENTS as u8 {
            self.last_sid = 0;
        }
        defmt::info!("Sending event {} with sid {}", event, self.last_sid);
        let b = serialize(event, self.last_sid).unwrap();
        self.buffer[self.last_sid as usize] = b;
        self.sm.tx().wait_push(b).await;
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
    let mut rx_sm = task_rx(&mut pio_common, sm1, &mut pin_rx);

    let mut tx_buffer = EventBuffer::new(tx_sm);
    let mut next_rx_sid: Option<u8> = None;
    let mut handshake = false;
    let mut rx_on_error = false;

    // Wait for the other side to boot
    loop {
        match select(SIDE_CHANNEL.receive(), rx_sm.rx().wait_pull()).await {
            Either::First(event) => {
                tx_buffer.send(event).await;
            }
            Either::Second(x) => {
                status_led.set_low();
                match deserialize(x) {
                    Ok((event, sid)) => match next_rx_sid {
                        Some(next) if sid != next => {
                            defmt::warn!(
                                "Invalid sid received: expected {}, got {} for event {:?}",
                                next,
                                sid,
                                defmt::Debug2Format(&event)
                            );
                            Timer::after_millis(10).await;
                            tx_buffer.send(Event::Error(next)).await;
                            if !rx_on_error {
                                if ANIM_CHANNEL.is_full() {
                                    defmt::error!("Anim channel is full");
                                }
                                ANIM_CHANNEL.send(AnimCommand::Error).await;
                                next_rx_sid = Some(next);
                            }
                        }
                        _ => {
                            defmt::info!(
                                "Received [{}] Event: {:?}",
                                sid,
                                defmt::Debug2Format(&event)
                            );
                            if rx_on_error && !event.is_error() {
                                rx_on_error = false;
                                if ANIM_CHANNEL.is_full() {
                                    defmt::error!("Anim channel is full");
                                }
                                ANIM_CHANNEL.send(AnimCommand::Fixed).await;
                            }
                            let mut next = sid + 1;
                            if next == SAVED_EVENTS as u8 {
                                next = 0;
                            }
                            next_rx_sid = Some(next);
                            match event {
                                Event::Hello => {
                                    tx_buffer.send(Event::Ack(sid)).await;
                                }
                                Event::Ack(_) => {}
                                Event::Error(r) => {
                                    Timer::after_millis(10).await;
                                    if !handshake {
                                        tx_buffer.set_sequence_id(r);
                                        handshake = true;
                                        tx_buffer.replay_once(r).await;
                                    } else {
                                        tx_buffer.replay_from(r).await;
                                    }
                                }
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
                            }
                        }
                    },
                    Err(_) => {
                        defmt::warn!("Unable to deserialize event: 0x{:04x}", x);
                        Timer::after_millis(10).await;
                        if let Some(sid) = next_rx_sid {
                            tx_buffer.send(Event::Error(sid)).await;
                        }
                        rx_on_error = true;
                    }
                }
                status_led.set_high();
            }
        }
    }
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
