//! Test compound PIO programs that do TX+RX in single state machine
//! This eliminates mode-switching overhead for 1kHz master/slave operation

#![no_std]
#![no_main]

use embassy_executor::Spawner;
use embassy_rp::{
    bind_interrupts,
    gpio::{Level, Output, Pull},
    peripherals::PIO1,
    pio::{
        self, program::pio_asm, Common, Direction, InterruptHandler as PioInterruptHandler, Pio,
        ShiftDirection, StateMachine,
    },
};
#[cfg(feature = "defmt")]
use embassy_time::Instant;
use embassy_time::{Duration, Ticker};
use fixed::{traits::ToFixed, types::U56F8};
#[cfg(feature = "defmt")]
use utils::log::error;
use utils::log::{info, warn};

#[cfg(not(feature = "defmt"))]
use panic_halt as _;
#[cfg(feature = "defmt")]
use {defmt_rtt as _, panic_probe as _};

bind_interrupts!(struct PioIrq1 {
    PIO1_IRQ_0 => PioInterruptHandler<PIO1>;
});

const SPEED: u64 = 460_800;

type SmCompound<'a> = StateMachine<'a, PIO1, 0>;
type PioCommon<'a> = Common<'a, PIO1>;
type PioPin<'a> = pio::Pin<'a, PIO1>;

fn pio_freq() -> fixed::FixedU32<fixed::types::extra::U8> {
    (embassy_rp::clocks::clk_sys_freq() as u64 / (8 * SPEED))
        .to_fixed::<U56F8>()
        .to_fixed()
}

/// Master: Transmit first, then receive
fn setup_master_compound<'a>(
    common: &mut PioCommon<'a>,
    mut sm: SmCompound<'a>,
    pin: &mut PioPin<'a>,
) -> SmCompound<'a> {
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
fn setup_slave_compound<'a>(
    common: &mut PioCommon<'a>,
    mut sm: SmCompound<'a>,
    pin: &PioPin<'a>,
) -> SmCompound<'a> {
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

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    info!("Compound PIO Test");

    let p = embassy_rp::init(Default::default());
    let mut pio1 = Pio::new(p.PIO1, PioIrq1);

    let is_right = embassy_rp::gpio::Input::new(p.PIN_29, Pull::Up).is_high();
    let mut status_led = Output::new(p.PIN_17, Level::Low);

    let mut pio_pin = pio1.common.make_pio_pin(p.PIN_1);
    pio_pin.set_pull(Pull::Up);

    const TEST_VALUES: [u32; 11] = [
        0x00000000u32,
        0x11111111,
        0x22222222,
        0x33333333,
        0xaaaaaaaa,
        0x55555555,
        0xffffffff,
        0x12345678,
        0x87654321,
        0xdeadbeef,
        0x0fedcba9,
    ];

    if is_right {
        // MASTER: Right side
        info!("MASTER: Starting 1kHz compound PIO test with ping-pong counter");
        let mut sm = setup_master_compound(&mut pio1.common, pio1.sm0, &mut pio_pin);

        let mut ticker = Ticker::every(Duration::from_millis(1));
        let mut index: usize = 0;
        #[cfg(feature = "defmt")]
        let mut iterations: u32 = 0;
        #[cfg(feature = "defmt")]
        let mut errors: u32 = 0;
        #[cfg(feature = "defmt")]
        let mut total_rtt_us: u64 = 0;
        #[cfg(feature = "defmt")]
        let mut max_rtt_us: u64 = 0;
        #[cfg(feature = "defmt")]
        let mut min_rtt_us: u64 = u64::MAX;
        #[allow(unused_assignments)]
        #[cfg(feature = "defmt")]
        let mut rtt_us: u64 = 0;

        loop {
            ticker.next().await;

            let send_data = TEST_VALUES[index];
            #[cfg(feature = "defmt")]
            let start = Instant::now();

            // Send current test value
            sm.tx().wait_push(send_data).await;
            let received = sm.rx().wait_pull().await;

            #[cfg(feature = "defmt")]
            {
                rtt_us = start.elapsed().as_micros();
                total_rtt_us += rtt_us;
                if rtt_us > max_rtt_us {
                    max_rtt_us = rtt_us;
                }
                if rtt_us < min_rtt_us {
                    min_rtt_us = rtt_us;
                }
            }

            // Toggle LED
            status_led.toggle();

            // Verify slave sent next value in sequence
            let expected_index = (index + 1) % TEST_VALUES.len();
            let expected = TEST_VALUES[expected_index];
            if received != expected {
                #[cfg(feature = "defmt")]
                {
                    errors += 1;
                    error!(
                    "[ERROR #{}] RTT={}µs index={} Sent: 0x{:08x}, Expected: 0x{:08x}, Received: 0x{:08x}",
                    errors,
                    rtt_us,
                    index,
                    send_data,
                    expected,
                    received
                );
                }
            }

            // Move to next value (skip one since slave will use index+1)
            index = (index + 2) % TEST_VALUES.len();
            #[cfg(feature = "defmt")]
            {
                iterations += 1;
                // Report every 5000 exchanges
                if iterations.is_multiple_of(5000) {
                    let avg_rtt_us = total_rtt_us / (iterations as u64);
                    info!(
                        "=== #{}: index={}, errors={}, RTT: avg={}µs min={}µs max={}µs ===",
                        iterations, index, errors, avg_rtt_us, min_rtt_us, max_rtt_us
                    );
                }
            }
        }
    } else {
        // SLAVE: Left side
        info!("SLAVE: Waiting for master, will reply with next test value");
        let mut sm = setup_slave_compound(&mut pio1.common, pio1.sm0, &pio_pin);

        let mut expected_index: Option<usize> = None;

        loop {
            // Check RX FIFO level - warn if it's getting full
            let rx_level = sm.rx().level();
            if rx_level >= 3 {
                warn!("SLAVE: RX FIFO filling up! Level: {}/4", rx_level);
            }

            // Receive value from master
            let received = sm.rx().wait_pull().await;
            status_led.toggle();

            // Find the index in test values
            let mut found_index = None;
            for (i, &val) in TEST_VALUES.iter().enumerate() {
                if val == received {
                    found_index = Some(i);
                    break;
                }
            }

            // Verify sequence (unless this is first transmission)
            if let Some(expected) = expected_index {
                match found_index {
                    Some(idx) if idx == expected => {
                        // Correct value received
                    }
                    Some(idx) => {
                        panic!(
                            "SLAVE: Expected index {} (0x{:08x}), got index {} (0x{:08x})",
                            expected, TEST_VALUES[expected], idx, received
                        );
                    }
                    None => {
                        panic!(
                            "SLAVE: Received invalid value 0x{:08x} (not in test array)",
                            received
                        );
                    }
                }
            }

            let reply = match found_index {
                Some(i) => {
                    let next_index = (i + 1) % TEST_VALUES.len();
                    expected_index = Some((next_index + 1) % TEST_VALUES.len());
                    TEST_VALUES[next_index]
                }
                None => {
                    // Should never reach here due to panic above
                    received
                }
            };

            sm.tx().wait_push(reply).await;
        }
    }
}
