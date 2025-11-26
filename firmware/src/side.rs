use crate::core::LAYOUT_CHANNEL;
use crate::rgb_leds::{AnimCommand, ANIM_CHANNEL};
use embassy_executor::Spawner;
use embassy_futures::select::{select, Either};
#[cfg(feature = "dilemma")]
use embassy_rp::peripherals::PIN_1;
#[cfg(feature = "cnano")]
use embassy_rp::peripherals::PIN_29;
use embassy_rp::{
    clocks,
    gpio::{Level, Output, Pull},
    peripherals::PIO1,
    pio::{self, program::pio_asm, Direction, ShiftDirection, StateMachine},
    Peri,
};
use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, channel::Channel};
use embassy_time::{Duration, Instant, Ticker};
use fixed::{traits::ToFixed, types::U56F8};
use keyberon::layout::Event as KBEvent;
#[cfg(feature = "defmt")]
use utils::log::Debug2Format;
use utils::log::{error, info, warn};
use utils::protocol::{Hardware, SideProtocol};
use utils::serde::Event;

/// Speed of the PIO state machine, in bps
const SPEED: u64 = 460_800;

/// Number of events in the channel to the other half of the keyboard
const NB_EVENTS: usize = 16;
/// Channel to send `utils::serde::event` events to the layout handler
pub static SIDE_CHANNEL: Channel<ThreadModeRawMutex, Event, NB_EVENTS> = Channel::new();

/// Hardware queue size (for decoupling protocol from hardware timing)
const HW_QUEUE_SIZE: usize = 128;
/// Hardware TX queue: protocol layer queues messages here to be sent
static HW_TX_QUEUE: Channel<ThreadModeRawMutex, u32, HW_QUEUE_SIZE> = Channel::new();
/// Hardware RX queue: hardware task places received messages here
static HW_RX_QUEUE: Channel<ThreadModeRawMutex, u32, HW_QUEUE_SIZE> = Channel::new();

/// Compound state machine that handles both TX and RX
pub type SmCompound<'a> = StateMachine<'a, PIO1, 0>;
pub type PioCommon<'a> = pio::Common<'a, PIO1>;
pub type PioPin<'a> = pio::Pin<'a, PIO1>;

struct SidesComms<W: Sized + Hardware> {
    /// Protocol to communicate with the other side
    protocol: SideProtocol<W>,
    /// Status LED
    status_led: Output<'static>,
    /// Message statistics: real messages sent counter
    msg_sent_real: usize,
    /// Message statistics: noop messages sent counter
    msg_sent_noop: usize,
    /// Message statistics: real messages received counter
    msg_received_real: usize,
    /// Message statistics: noop messages received counter
    msg_received_noop: usize,
    /// Message statistics: last report time
    msg_stats_last_report: Instant,
}

/// Protocol layer Hardware implementation - interfaces with queues
struct HwProtocol {
    on_error: bool,
}

impl Hardware for HwProtocol {
    async fn queue_send(&mut self, msg: u32) {
        if HW_TX_QUEUE.is_full() {
            error!("HW TX queue is full");
        }
        HW_TX_QUEUE.send(msg).await;
    }

    async fn receive(&mut self) -> u32 {
        HW_RX_QUEUE.receive().await
    }

    // Set error state
    async fn set_error_state(&mut self, error: bool) {
        if error && !self.on_error {
            self.on_error = true;
            if ANIM_CHANNEL.is_full() {
                error!("Anim channel is full");
            }
            ANIM_CHANNEL.send(AnimCommand::Error).await;
        }
        if !error && self.on_error {
            self.on_error = false;
            if ANIM_CHANNEL.is_full() {
                error!("Anim channel is full");
            }
            ANIM_CHANNEL.send(AnimCommand::Fixed).await;
        }
    }
}

/// Independent hardware task that maintains strict 1ms bidirectional communication
/// This runs independently and maintains continuous 1ms timing
#[embassy_executor::task]
async fn hardware_task(mut sm: SmCompound<'static>) {
    info!(
        "Starting side comms hardware task (PIO SM0 at {} bps)",
        SPEED
    );
    let mut ticker = Ticker::every(Duration::from_millis(1));

    let mut tick_count: u32 = 0;
    let mut next_log: u32 = 1;
    loop {
        ticker.next().await;
        tick_count = tick_count.wrapping_add(1);
        if tick_count == next_log {
            info!("Side comms running... (tick_count={})", tick_count);
            next_log = next_log.wrapping_mul(2);
        }

        // ALWAYS send something to maintain 1ms timing
        let msg_to_send = HW_TX_QUEUE.try_receive().unwrap_or_default();

        // Send via PIO (compound state machine handles TX automatically)
        sm.tx().wait_push(msg_to_send).await;

        // Check if we received anything (non-blocking)
        if sm.rx().level() > 0 {
            let received_msg = sm.rx().wait_pull().await;
            // Filter out keepalive messages (0x00000000)
            if received_msg != 0x00000000 {
                // Queue it for the protocol layer (non-blocking)
                let _ = HW_RX_QUEUE.try_send(received_msg);
            }
        }
    }
}

