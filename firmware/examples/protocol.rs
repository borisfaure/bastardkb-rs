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
use embassy_rp::{
    bind_interrupts, clocks,
    gpio::{Input, Level, Output, Pull},
    peripherals::{PIN_1, PIO1},
    pio::{
        self, program::pio_asm, Common, Direction, InterruptHandler as PioInterruptHandler, Pio,
        ShiftDirection, StateMachine,
    },
    Peri,
};
use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, channel::Channel};
use embassy_time::{Duration, Ticker};
use fixed::{traits::ToFixed, types::U56F8};
use futures::future;
#[cfg(not(feature = "defmt"))]
use panic_halt as _;
use utils::log::info;
use utils::protocol::{Hardware, SideProtocol};
use utils::serde::Event;
#[cfg(feature = "defmt")]
use {defmt_rtt as _, panic_probe as _};

bind_interrupts!(struct PioIrq1 {
    PIO1_IRQ_0 => PioInterruptHandler<PIO1>;
});

/// Number of events in the channel to the other half of the keyboard
const NB_EVENTS: usize = 8;
/// Channel to send `utils::serde::event` events to the layout handler
pub static SIDE_CHANNEL: Channel<ThreadModeRawMutex, Event, NB_EVENTS> = Channel::new();

/// Hardware queue size for TX/RX messages
const HW_QUEUE_SIZE: usize = 16;
/// Hardware TX queue: protocol layer queues messages here to be sent by hardware task
static HW_TX_QUEUE: Channel<ThreadModeRawMutex, u32, HW_QUEUE_SIZE> = Channel::new();
/// Hardware RX queue: hardware task places received messages here for protocol layer
static HW_RX_QUEUE: Channel<ThreadModeRawMutex, u32, HW_QUEUE_SIZE> = Channel::new();

type SmCompound<'a> = StateMachine<'a, PIO1, 0>;
type PioCommon<'a> = Common<'a, PIO1>;
type PioPin<'a> = pio::Pin<'a, PIO1>;

const SPEED: u64 = 460_800;

struct SidesComms<'a, W: Sized + Hardware> {
    /// Protocol to communicate with the other side
    protocol: SideProtocol<W>,
    /// Status LED
    status_led: &'a mut Output<'static>,
    /// Is this the right side (master)?
    is_right: bool,
    /// Counter for received events
    ping_count: u32,
    press_count: u32,
    release_count: u32,
    /// Last stats print time
    last_stats: embassy_time::Instant,
}

struct Hw {
    // error state
    on_error: bool,
}

impl Hardware for Hw {
    async fn queue_send(&mut self, msg: u32) {
        // Queue the message to be sent by the hardware task
        HW_TX_QUEUE.send(msg).await;
    }

