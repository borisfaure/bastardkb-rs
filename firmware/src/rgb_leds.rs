use crate::side::SIDE_CHANNEL;
use embassy_executor::Spawner;
use embassy_futures::select::{select3, Either3};
use embassy_rp::{
    clocks,
    dma::{AnyChannel, Channel as DmaChannel},
    peripherals::PIO0,
    pio::{
        program::pio_file, Common, Config as PioConfig, FifoJoin, Instance, PioPin, ShiftConfig,
        ShiftDirection, StateMachine,
    },
    Peri,
};
use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, channel::Channel};
#[cfg(feature = "timing_logs")]
use embassy_time::Instant;
use embassy_time::{Duration, Ticker, Timer};
use fixed::types::U24F8;
use fixed_macro::fixed;
use keyberon::layout::Event as KbEvent;
use utils::rgb_anims::{RgbAnim, RgbAnimType, ERROR_COLOR_INDEX, NUM_LEDS, RGB8};
use utils::serde::Event;

use {defmt_rtt as _, panic_probe as _};

/// Number of events in the channel from keys
const NB_EVENTS: usize = 128;
/// Channel to send `keyberon::layout::event` events to the layout handler
pub static RGB_CHANNEL: Channel<ThreadModeRawMutex, KbEvent, NB_EVENTS> = Channel::new();

/// Animation commands
#[derive(Debug, defmt::Format)]
pub enum AnimCommand {
    /// Set the next animation
    Next,
    /// Change Layer
    ChangeLayer(u8),
    /// Set the animation
    Set(RgbAnimType),
    /// On error
    Error,
    /// Error has been fixed
    Fixed,
}

/// Channel to change the animation of the RGB LEDs
pub static ANIM_CHANNEL: Channel<ThreadModeRawMutex, AnimCommand, NB_EVENTS> = Channel::new();

/// WS2812 driver
pub struct Ws2812<'d, P: Instance, const S: usize, const N: usize, DMA: DmaChannel> {
    /// DMA channel to push RGB data to the PIO state machine
    dma: Peri<'d, DMA>,
    /// PIO state machine to control the WS2812 chain
    sm: StateMachine<'d, P, S>,
}

impl<'d, P: Instance, const S: usize, const N: usize, DMA: DmaChannel> Ws2812<'d, P, S, N, DMA> {
    pub fn new(
        pio: &mut Common<'d, P>,
        mut sm: StateMachine<'d, P, S>,
        dma: Peri<'d, DMA>,
        pin: Peri<'d, impl PioPin>,
    ) -> Self {
        // Setup sm0

        // prepare the PIO program
        let rgb_led_prog = pio_file!("src/rgb_led.pio");
        let mut cfg = PioConfig::default();

        // Pin config
        let out_pin = pio.make_pio_pin(pin);
        cfg.set_out_pins(&[&out_pin]);
        cfg.set_set_pins(&[&out_pin]);

        cfg.use_program(&pio.load_program(&rgb_led_prog.program), &[&out_pin]);

        // Clock config, measured in kHz to avoid overflows
        // TODO CLOCK_FREQ should come from embassy_rp
        let clock_freq = U24F8::from_num(clocks::clk_sys_freq() / 1000);
        let ws2812_freq = fixed!(800: U24F8);
        const CYCLES_PER_BIT: u32 = (2 + 5 + 3) as u32;
        let bit_freq = ws2812_freq * CYCLES_PER_BIT;
        cfg.clock_divider = clock_freq / bit_freq;

        // FIFO config
        cfg.fifo_join = FifoJoin::TxOnly;
        cfg.shift_out = ShiftConfig {
            auto_fill: true,
            threshold: 24,
            direction: ShiftDirection::Left,
        };

        sm.set_config(&cfg);
        sm.set_enable(true);

        Self { dma, sm }
    }

    pub async fn write(&mut self, colors: &[RGB8; N]) {
        // Precompute the word bytes from the colors
        let mut words = [0u32; N];
        for i in 0..N {
            let word = (u32::from(colors[i].g) << 24)
                | (u32::from(colors[i].r) << 16)
                | (u32::from(colors[i].b) << 8);
            words[i] = word;
        }

        // DMA transfer
        self.sm
            .tx()
            .dma_push(self.dma.reborrow(), &words, false)
            .await;

        Timer::after_micros(55).await;
    }
}

