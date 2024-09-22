use core::sync::atomic::{AtomicBool, Ordering};
use defmt::*;
use embassy_usb::Handler;

/// Device configured flag
static CONFIGURED: AtomicBool = AtomicBool::new(false);

/// Whether the device is the host or not
pub fn is_host() -> bool {
    CONFIGURED.load(Ordering::Relaxed)
}

/// Device Handler, used to know when it's configured
pub struct DeviceHandler {}

impl DeviceHandler {
    /// Create a new Device Handler
    pub fn new() -> Self {
        DeviceHandler {}
    }
}

impl Handler for DeviceHandler {
    fn enabled(&mut self, enabled: bool) {
        CONFIGURED.store(false, Ordering::Relaxed);
        if enabled {
            info!("Device enabled");
        } else {
            info!("Device disabled");
        }
    }

    fn reset(&mut self) {
        CONFIGURED.store(false, Ordering::Relaxed);
        info!("Bus reset, the Vbus current limit is 100mA");
    }

    fn addressed(&mut self, addr: u8) {
        CONFIGURED.store(false, Ordering::Relaxed);
        info!("USB address set to: {}", addr);
    }

    fn configured(&mut self, configured: bool) {
        CONFIGURED.store(configured, Ordering::Relaxed);
        if configured {
            info!(
                "Device configured, it may now draw up to the configured current limit from Vbus."
            )
        } else {
            info!("Device is no longer configured, the Vbus current limit is 100mA.");
        }
    }
}
