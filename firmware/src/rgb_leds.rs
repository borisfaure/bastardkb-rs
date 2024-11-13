use crate::side::SIDE_CHANNEL;
use embassy_futures::select::{select3, Either3};
use embassy_rp::dma::{AnyChannel, Channel as DmaChannel};
use embassy_rp::peripherals::PIO0;
use embassy_rp::pio::{
    Common, Config as PioConfig, FifoJoin, Instance, PioPin, ShiftConfig, ShiftDirection,
    StateMachine,
};
use embassy_rp::{clocks, into_ref, Peripheral, PeripheralRef};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};
use embassy_time::{Duration, Ticker, Timer};
use fixed::types::U24F8;
use fixed_macro::fixed;
use keyberon::layout::Event as KbEvent;
use utils::rgb_anims::{RgbAnim, RgbAnimType, ERROR_COLOR_INDEX, RGB8};
use utils::serde::Event;

use {defmt_rtt as _, panic_probe as _};

/// Number of events in the channel from keys
const NB_EVENTS: usize = 64;
/// Channel to send `keyberon::layout::event` events to the layout handler
pub static RGB_CHANNEL: Channel<CriticalSectionRawMutex, KbEvent, NB_EVENTS> = Channel::new();

/// Animation commands
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
pub static ANIM_CHANNEL: Channel<CriticalSectionRawMutex, AnimCommand, NB_EVENTS> = Channel::new();

/// WS2812 driver
pub struct Ws2812<'d, P: Instance, const S: usize, const N: usize> {
    /// DMA channel to push RGB data to the PIO state machine
    dma: PeripheralRef<'d, AnyChannel>,
    /// PIO state machine to control the WS2812 chain
    sm: StateMachine<'d, P, S>,
}

impl<'d, P: Instance, const S: usize, const N: usize> Ws2812<'d, P, S, N> {
    pub fn new(
        pio: &mut Common<'d, P>,
        mut sm: StateMachine<'d, P, S>,
        dma: impl Peripheral<P = impl DmaChannel> + 'd,
        pin: impl PioPin,
    ) -> Self {
        into_ref!(dma);

        // Setup sm0

        // prepare the PIO program
        let rgb_led_prog = pio_proc::pio_file!("src/rgb_led.pio");
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

        Self {
            dma: dma.map_into(),
            sm,
        }
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
        self.sm.tx().dma_push(self.dma.reborrow(), &words).await;

        Timer::after_micros(55).await;
    }
}

/// Run the LED animation control
pub async fn run(
    mut common: Common<'_, PIO0>,
    sm0: StateMachine<'_, PIO0, 0>,
    dma: impl DmaChannel,
    pin: impl PioPin,
    is_right: bool,
) {
    let mut ws2812 = Ws2812::new(&mut common, sm0, dma, pin);

    // Loop forever making RGB values and pushing them out to the WS2812.
    let mut ticker = Ticker::every(Duration::from_hz(30));

    let mut anim = RgbAnim::new(is_right, clocks::rosc_freq());
    loop {
        match select3(RGB_CHANNEL.receive(), ANIM_CHANNEL.receive(), ticker.next()).await {
            Either3::First(event) => match event {
                KbEvent::Press(i, j) => {
                    anim.on_key_event(i, j, true);
                }
                KbEvent::Release(i, j) => {
                    anim.on_key_event(i, j, false);
                }
            },
            Either3::Second(cmd) => match cmd {
                AnimCommand::Next => {
                    let new_anim = anim.next_animation();
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
            },
            Either3::Third(_) => {
                let data = anim.tick();
                ws2812.write(data).await;
            }
        }
    }
}
