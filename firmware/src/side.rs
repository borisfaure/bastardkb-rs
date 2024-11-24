use crate::core::LAYOUT_CHANNEL;
use crate::rgb_leds::{AnimCommand, ANIM_CHANNEL};
use embassy_futures::select::{select, Either};
use embassy_rp::clocks::clk_sys_freq;
use embassy_rp::gpio::{Level, Output};
use embassy_rp::peripherals::{PIN_1, PIN_29, PIO1};
use embassy_rp::pio::{self, Direction, FifoJoin, ShiftDirection, StateMachine};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};
use embassy_time::{Duration, Ticker, Timer};
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

pub type SmRx<'a> = StateMachine<'a, PIO1, { RX }>;
pub type SmTx<'a> = StateMachine<'a, PIO1, { TX }>;
pub type PioCommon<'a> = pio::Common<'a, PIO1>;
pub type PioPin<'a> = pio::Pin<'a, PIO1>;

struct EventBuffer<'a, 'b> {
    /// Buffer of events sent to the other half of the keyboard
    buffer: [u32; 256],
    /// Current sequence id
    last_sid: u8,

    /// State machine to send events
    sm: SmTx<'a>,

    // LED to light up when sending an event
    status_led: &'b mut Output<'static>,
}

impl<'a, 'b> EventBuffer<'a, 'b> {
    /// Create a new event buffer
    pub fn new(sm: SmTx<'a>, status_led: &'b mut Output<'static>) -> Self {
        Self {
            buffer: [0; 256],
            last_sid: u8::MAX,
            sm,
            status_led,
        }
    }

    /// Replay events from the given sid
    async fn replay_from(&mut self, first_sid: u8) {
        let start = first_sid as usize;
        let end = self.last_sid as usize;
        // The buffer is a circular buffer, so we need to iterate from sid
        // to the end of the buffer and then from the beginning of the buffer
        // to `self.last_sid` excluded.
        if start <= end {
            for b in self.buffer[start..=end].iter() {
                self.status_led.set_low();
                self.sm.tx().wait_push(*b).await;
                self.status_led.set_high();
            }
        } else {
            for b in self.buffer[start..]
                .iter()
                .chain(self.buffer[..=end].iter())
            {
                self.status_led.set_low();
                self.sm.tx().wait_push(*b).await;
                self.status_led.set_high();
            }
        }
    }