#[embassy_executor::task]
pub async fn run(mut ws2812: Ws2812<'static, PIO0, 0, NUM_LEDS, AnyChannel>, is_right: bool) {
    // Loop forever making RGB values and pushing them out to the WS2812.
    let mut ticker = Ticker::every(Duration::from_hz(24));

    #[cfg(feature = "timing_logs")]
    let mut timing_tick_count: usize = 0;
    #[cfg(feature = "timing_logs")]
    let mut timing_total_us: u64 = 0;
    #[cfg(feature = "timing_logs")]
    let mut timing_max_us: u64 = 0;

    let mut anim = RgbAnim::new(is_right, clocks::rosc_freq());
    loop {
        match select3(RGB_CHANNEL.receive(), ANIM_CHANNEL.receive(), ticker.next()).await {
            Either3::First(event) => {
                #[cfg(feature = "timing_logs")]
                let start = Instant::now();

                match event {
                    KbEvent::Press(i, j) => {
                        anim.on_key_event(i, j, true);
                    }
                    KbEvent::Release(i, j) => {
                        anim.on_key_event(i, j, false);
                    }
                }

                #[cfg(feature = "timing_logs")]
                {
                    let elapsed_us = start.elapsed().as_micros();
                    timing_total_us += elapsed_us;
                    timing_tick_count += 1;
                    if elapsed_us > timing_max_us {
                        timing_max_us = elapsed_us;
                    }
                }
            }
            Either3::Second(cmd) => {
                #[cfg(feature = "timing_logs")]
                let start = Instant::now();

                match cmd {
                    AnimCommand::Next => {
                        let new_anim = anim.next_animation();
                        if SIDE_CHANNEL.is_full() {
                            defmt::error!("Side channel is full");
                        }
                        SIDE_CHANNEL.send(Event::RgbAnim(new_anim)).await;
                        defmt::info!("New animation: {:?}", defmt::Debug2Format(&new_anim));
                    }
                    AnimCommand::Set(new_anim) => {
                        anim.set_animation(new_anim);
                    }
                    AnimCommand::ChangeLayer(layer) => {
                        if layer == 0 {
                            anim.restore_animation();
                        } else {
                            anim.temporarily_solid_color(layer);
                        }
                    }
                    AnimCommand::Error => {
                        anim.temporarily_solid_color(ERROR_COLOR_INDEX);
                    }
                    AnimCommand::Fixed => {
                        anim.restore_animation();
                    }
                }

                #[cfg(feature = "timing_logs")]
                {
                    let elapsed_us = start.elapsed().as_micros();
                    timing_total_us += elapsed_us;
                    timing_tick_count += 1;
                    if elapsed_us > timing_max_us {
                        timing_max_us = elapsed_us;
                    }
                }
            }
            Either3::Third(_) => {
                #[cfg(feature = "timing_logs")]
                let start = Instant::now();

                let data = anim.tick();
                ws2812.write(data).await;

                #[cfg(feature = "timing_logs")]
                {
                    let elapsed_us = start.elapsed().as_micros();
                    timing_total_us += elapsed_us;
                    timing_tick_count += 1;
                    if elapsed_us > timing_max_us {
                        timing_max_us = elapsed_us;
                    }
                }
            }
        }

        #[cfg(feature = "timing_logs")]
        {
            // Log every 120 iterations (5 seconds at 24Hz)
            if timing_tick_count >= 120 {
                defmt::info!(
                    "[TIMING] rgb_leds total={}ms max={}us (over {} iterations in 5s)",
                    timing_total_us / 1000,
                    timing_max_us,
                    timing_tick_count
                );
                timing_tick_count = 0;
                timing_total_us = 0;
                timing_max_us = 0;
            }
        }
    }
}

/// Run the LED animation control
pub fn init(
    spawner: &Spawner,
    mut common: Common<'static, PIO0>,
    sm0: StateMachine<'static, PIO0, 0>,
    dma: Peri<'static, AnyChannel>,
    pin: Peri<'static, impl PioPin>,
    is_right: bool,
) {
    let ws2812 = Ws2812::new(&mut common, sm0, dma, pin);

    spawner.must_spawn(run(ws2812, is_right));
}
