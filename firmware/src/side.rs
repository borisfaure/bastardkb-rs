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

/// Size of the queue
const QUEUE_SIZE: usize = 32;

pub type SmRx<'a> = StateMachine<'a, PIO1, { RX }>;
pub type SmTx<'a> = StateMachine<'a, PIO1, { TX }>;
pub type PioCommon<'a> = pio::Common<'a, PIO1>;
pub type PioPin<'a> = pio::Pin<'a, PIO1>;

struct SidesComms<'a, W: Sized + Hardware> {
    protocol: SideProtocol<W>,

    /// Buffer of events to sent to the other half of the keyboard
    queued_buffer: [Option<Event>; QUEUE_SIZE],
    /// Number of queued events
    queued: usize,
    /// Next position in the queued buffer
    next_queued: usize,

    /// State machine to receive events
    rx_sm: SmRx<'a>,
    /// Status LED
    status_led: &'a mut Output<'static>,
}

struct SenderHw<'a> {
    /// State machine to send events
    tx: SmTx<'a>,
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
}

impl<'a, W: Sized + Hardware> SidesComms<'a, W> {
    /// Create a new event buffer
    pub fn new(sender_hw: W, rx_sm: SmRx<'a>, status_led: &'a mut Output<'static>) -> Self {
        Self {
            protocol: SideProtocol::new(sender_hw),
            queued_buffer: [None; QUEUE_SIZE],
            queued: 0,
            next_queued: 0,
            rx_sm,
            status_led,
        }
    }

    // Replay the event at the given sequence id
    //async fn replay_once(&mut self, sid: Sid) {
    //    if let Some(b) = self.sent_buffer[sid.as_usize()] {
    //        defmt::info!("Replaying event {}", deserialize(b).unwrap());
    //        self.tx_sm.tx().wait_push(b).await;
    //    } else {
    //        defmt::warn!("No event to replay at sid {}, send noop", sid);
    //        self.tx_sm
    //            .tx()
    //            .wait_push(serialize(Event::Noop, sid.v).unwrap())
    //            .await;
    //    }
    //}

    // Send an event bypassing the queue
    //async fn send_event(&mut self, event: Event) {
    //    defmt::info!("Sending event {} with sid {}", event, self.next_tx_sid.v);
    //    let b = serialize(event, self.next_tx_sid.v).unwrap();
    //    self.sent_buffer[self.next_tx_sid.as_usize()] = Some(b);
    //    self.next_tx_sid.next();
    //    self.tx_sm.tx().wait_push(b).await;
    //}

    // Pop the next event from the queue
    //fn pop_queued(&mut self) -> Event {
    //    for p in self.next_queued..(QUEUE_SIZE - 1) {
    //        if let Some(e) = self.queued_buffer[p] {
    //            self.queued_buffer[p] = None;
    //            self.queued -= 1;
    //            return e;
    //        }
    //    }
    //    for p in 0..self.next_queued {
    //        if let Some(e) = self.queued_buffer[p] {
    //            self.queued_buffer[p] = None;
    //            self.queued -= 1;
    //            return e;
    //        }
    //    }
    //    panic!("No event to pop");
    //}

    // Unqueue and send the next event
    //async fn unqueue(&mut self) {
    //    if self.queued != 0 {
    //        let event = self.pop_queued();
    //        self.send_event(event).await;
    //    }
    //}

    // Queue an event to the buffer and return its serialized value
    //async fn queue(&mut self, event: Event) {
    //    if self.queued > QUEUE_SIZE {
    //        defmt::warn!("Queue is full, dropping event {:?}", event);
    //    }
    //    self.queued_buffer[self.next_queued] = Some(event);
    //    self.queued += 1;
    //    self.next_queued += 1;
    //    if self.next_queued == QUEUE_SIZE {
    //        self.next_queued = 0;
    //    }
    //    self.unqueue().await;
    //}

    // Send an ACK for the given sequence id
    //async fn send_ack(&mut self, sid: u8) {
    //    defmt::info!("Sending ACK for sid {}", sid);
    //    self.send_event(Event::Ack(sid)).await;
    //}

