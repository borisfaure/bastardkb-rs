use crate::core::LAYOUT_CHANNEL;
use crate::rgb_leds::{AnimCommand, ANIM_CHANNEL};
use embassy_executor::Spawner;
#[cfg(feature = "timing_logs")]
use embassy_futures::select::select3;
use embassy_futures::{
    select::{select, Either, Either3},
    yield_now,
};
use embassy_rp::{
    clocks,
    gpio::{Drive, Level, Output, Pull},
    peripherals::{PIN_1, PIO1},
    pio::{self, program::pio_asm, Direction, FifoJoin, ShiftDirection, StateMachine},
    Peri,
};
use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, channel::Channel};
#[cfg(feature = "timing_logs")]
use embassy_time::Instant;
use embassy_time::{Duration, Ticker};
use fixed::{traits::ToFixed, types::U56F8};
use keyberon::layout::Event as KBEvent;
use utils::protocol::{Hardware, ReceivedOrTick, SideProtocol};
use utils::serde::Event;

/// Speed of the PIO state machine, in bps
const SPEED: u64 = 460_800;

/// Number of events in the channel to the other half of the keyboard
const NB_EVENTS: usize = 16;
/// Channel to send `utils::serde::event` events to the layout handler
pub static SIDE_CHANNEL: Channel<ThreadModeRawMutex, Event, NB_EVENTS> = Channel::new();

/// Index of the TX state machine
const TX: usize = 0;
/// Index of the RX state machine
const RX: usize = 1;

/// Send PING every x seconds
const PING_PERIOD_SEC: u64 = 1;

pub type SmRx<'a> = StateMachine<'a, PIO1, { RX }>;
pub type SmTx<'a> = StateMachine<'a, PIO1, { TX }>;
pub type PioCommon<'a> = pio::Common<'a, PIO1>;
pub type PioPin<'a> = pio::Pin<'a, PIO1>;

struct SidesComms<W: Sized + Hardware> {
    /// Protocol to communicate with the other side
    protocol: SideProtocol<W>,
    /// Status LED
    status_led: Output<'static>,
    /// Timing logs: tick counter
    #[cfg(feature = "timing_logs")]
    timing_tick_count: usize,
    /// Timing logs: total time in microseconds
    #[cfg(feature = "timing_logs")]
    timing_total_us: u64,
    /// Timing logs: max time in microseconds
    #[cfg(feature = "timing_logs")]
    timing_max_us: u64,
    /// Timing logs: ticker for periodic reporting
    #[cfg(feature = "timing_logs")]
    timing_ticker: Ticker,
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
            ticker: Ticker::every(Duration::from_secs(PING_PERIOD_SEC)),
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
        let bit_time_cycles = (sys_freq * 100) / SPEED; // Increased from 50 to 100 bits worth of time (~217us)
        let delay_us = (bit_time_cycles * 1_000_000) / sys_freq;
        embassy_time::Timer::after_micros(delay_us).await;

        // Disable TX state machine before manipulating TX pin
        self.tx_sm.set_enable(false);

        // Set minimal drive strength on TX pin to avoid interfering with RX
        self.pin.set_drive_strength(Drive::_2mA);

        // Set pin as input (pull-up configured at line 337 handles idle-high)
        self.rx_sm.set_pin_dirs(Direction::In, &[&self.pin]);
        self.tx_sm.set_pin_dirs(Direction::In, &[&self.pin]);

        // Small delay to let pin settle after direction change
        yield_now().await;
        //let settle_delay_us = (100 * 1_000_000) / sys_freq;
        //embassy_time::Timer::after_micros(settle_delay_us).await;

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
    pub fn new(name: &'static str, hw: W, status_led: Output<'static>, is_right: bool) -> Self {
        Self {
            protocol: SideProtocol::new(hw, name, is_right),
            status_led,
            #[cfg(feature = "timing_logs")]
            timing_tick_count: 0,
            #[cfg(feature = "timing_logs")]
            timing_total_us: 0,
            #[cfg(feature = "timing_logs")]
            timing_max_us: 0,
            #[cfg(feature = "timing_logs")]
            timing_ticker: Ticker::every(Duration::from_secs(5)),
        }
    }

