#![no_std]
#![no_main]

use crate::hid::{hid_kb_writer_handler, KB_REPORT_DESCRIPTOR, MOUSE_REPORT_DESCRIPTOR};
use crate::keys::Matrix;
#[cfg(feature = "cnano")]
use crate::pmw3360::Pmw3360;
use embassy_executor::Spawner;
use embassy_rp::bind_interrupts;
use embassy_rp::gpio::{Input, Level, Output, Pull};
use embassy_rp::peripherals::{PIO0, PIO1, USB};
use embassy_rp::pio::{InterruptHandler as PioInterruptHandler, Pio};
#[cfg(feature = "cnano")]
use embassy_rp::spi::{Config as SpiConfig, Phase, Polarity, Spi};
use embassy_rp::usb::{Driver, InterruptHandler as USBInterruptHandler};
use embassy_usb::class::hid::{Config as HidConfig, HidReaderWriter, HidWriter, State};
use embassy_usb::Builder;
use futures::future;
use {defmt_rtt as _, panic_probe as _};

/// Layout events processing
mod core;
use core::Core;
/// Device
mod device;
/// USB HID configuration
mod hid;
/// Key handling
mod keys;
/// Mouse handling
mod mouse;
/// PMW3360 sensor
#[cfg(feature = "cnano")]
mod pmw3360;
/// RGB LEDs
mod rgb_leds;
/// Handling the other half of the keyboard
mod side;
/// USB handling
mod usb;

/// Basic layout for the keyboard
#[cfg(feature = "keymap_basic")]
mod keymap_basic;

/// Keymap by Boris Faure
#[cfg(feature = "keymap_borisfaure")]
mod keymap_borisfaure;

/// Test layout for the keyboard
#[cfg(feature = "keymap_test")]
mod keymap_test;

#[cfg(not(any(
    feature = "keymap_borisfaure",
    feature = "keymap_basic",
    feature = "keymap_test"
)))]
compile_error!(
    "Either feature \"keymap_basic\" or \"keymap_borisfaure\" or \"keymap_test\" must be enabled."
);