    // Received an ACK for the given sequence id
    // This means the other side has received this event
    //async fn on_ack(&mut self, sid: u8) {
    //    // The buffer is a circular buffer
    //    self.sent_buffer[sid as usize] = None;
    //}

    // On invalid sequence id
    //async fn on_invalid_sid(&mut self, sid: Sid) {
    //    defmt::warn!(
    //        "Invalid sid received: expected {}, got {}",
    //        self.next_rx_sid,
    //        sid
    //    );
    //    if expected < got {
    //        for i in expected..=got {
    //            self.rx_errors[i as usize] = true;
    //        }
    //    } else {
    //        for i in expected..=SEQ_ID_MAX {
    //            self.rx_errors[i as usize] = true;
    //        }
    //        for i in 0..=got {
    //            self.rx_errors[i as usize] = true;
    //        }
    //    }
    //    Timer::after_millis(5).await;
    //    self.send_event(Event::Retransmit(expected)).await;
    //}

    // On error
    //async fn rx_error(&mut self) {
    //    if !self.rx_on_error {
    //        if ANIM_CHANNEL.is_full() {
    //            defmt::error!("Anim channel is full");
    //        }
    //        ANIM_CHANNEL.send(AnimCommand::Error).await;
    //        self.rx_on_error = true;
    //    }
    //}

    // Handle an error from the other side
    //async fn retransmit(&mut self, r: u8) {
    //    if !self.tx_errors[r as usize] {
    //        self.tx_errors[r as usize] = true;
    //        self.nb_tx_errors += 1;
    //    }
    //    self.replay_once(r).await;
    //}

    // On Ok event
    //pub async fn handle_received_event(&mut self, event: Event, sid: u8) {
    //    // Send an ACK for the event
    //    if event.needs_ack() || self.rx_on_error {
    //        self.send_ack(sid).await;
    //    }
    //    let mut next = sid + 1;
    //    if next > SEQ_ID_MAX {
    //        next = 0;
    //    }
    //    self.next_rx_sid = Some(next);
    //    self.process_event(event, sid).await;
    //    let next = if sid == SEQ_ID_MAX { 0 } else { sid + 1 };
    //    if self.rx_errors[next as usize] {
    //        self.send_event(Event::Retransmit(next)).await;
    //    }
    //    if self.rx_on_error && !event.is_error() {
    //        if ANIM_CHANNEL.is_full() {
    //            defmt::error!("Anim channel is full");
    //        }
    //        ANIM_CHANNEL.send(AnimCommand::Fixed).await;
    //        self.rx_on_error = false;
    //    }
    //}

    // Mark rx error
    //async fn mark_rx_error(&mut self, sid: u8) {
    //    if !self.rx_on_error {
    //        if ANIM_CHANNEL.is_full() {
    //            defmt::error!("Anim channel is full");
    //        }
    //        ANIM_CHANNEL.send(AnimCommand::Error).await;
    //        self.rx_on_error = true;
    //    }
    //}

    // On received event
    //pub async fn on_received_event(&mut self, x: u32) {
    //    match deserialize(x) {
    //        Ok((event, sid)) => {
    //            let sid = Sid { v: sid };
    //            defmt::info!(
    //                "Received [{}/{}] Event: {:?}",
    //                sid,
    //                self.next_rx_sid.v,
    //                defmt::Debug2Format(&event)
    //            );
    //            if self.next_rx_sid != sid {
    //                self.on_invalid_sid(sid).await;
    //            } else {
    //                self.handle_received_event(event, sid).await;
    //            }
    //        }
    //        Err(_) => {
    //            defmt::warn!("Unable to deserialize event: 0x{:04x}", x);
    //            Timer::after_millis(5).await;
    //            self.send_event(Event::Retransmit(self.next_rx_sid.v)).await;
    //        }
    //    }
    //}

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

    let sender_hw = SenderHw { tx: tx_sm };
    let mut sides_comms: SidesComms<'_, SenderHw<'_>> =
        SidesComms::new(sender_hw, rx_sm, status_led);
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