    async fn receive(&mut self) -> u32 {
        HW_RX_QUEUE.receive().await
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

/// Hardware task that maintains continuous 1ms communication
/// This runs independently of the protocol layer
#[embassy_executor::task]
async fn hardware_task(mut sm: SmCompound<'static>) {
    let mut ticker = Ticker::every(Duration::from_millis(1));
    let mut loop_count: u32 = 0;

    loop {
        ticker.next().await;
        loop_count += 1;

        // Print heartbeat every 5000ms
        if loop_count.is_multiple_of(5000) {
            info!("HW task heartbeat: {} iterations", loop_count);
        }

        // ALWAYS send something to maintain 1ms timing
        let msg_to_send = HW_TX_QUEUE.try_receive().unwrap_or_default();

        // Send via PIO
        sm.tx().wait_push(msg_to_send).await;

        // Check if we received anything (non-blocking)
        if sm.rx().level() > 0 {
            let received_msg = sm.rx().wait_pull().await;
            // Filter out keepalive messages (0x00000000)
            if received_msg != 0x00000000 {
                // Queue it for the protocol layer (non-blocking)
                // If queue is full, drop the message (should not happen with proper sizing)
                let _ = HW_RX_QUEUE.try_send(received_msg);
            }
        }
    }
}

impl<'a, W: Sized + Hardware> SidesComms<'a, W> {
    /// Create a new event buffer
    pub fn new(
        #[cfg(feature = "defmt")] name: &'static str,
        hw: W,
        status_led: &'a mut Output<'static>,
        is_master: bool,
    ) -> Self {
        Self {
            protocol: SideProtocol::new(
                hw,
                #[cfg(feature = "defmt")]
                name,
            ),
            status_led,
            is_right: is_master,
            ping_count: 0,
            press_count: 0,
            release_count: 0,
            last_stats: embassy_time::Instant::now(),
        }
    }

    /// Process received event and update counters
    fn process_event(&mut self, event: Event) {
        match event {
            Event::Ping => {
                self.ping_count += 1;
                if !self.is_right {
                    info!("Received Ping #{}", self.ping_count);
                }
            }
            Event::Press(_, _) => {
                self.press_count += 1;
            }
            Event::Release(_, _) => {
                self.release_count += 1;
            }
            _ => {}
        }
    }

    /// Print stats every 3 seconds
    fn maybe_print_stats(&mut self) {
        let now = embassy_time::Instant::now();
        if (now - self.last_stats).as_secs() >= 3 {
            if self.is_right {
                info!(
                    "Right side stats: Press={}, Release={}",
                    self.press_count, self.release_count
                );
            }
            self.last_stats = now;
        }
    }

    /// Run the communication between the two sides in continuous mode (1ms cycle)
    pub async fn run(&mut self) {
        info!("Starting communication loop");
        let mut ticker = Ticker::every(Duration::from_millis(1));
        let mut loop_count: u32 = 0;
        loop {
            // Wait for next 1ms tick
            ticker.next().await;
            loop_count += 1;

            // Print heartbeat every 1000ms
            if loop_count.is_multiple_of(1000) {
                info!("Heartbeat: {} iterations", loop_count);
            }

            // Queue any pending events from the channel (non-blocking)
            while let Ok(event) = SIDE_CHANNEL.try_receive() {
                self.protocol.queue_event(event).await;
            }

            // Run one protocol cycle (always sends, checks for received)
            if let Some(event) = self.protocol.run_once_continuous().await {
                self.status_led.set_low();
                self.process_event(event);
                self.status_led.set_high();
            }

            // Print stats periodically
            self.maybe_print_stats();
        }
    }
}

fn pio_freq() -> fixed::FixedU32<fixed::types::extra::U8> {
    (clocks::clk_sys_freq() as u64 / (8 * SPEED))
        .to_fixed::<U56F8>()
        .to_fixed()
}

/// Master: Transmit first, then receive
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

async fn ping_pong(
    spawner: Spawner,
    mut pio1_common: PioCommon<'static>,
    sm0: SmCompound<'static>,
    gpio_pin1: Peri<'static, PIN_1>,
    status_led: &mut Output<'static>,
    is_right: bool,
) {
    let mut pio_pin = pio1_common.make_pio_pin(gpio_pin1);
    pio_pin.set_pull(Pull::Up);

    let sm = if is_right {
        setup_master_compound(&mut pio1_common, sm0, &mut pio_pin)
    } else {
        setup_slave_compound(&mut pio1_common, sm0, &pio_pin)
    };

    // Spawn the hardware task that maintains 1ms timing
    spawner.spawn(hardware_task(sm)).unwrap();

    #[cfg(feature = "defmt")]
    let name = if is_right { "Right" } else { "Left" };
    let hw = Hw { on_error: false };
    let mut sides_comms: SidesComms<'_, Hw> = SidesComms::new(
        #[cfg(feature = "defmt")]
        name,
        hw,
        status_led,
        is_right,
    );
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
async fn main(spawner: Spawner) {
    info!("=== Protocol Example Starting ===");

    let p = embassy_rp::init(Default::default());
    let pio1 = Pio::new(p.PIO1, PioIrq1);

    let mut status_led = Output::new(p.PIN_17, Level::Low);
    let is_right = Input::new(p.PIN_29, Pull::Up).is_high();

    #[cfg(feature = "defmt")]
    {
        if is_right {
            info!("=== RIGHT SIDE (Master) ===");
        } else {
            info!("=== LEFT SIDE (Slave) ===");
        }
    }

    let pp_fut = ping_pong(
        spawner,
        pio1.common,
        pio1.sm0,
        p.PIN_1,
        &mut status_led,
        is_right,
    );
    let sender_fut = sender(is_right);

    future::join(pp_fut, sender_fut).await;
}
