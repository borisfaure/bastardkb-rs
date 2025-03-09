use defmt::{error, info};
use embassy_executor::Spawner;
use embassy_rp::{
    dma::AnyChannel,
    gpio::{self, Output},
    peripherals::{PIN_20, PIN_21, PIN_22, PIN_23, SPI0},
    spi::{self, Async, Spi},
};
use embassy_time::{Duration, Ticker};
use embedded_hal_bus::spi::ExclusiveDevice;

pub mod driver;
mod glide;
pub mod regs;

type TrackpadSpi = ExclusiveDevice<Spi<'static, SPI0, Async>, Output<'static>, embassy_time::Delay>;

pub fn init(
    spawner: &Spawner,
    spi: SPI0,
    clk: PIN_22,
    mosi: PIN_23,
    miso: PIN_20,
    cs: PIN_21,
    tx_dma: AnyChannel,
    rx_dma: AnyChannel,
) {
    let mut config = spi::Config::default();
    config.phase = spi::Phase::CaptureOnSecondTransition;
    let spi = Spi::new(spi, clk, mosi, miso, tx_dma, rx_dma, config);
    let spi =
        ExclusiveDevice::new(spi, Output::new(cs, gpio::Level::Low), embassy_time::Delay).unwrap();

    spawner.must_spawn(trackpad_task(spi));
}

#[embassy_executor::task]
async fn trackpad_task(spi: TrackpadSpi) {
    let mut trackpad = driver::Trackpad::<_, 35>::new(
        spi,
        driver::PositionMode::Absolute,
        driver::Overlay::Curved,
        driver::TransformMode::Rotate90,
        None,
    );

    if let Err(_e) = trackpad.init().await {
        error!("Couldn't init trackpad");
        return;
    }

    let mut ticker = Ticker::every(Duration::from_hz(250));

    loop {
        match trackpad.get_report().await {
            Ok(Some(report)) => {
                info!("trackpad report: {:?}", report);
            }
            Err(_e) => {
                error!("Failed to get a trackpad report");
            }
            _ => (),
        }

        ticker.next().await;
    }
}