    /// Run the communication between the two sides
    pub async fn run(&mut self) {
        // Wait for the other side to boot
        loop {
            #[cfg(feature = "timing_logs")]
            let result = select3(
                SIDE_CHANNEL.receive(),
                self.protocol.receive(),
                self.timing_ticker.next(),
            )
            .await;

            #[cfg(not(feature = "timing_logs"))]
            let result: Either3<Event, Event, ()> =
                match select(SIDE_CHANNEL.receive(), self.protocol.receive()).await {
                    Either::First(event) => Either3::First(event),
                    Either::Second(x) => Either3::Second(x),
                };

            match result {
                Either3::First(event) => {
                    #[cfg(feature = "timing_logs")]
                    let start = Instant::now();

                    self.protocol.queue_event(event).await;

                    #[cfg(feature = "timing_logs")]
                    {
                        let elapsed_us = start.elapsed().as_micros();
                        self.timing_total_us += elapsed_us;
                        self.timing_tick_count += 1;
                        if elapsed_us > self.timing_max_us {
                            self.timing_max_us = elapsed_us;
                        }
                    }
                }
                Either3::Second(x) => {
                    #[cfg(feature = "timing_logs")]
                    let start = Instant::now();

                    #[cfg(feature = "cnano")]
                    self.status_led.set_low();
                    #[cfg(feature = "dilemma")]
                    self.status_led.set_high();
                    process_event(x).await;
                    #[cfg(feature = "cnano")]
                    self.status_led.set_high();
                    #[cfg(feature = "dilemma")]
                    self.status_led.set_low();

                    #[cfg(feature = "timing_logs")]
                    {
                        let elapsed_us = start.elapsed().as_micros();
                        self.timing_total_us += elapsed_us;
                        self.timing_tick_count += 1;
                        if elapsed_us > self.timing_max_us {
                            self.timing_max_us = elapsed_us;
                        }
                    }
                }
                #[cfg(feature = "timing_logs")]
                Either3::Third(_) => {
                    defmt::info!(
                        "[TIMING] side::run total={}ms max={}us (over {} events in 5s)",
                        self.timing_total_us / 1000,
                        self.timing_max_us,
                        self.timing_tick_count
                    );
                    self.timing_tick_count = 0;
                    self.timing_total_us = 0;
                    self.timing_max_us = 0;
                }
                #[cfg(not(feature = "timing_logs"))]
                Either3::Third(_) => unreachable!(),
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
        // After transmission, delay to give receiver time to process and respond
        // This is critical when CPU is busy with USB/RGB/scanning tasks
        "set   y, 7  side 1",    // Set line high (idle) and outer loop counter
        "outer_loop:",
        "set   x, 31",           // Inner loop counter (32 iterations)
        "inner_loop:",
        "nop [7]",               // Wait 8 PIO cycles (side-set limits delay to 7)
        "nop [7]",               // Another 8 cycles
        "nop [7]",               // Another 8 cycles
        "nop [7]",               // Another 8 cycles (total 33 per inner loop)
        "jmp   x--, inner_loop", // Loop 32 times per outer iteration
        "jmp   y--, outer_loop", // Outer loop 8 times: 8*32*33 = 8448 cycles ≈ 250 bit-times ≈ 540us
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

#[embassy_executor::task]
async fn run(mut sides_comms: SidesComms<Hw<'static>>) {
    sides_comms.run().await;
}

pub async fn init(
    spawner: &Spawner,
    mut pio_common: PioCommon<'static>,
    sm0: SmTx<'static>,
    sm1: SmRx<'static>,
    gpio_pin1: Peri<'static, PIN_1>,
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
    let comms = SidesComms::new(name, hw, status_led, is_right);
    spawner.must_spawn(run(comms));
}