    /// Send an event to the buffer and return its serialized value
    pub async fn send(&mut self, event: Event) {
        self.last_sid = self.last_sid.wrapping_add(1);
        let b = serialize(event, self.last_sid);
        self.buffer[self.last_sid as usize] = b;
        self.status_led.set_low();
        self.sm.tx().wait_push(b).await;
        self.status_led.set_high();
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

    let tx_sm = task_tx(&mut pio_common, sm0, &mut pin_tx);
    let mut rx_sm = task_rx(&mut pio_common, sm1, &mut pin_rx);

    let mut tx_buffer = EventBuffer::new(tx_sm, status_led);
    let mut next_rx_sid;
    let mut rx_on_error = false;

    /* handshake */
    if is_right {
        // Wait for the other side to boot
        Timer::after_secs(2).await;
        let mut ticker = Ticker::every(Duration::from_secs(2));
        tx_buffer.send(Event::Hello).await;
        /* Wait for the other side to acknowledge the hello */
        loop {
            match select(ticker.next(), rx_sm.rx().wait_pull()).await {
                Either::First(_) => {
                    defmt::info!("Timeout waiting for Ack, resending Hello");
                    tx_buffer.send(Event::Hello).await;
                }
                Either::Second(x) => {
                    match deserialize(x) {
                        Ok((event, sid)) => match event {
                            Event::Ack(ack_sid) => {
                                defmt::warn!("[{}] Ack received about sid {}", sid, ack_sid);
                                next_rx_sid = sid.wrapping_add(1);
                                // Send Ack back to finish the handshake
                                tx_buffer.send(Event::Ack(sid)).await;
                                break;
                            }
                            Event::Error(r) => {
                                defmt::warn!("[{}] Error received about sid {}", sid, r);
                                Timer::after_millis(10).await;
                                tx_buffer.send(Event::Hello).await;
                            }
                            _ => {
                                defmt::warn!(
                                    "[{}] Invalid event received: {:?}",
                                    sid,
                                    defmt::Debug2Format(&event)
                                );
                                Timer::after_millis(10).await;
                                tx_buffer.send(Event::Hello).await;
                            }
                        },
                        Err(_) => {
                            defmt::warn!("Unable to deserialize event: 0x{:04x}", x);
                            Timer::after_millis(10).await;
                            tx_buffer.send(Event::Hello).await;
                        }
                    }
                }
            }
        }
    } else {
        loop {
            let x = rx_sm.rx().wait_pull().await;
            match deserialize(x) {
                Ok((event, sid)) => match event {
                    Event::Hello => {
                        defmt::info!("HS: [{}] Hello received", sid);
                        tx_buffer.send(Event::Ack(sid)).await;
                    }
                    Event::Ack(ack_sid) => {
                        defmt::warn!("HS: [{}] Ack received about sid {}", sid, ack_sid);
                        next_rx_sid = sid.wrapping_add(1);
                        break;
                    }
                    Event::Error(r) => {
                        defmt::warn!("HS: [{}] Error received about sid {}", sid, r);
                        Timer::after_millis(10).await;
                        tx_buffer.send(Event::Ack(r)).await;
                    }
                    _ => {
                        defmt::warn!(
                            "HS: [{}] Invalid event received: {:?}",
                            sid,
                            defmt::Debug2Format(&event)
                        );
                        Timer::after_millis(10).await;
                        tx_buffer.send(Event::Error(sid)).await;
                    }
                },
                Err(_) => {
                    defmt::warn!("HS: Unable to deserialize event: 0x{:04x}", x);
                    Timer::after_millis(10).await;
                    tx_buffer.send(Event::Error(0)).await;
                }
            }
        }
    }

    defmt::info!("Handshake completed");

    loop {
        match select(SIDE_CHANNEL.receive(), rx_sm.rx().wait_pull()).await {
            Either::First(event) => {
                tx_buffer.send(event).await;
            }
            Either::Second(x) => match deserialize(x) {
                Ok((event, sid)) => {
                    if sid != next_rx_sid {
                        defmt::warn!(
                            "Invalid sid received: expected {}, got {}",
                            next_rx_sid,
                            sid
                        );
                        Timer::after_millis(10).await;
                        tx_buffer.send(Event::Error(next_rx_sid)).await;
                        if ANIM_CHANNEL.is_full() {
                            defmt::error!("Anim channel is full");
                        }
                        ANIM_CHANNEL.send(AnimCommand::Error).await;
                        rx_on_error = true;
                    } else {
                        defmt::info!("Event: {:?}", defmt::Debug2Format(&event));
                        next_rx_sid = sid.wrapping_add(1);
                        if rx_on_error && !event.is_error() {
                            if ANIM_CHANNEL.is_full() {
                                defmt::error!("Anim channel is full");
                            }
                            ANIM_CHANNEL.send(AnimCommand::Fixed).await;
                            rx_on_error = false;
                        }
                        match event {
                            Event::Hello => {
                                defmt::warn!("Hello received");
                                tx_buffer.send(Event::Ack(sid)).await;
                            }
                            Event::Ack(_) => {
                                defmt::warn!("Unexpected Ack received");
                            }
                            Event::Error(r) => {
                                Timer::after_millis(10).await;
                                tx_buffer.replay_from(r).await;
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
                        }
                    }
                }
                Err(_) => {
                    defmt::warn!("Unable to deserialize event: 0x{:04x}", x);
                    Timer::after_millis(10).await;
                    tx_buffer.send(Event::Error(next_rx_sid)).await;
                    ANIM_CHANNEL.send(AnimCommand::Error).await;
                    rx_on_error = true;
                }
            },
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