#[cfg(not(any(feature = "dilemma", feature = "cnano",)))]
compile_error!("Either feature \"cnano\" or \"dilemma\" must be enabled.");
#[cfg(all(feature = "dilemma", feature = "cnano",))]
compile_error!("Only one of \"cnano\" or \"dilemma\" can be enabled at a time.");

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => USBInterruptHandler<USB>;
});
bind_interrupts!(struct PioIrq0 {
    PIO0_IRQ_0 => PioInterruptHandler<PIO0>;
});
bind_interrupts!(struct PioIrq1 {
    PIO1_IRQ_0 => PioInterruptHandler<PIO1>;
});

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());
    defmt::info!("Hello World!");

    // Create the driver, from the HAL.
    let driver = Driver::new(p.USB, Irqs);

    // Create embassy-usb DeviceBuilder using the driver and config.
    // It needs some buffers for building the descriptors.
    let mut config_descriptor = [0; 256];
    let mut bos_descriptor = [0; 256];
    // You can also add a Microsoft OS descriptor.
    let mut msos_descriptor = [0; 256];
    let mut control_buf = [0; 256];

    let mut device_handler = device::DeviceHandler::new();

    let mut state_kb = State::new();
    let mut state_mouse = State::new();

    let usb_config = usb::config();
    let mut builder = Builder::new(
        driver,
        usb_config,
        &mut config_descriptor,
        &mut bos_descriptor,
        &mut msos_descriptor,
        &mut control_buf,
    );

    builder.handler(&mut device_handler);

    defmt::info!("Detecting side...");
    #[cfg(feature = "cnano")]
    let is_right = device::is_right(Input::new(p.PIN_15, Pull::Up));
    #[cfg(feature = "dilemma")]
    let is_right = device::is_right(Input::new(p.PIN_29, Pull::Up));

    // Create classes on the builder.
    let hidkb_config = HidConfig {
        report_descriptor: KB_REPORT_DESCRIPTOR,
        request_handler: None,
        poll_ms: 60,
        max_packet_size: 8,
    };
    let hidkb = HidReaderWriter::<_, 8, 8>::new(&mut builder, &mut state_kb, hidkb_config);

    let hidm_config = HidConfig {
        report_descriptor: MOUSE_REPORT_DESCRIPTOR,
        request_handler: None,
        poll_ms: 10,
        max_packet_size: 7,
    };
    let hid_mouse = HidWriter::<_, 7>::new(&mut builder, &mut state_mouse, hidm_config);

    let mut request_handler = hid::HidRequestHandler::new(&spawner);
    let (hid_kb_reader, hid_kb_writer) = hidkb.split();
    let hid_kb_reader_fut = async {
        hid_kb_reader.run(false, &mut request_handler).await;
    };
    let hid_kb_writer_fut = hid_kb_writer_handler(hid_kb_writer);

    // Build the builder.
    let mut usb = builder.build();

    // Run the USB device.
    let usb_fut = usb.run();

    #[cfg(feature = "cnano")]
    let rows = [
        Input::new(p.PIN_26, Pull::Up), // R2
        Input::new(p.PIN_5, Pull::Up),  // R3
        Input::new(p.PIN_4, Pull::Up),  // R4
        Input::new(p.PIN_9, Pull::Up),  // R5
    ];
    #[cfg(feature = "dilemma")]
    let rows = [
        Input::new(p.PIN_4, Pull::Up),  // R2
        Input::new(p.PIN_5, Pull::Up),  // R3
        Input::new(p.PIN_27, Pull::Up), // R4
        Input::new(p.PIN_26, Pull::Up), // R5
    ];

    #[cfg(feature = "cnano")]
    let cols = [
        Output::new(p.PIN_28, Level::High), // C2
        Output::new(p.PIN_21, Level::High), // C3
        Output::new(p.PIN_6, Level::High),  // C4
        Output::new(p.PIN_7, Level::High),  // C5
        Output::new(p.PIN_8, Level::High),  // C6
    ];
    #[cfg(feature = "dilemma")]
    let cols = [
        Output::new(p.PIN_8, Level::High),  // C2
        Output::new(p.PIN_9, Level::High),  // C3
        Output::new(p.PIN_7, Level::High),  // C4
        Output::new(p.PIN_6, Level::High),  // C5
        Output::new(p.PIN_28, Level::High), // C6
    ];

    let matrix = Matrix::new(rows, cols);
    #[cfg(feature = "cnano")]
    let mut status_led = Output::new(p.PIN_24, Level::Low);
    #[cfg(feature = "dilemma")]
    let mut status_led = Output::new(p.PIN_17, Level::Low);
    // Disable the status LED on startup
    status_led.set_high();

    let pio1 = Pio::new(p.PIO1, PioIrq1);
    side::init(
        &spawner,
        pio1.common,
        pio1.sm0,
        pio1.sm1,
        p.PIN_1,
        status_led,
        is_right,
    )
    .await;

    let pio0 = Pio::new(p.PIO0, PioIrq0);
    rgb_leds::init(
        &spawner,
        pio0.common,
        pio0.sm0,
        p.DMA_CH0,
        #[cfg(feature = "cnano")]
        p.PIN_0,
        #[cfg(feature = "dilemma")]
        p.PIN_10,
        is_right,
    );

    let mut core = Core::new(hid_mouse, is_right);
    let layout_fut = core.run();
    keys::init(&spawner, matrix, is_right);

    #[cfg(feature = "cnano")]
    if is_right {
        let sclk = p.PIN_22; // B1
        let mosi = p.PIN_23; // B2
        let miso = p.PIN_20; // B3
        let cs = Output::new(p.PIN_16, Level::High); // F0
        let tx_dma = p.DMA_CH1;
        let rx_dma = p.DMA_CH2;
        let mut spi_config = SpiConfig::default();
        spi_config.frequency = 7_000_000;
        spi_config.polarity = Polarity::IdleHigh;
        spi_config.phase = Phase::CaptureOnSecondTransition;
        let ball_spi = Spi::new(p.SPI0, sclk, mosi, miso, tx_dma, rx_dma, spi_config);
        let ball = Pmw3360::new(ball_spi, cs);

        spawner.must_spawn(pmw3360::run(ball));
    }

    defmt::info!("let's go!");
    future::join(
        future::join(usb_fut, layout_fut),
        future::join(hid_kb_reader_fut, hid_kb_writer_fut),
    )
    .await;
}
