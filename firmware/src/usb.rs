use embassy_rp::peripherals::USB;
use embassy_rp::usb::Driver;
use embassy_usb::Builder;
use embassy_usb::Config as USBConfig;

/// USB VID based on
/// <https://github.com/obdev/v-usb/blob/master/usbdrv/USB-IDs-for-free.txt>
const VID: u16 = 0x16c0;

/// USB PID
const PID: u16 = 0x27db;

/// USB Product
#[cfg(feature = "cnano")]
const PRODUCT: &str = "Charybdis Nano keyboard";
#[cfg(feature = "dilemma")]
const PRODUCT: &str = "Dilemma keyboard";
/// USB Manufacturer
const MANUFACTURER: &str = "Bastard Keyboards & Boris Faure";

/// Generate the Embassy-USB configuration
pub fn config() -> USBConfig<'static> {
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

#[embassy_executor::task]
pub async fn run(builder: Builder<'static, Driver<'static, USB>>) {
    builder.build().run().await;
}