/// Process an event
async fn process_event(event: Event) {
    match event {
        Event::Noop => {}
        Event::Press(i, j) => {
            if LAYOUT_CHANNEL.is_full() {
                error!("Layout channel is full");
            }
            LAYOUT_CHANNEL.send(KBEvent::Press(i, j)).await;
        }
        Event::Release(i, j) => {
            if LAYOUT_CHANNEL.is_full() {
                error!("Layout channel is full");
            }
            LAYOUT_CHANNEL.send(KBEvent::Release(i, j)).await;
        }
        Event::RgbAnim(anim) => {
            if ANIM_CHANNEL.is_full() {
                error!("Anim channel is full");
            }
            ANIM_CHANNEL.send(AnimCommand::Set(anim)).await;
        }
        Event::RgbAnimChangeLayer(layer) => {
            if ANIM_CHANNEL.is_full() {
                error!("Anim channel is full");
            }
            ANIM_CHANNEL.send(AnimCommand::ChangeLayer(layer)).await;
        }
        Event::SeedRng(seed) => {
            todo!("Seed random {}", seed);
        }
        _ => {
            warn!("Unhandled event {:?}", Debug2Format(&event));
        }
    }
}

impl<W: Sized + Hardware> SidesComms<W> {
    /// Create a new event buffer
    pub fn new(
        #[cfg(feature = "defmt")] name: &'static str,
        hw: W,
        status_led: Output<'static>,
    ) -> Self {
        Self {
            protocol: SideProtocol::new(
                hw,
                #[cfg(feature = "defmt")]
                name,
            ),
            status_led,
            msg_sent_real: 0,
            msg_sent_noop: 0,
            msg_received_real: 0,
            msg_received_noop: 0,
            msg_stats_last_report: Instant::now(),
        }
    }

    /// Run the communication between the two sides
    pub async fn run(&mut self) {
        // Wait for the other side to boot
        loop {
            // Check if it's time to report stats (non-blocking)
            let now = Instant::now();
            if now.duration_since(self.msg_stats_last_report) >= Duration::from_secs(5) {
                info!(
                    "[MSG_STATS] sent: real={} noop={} | received: real={} noop={} (in last ~5s)",
                    self.msg_sent_real,
                    self.msg_sent_noop,
                    self.msg_received_real,
                    self.msg_received_noop
                );
                self.msg_sent_real = 0;
                self.msg_sent_noop = 0;
                self.msg_received_real = 0;
                self.msg_received_noop = 0;
                self.msg_stats_last_report = now;
            }

            let result = select(SIDE_CHANNEL.receive(), self.protocol.receive()).await;

            match result {
                Either::First(event) => {
                    // Track noop vs real messages
                    if matches!(event, Event::Noop) {
                        self.msg_sent_noop += 1;
                    } else {
                        self.msg_sent_real += 1;
                    }

                    self.protocol.queue_event(event).await;
                }
                Either::Second(x) => {
                    #[cfg(feature = "cnano")]
                    self.status_led.set_low();
                    #[cfg(feature = "dilemma")]
                    self.status_led.set_high();
                    process_event(x).await;
                    #[cfg(feature = "cnano")]
                    self.status_led.set_high();
                    #[cfg(feature = "dilemma")]
                    self.status_led.set_low();

                    // Track noop vs real messages
                    if matches!(x, Event::Noop) {
                        self.msg_received_noop += 1;
                    } else {
                        self.msg_received_real += 1;
                    }
                }
            }
        }
    }
}

/// Frequency of the PIO state machine
fn pio_freq() -> fixed::FixedU32<fixed::types::extra::U8> {
    (U56F8::from_num(clocks::clk_sys_freq()) / (8 * SPEED)).to_fixed()
}

