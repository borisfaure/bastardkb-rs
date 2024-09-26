#![no_std]
#![no_main]

use crate::hid::hid_kb_writer_handler;
use crate::keys::{matrix_scanner, Matrix};
use device::detect_side;
use embassy_executor::Spawner;
use embassy_rp::bind_interrupts;
use embassy_rp::gpio::{Input, Level, Output, Pull};
use embassy_rp::peripherals::USB;
use embassy_rp::usb::{Driver, InterruptHandler};
use embassy_usb::class::hid::{HidReaderWriter, State};
use embassy_usb::{Builder, Config as USBConfig};
use futures::future;
use usbd_hid::descriptor::{KeyboardReport, SerializedDescriptor};
use {defmt_rtt as _, panic_probe as _};

/// Device
mod device;
/// USB HID configuration
mod hid;
/// Key handling
mod keys;
/// Layout events processing
mod layout;
/// Handling the other half of the keyboard
mod side;

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

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => InterruptHandler<USB>;
});

/// USB VID based on
/// <https://github.com/obdev/v-usb/blob/master/usbdrv/USB-IDs-for-free.txt>
const VID: u16 = 0x16c0;

/// USB PID
const PID: u16 = 0x27db;

/// USB Product
const PRODUCT: &str = "Charybdis Nano keyboard";
/// USB Manufacturer
const MANUFACTURER: &str = "Bastard Keyboards & Boris Faure";

/// Generate the Embassy-USB configuration
pub fn usb_config() -> USBConfig<'static> {
    let mut config = USBConfig::new(VID, PID);
    config.manufacturer = Some(MANUFACTURER);
    config.product = Some(PRODUCT);
    config.serial_number = Some(env!("CARGO_PKG_VERSION"));
    config.max_power = 100;
    config.max_packet_size_0 = 64;

    // Required for windows compatibility.
    // https://developer.nordicsemi.com/nRF_Connect_SDK/doc/1.9.1/kconfig/CONFIG_CDC_ACM_IAD.html#help
    config.device_class = 0xEF;
    config.device_sub_class = 0x02;
    config.device_protocol = 0x01;
    config.composite_with_iads = true;
    config
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());
    defmt::info!("Hello World!");

    // Create the driver, from the HAL.
    let driver = Driver::new(p.USB, Irqs);

    // Create embassy-usb Config
    let usb_config = usb_config();

    // Create embassy-usb DeviceBuilder using the driver and config.
    // It needs some buffers for building the descriptors.
    let mut config_descriptor = [0; 256];
    let mut bos_descriptor = [0; 256];
    // You can also add a Microsoft OS descriptor.
    let mut msos_descriptor = [0; 256];
    let mut control_buf = [0; 256];

    let mut device_handler = device::DeviceHandler::new();

    let mut state_kb = State::new();

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
    detect_side(Input::new(p.PIN_15, Pull::Up));

    // Create classes on the builder.
    let hidkb_config = embassy_usb::class::hid::Config {
        report_descriptor: KeyboardReport::desc(),
        request_handler: None,
        poll_ms: 60,
        max_packet_size: 8,
    };
    let hidkb = HidReaderWriter::<_, 64, 64>::new(&mut builder, &mut state_kb, hidkb_config);

    // Build the builder.
    let mut usb = builder.build();

    // Run the USB device.
    let usb_fut = usb.run();

    let mut request_handler = hid::HidRequestHandler::new(&spawner);
    let (hid_kb_reader, hid_kb_writer) = hidkb.split();
    let hid_kb_reader_fut = async {
        hid_kb_reader.run(false, &mut request_handler).await;
    };
    let hid_kb_writer_fut = hid_kb_writer_handler(hid_kb_writer);

    let rows = [
        Input::new(p.PIN_26, Pull::Up), // R2
        Input::new(p.PIN_5, Pull::Up),  // R3
        Input::new(p.PIN_4, Pull::Up),  // R4
        Input::new(p.PIN_9, Pull::Up),  // R5
    ];
    let cols = [
        Output::new(p.PIN_28, Level::High), // C2
        Output::new(p.PIN_21, Level::High), // C3
        Output::new(p.PIN_6, Level::High),  // C4
        Output::new(p.PIN_7, Level::High),  // C5
        Output::new(p.PIN_8, Level::High),  // C6
    ];
    let matrix = Matrix::new(rows, cols);

    let layout_fut = layout::layout_handler();
    let matrix_fut = matrix_scanner(matrix);
    defmt::info!("let's go!");

    future::join(
        future::join3(usb_fut, matrix_fut, layout_fut),
        future::join(hid_kb_reader_fut, hid_kb_writer_fut),
    )
    .await;
}
