use crate::mouse::{MouseMove, MOUSE_MOVE_CHANNEL};
use defmt::error;
use embassy_executor::Spawner;
use embassy_rp::{
    dma::AnyChannel,
    gpio::{self, Output},
    peripherals::{PIN_20, PIN_21, PIN_22, PIN_23, SPI0},
    spi::{self, Async, Spi},
    Peri,
};
use embassy_time::{Duration, Ticker};
use embedded_hal_bus::spi::ExclusiveDevice;

pub mod driver;
mod glide;
pub mod regs;

/// Sensor refresh rate, in ms
const REFRESH_RATE_MS: u64 = 10;

type TrackpadSpi = ExclusiveDevice<Spi<'static, SPI0, Async>, Output<'static>, embassy_time::Delay>;

pub struct TrackpadPins {
    pub clk: Peri<'static, PIN_22>,
    pub mosi: Peri<'static, PIN_23>,
    pub miso: Peri<'static, PIN_20>,
    pub cs: Peri<'static, PIN_21>,
}

pub fn init(
    spawner: &Spawner,
    spi: Peri<'static, SPI0>,
    pins: TrackpadPins,
    tx_dma: Peri<'static, AnyChannel>,
    rx_dma: Peri<'static, AnyChannel>,
) {
    let mut config = spi::Config::default();
    config.phase = spi::Phase::CaptureOnSecondTransition;
    let spi = Spi::new(spi, pins.clk, pins.mosi, pins.miso, tx_dma, rx_dma, config);
    let spi = ExclusiveDevice::new(
        spi,
        Output::new(pins.cs, gpio::Level::Low),
        embassy_time::Delay,
    )
    .unwrap();

    spawner.must_spawn(trackpad_task(spi));
}

#[embassy_executor::task]
async fn trackpad_task(spi: TrackpadSpi) {
    let mut trackpad = driver::Trackpad::<_, 35>::new(spi, None);

    if let Err(_e) = trackpad.init().await {
        error!("Couldn't init trackpad");
        return;
    }

    let mut ticker = Ticker::every(Duration::from_millis(REFRESH_RATE_MS));

    let mut last_dx = 0_i8;
    let mut last_dy = 0_i8;
    loop {
        match trackpad.get_report().await {
            Ok(Some((dx, dy))) => {
                if last_dx != dx || last_dy != dy {
                    if MOUSE_MOVE_CHANNEL.is_full() {
                        defmt::error!("Mouse move channel is full");
                    }
                    last_dx = dx;
                    last_dy = dy;
                    MOUSE_MOVE_CHANNEL
                        .send(MouseMove {
                            dx: dx.into(),
                            dy: dy.into(),
                        })
                        .await;
                }
            }
            Err(_e) => {
                error!("Failed to get a trackpad report");
            }
            _ => (),
        }

        ticker.next().await;
    }
}