/// Master: Transmit first, then receive
/// Used by the right side (master)
fn setup_master_compound(
    common: &mut PioCommon<'static>,
    mut sm: SmCompound<'static>,
    pin: &mut PioPin<'static>,
) -> SmCompound<'static> {
    sm.set_pins(Level::High, &[pin]);
    sm.set_pin_dirs(Direction::Out, &[pin]);
    pin.set_slew_rate(embassy_rp::gpio::SlewRate::Fast);
    pin.set_schmitt(true);

    let prog = pio_asm!(
        ".wrap_target",
        // === TX Phase ===
        "pull block",      // Get data to transmit
        "set pindirs, 1",  // Pin as output
        "set pins, 1 [3]", // Idle high
        "set x, 31",       // Counter for 32 bits
        "set pins, 0 [3]", // Start bit low
        "tx_loop:",
        "out pins, 1",          // Output data bit
        "jmp x--, tx_loop [2]", // Loop (4 cycles per bit)
        "set pins, 1 [7]",      // Return to idle, delay for slave
        // === Switch to RX ===
        "set pindirs, 0", // Pin as input
        // === RX Phase ===
        "wait 0 pin, 0", // Wait for slave's start bit
        "nop [2]",       // Align to middle of first bit
        "set x, 31",     // Counter for 32 bits
        "rx_loop:",
        "in pins, 1",           // Sample bit
        "jmp x--, rx_loop [2]", // Loop (4 cycles per bit)
        "push block",           // Push received data
        "wait 1 pin, 0",        // Wait for idle
        ".wrap"
    );

    let mut cfg = embassy_rp::pio::Config::default();
    cfg.use_program(&common.load_program(&prog.program), &[]);
    cfg.set_set_pins(&[pin]);
    cfg.set_out_pins(&[pin]);
    cfg.set_in_pins(&[pin]);
    cfg.clock_divider = pio_freq();
    cfg.shift_out.auto_fill = false;
    cfg.shift_out.direction = ShiftDirection::Right;
    cfg.shift_out.threshold = 32;
    cfg.shift_in.auto_fill = false;
    cfg.shift_in.direction = ShiftDirection::Right;
    cfg.shift_in.threshold = 32;
    // Don't join FIFOs - need both TX and RX
    sm.set_config(&cfg);

    sm.set_enable(true);
    sm
}

/// Slave: Receive first, then transmit
/// Used by the left side (slave)
fn setup_slave_compound(
    common: &mut PioCommon<'static>,
    mut sm: SmCompound<'static>,
    pin: &PioPin<'static>,
) -> SmCompound<'static> {
    let prog = pio_asm!(
        ".wrap_target",
        // === RX Phase (slave receives first) ===
        "set pindirs, 0", // Pin as input
        "wait 0 pin, 0",  // Wait for master's start bit
        "nop [2]",        // Align to middle of first bit
        "set x, 31",      // Counter for 32 bits
        "rx_loop:",
        "in pins, 1",           // Sample bit
        "jmp x--, rx_loop [2]", // Loop (4 cycles per bit)
        "push block",           // Push received data
        "wait 1 pin, 0",        // Wait for idle
        // === Switch to TX ===
        "pull block",     // Get response data (wait for CPU to provide it)
        "set pindirs, 1", // Pin as output
        // === TX Phase ===
        "set pins, 1 [3]", // Idle high
        "set x, 31",       // Counter for 32 bits
        "set pins, 0 [3]", // Start bit low
        "tx_loop:",
        "out pins, 1",          // Output data bit
        "jmp x--, tx_loop [2]", // Loop (4 cycles per bit)
        "set pins, 1 [7]",      // Return to idle with delay
        ".wrap"
    );

    let mut cfg = embassy_rp::pio::Config::default();
    cfg.use_program(&common.load_program(&prog.program), &[]);
    cfg.set_set_pins(&[pin]);
    cfg.set_out_pins(&[pin]);
    cfg.set_in_pins(&[pin]);
    cfg.clock_divider = pio_freq();
    cfg.shift_out.auto_fill = false;
    cfg.shift_out.direction = ShiftDirection::Right;
    cfg.shift_out.threshold = 32;
    cfg.shift_in.auto_fill = false;
    cfg.shift_in.direction = ShiftDirection::Right;
    cfg.shift_in.threshold = 32;
    // Don't join FIFOs - need both TX and RX
    sm.set_config(&cfg);

    sm.set_enable(true);
    sm
}

#[embassy_executor::task]
async fn run(mut sides_comms: SidesComms<HwProtocol>) {
    info!("Starting side comms protocol task...");
    sides_comms.run().await;
}

pub async fn init(
    spawner: &Spawner,
    mut pio_common: PioCommon<'static>,
    sm0: SmCompound<'static>,
    #[cfg(feature = "cnano")] gpio_pin: Peri<'static, PIN_29>,
    #[cfg(feature = "dilemma")] gpio_pin: Peri<'static, PIN_1>,
    status_led: Output<'static>,
    is_right: bool,
) {
    let mut pio_pin = pio_common.make_pio_pin(gpio_pin);
    pio_pin.set_pull(Pull::Up);

    info!("Side is {}", if is_right { "Right" } else { "Left" });

    info!("Setting up PIO side communication...");
    // Setup compound PIO state machine (master or slave)
    let sm = if is_right {
        setup_master_compound(&mut pio_common, sm0, &mut pio_pin)
    } else {
        setup_slave_compound(&mut pio_common, sm0, &pio_pin)
    };

    info!("setup complete");

    // Spawn the hardware task that maintains 1ms timing
    spawner.must_spawn(hardware_task(sm));
    info!("hardware task spawned");

    #[cfg(feature = "defmt")]
    let name = if is_right { "Right" } else { "Left" };
    // Create protocol instance with queue-based hardware interface
    let protocol_hw = HwProtocol { on_error: false };
    let comms = SidesComms::new(
        #[cfg(feature = "defmt")]
        name,
        protocol_hw,
        status_led,
    );
    spawner.must_spawn(run(comms));
    info!("protocol task spawned");
}
