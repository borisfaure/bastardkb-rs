#![allow(dead_code)]

use crate::mouse::{MouseMove, MOUSE_MOVE_CHANNEL};
use core::fmt::Debug;
use embassy_futures::select::{select, Either};
use embassy_rp::gpio::Output;
use embassy_rp::peripherals::SPI0;
use embassy_rp::spi::{Async, Error as SpiError, Instance as SpiInstance, Mode, Spi};
use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, channel::Channel};
use embassy_time::{Duration, Ticker, Timer};
use embedded_hal::spi::SpiBus;
use utils::log::{error, info};

mod firmware;

use firmware::Register;

/// Maximum number of commands in the channel
pub const NB_CMD: usize = 64;

/// Channel to send commands to the sensor
pub static SENSOR_CMD_CHANNEL: Channel<ThreadModeRawMutex, SensorCommand, NB_CMD> = Channel::new();

const DEFAULT_CPI: u16 = 800;

/// Default angle tune value, the sensor will be turned 32 degrees
const DEFAULT_ANGLE_TUNE: u8 = 32;

/// Sensor refresh rate, in ms
const REFRESH_RATE_MS: u64 = 10;

#[derive(Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum SensorCommand {
    IncreaseCpi,
    DecreaseCpi,
}

#[derive(Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct BurstData {
    pub motion: bool,
    pub dx: i16,
    pub dy: i16,
}

#[derive(Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum TrackballError {
    InvalidSignature,
    Spi(SpiError),
}
impl From<SpiError> for TrackballError {
    fn from(e: SpiError) -> Self {
        TrackballError::Spi(e)
    }
}

pub struct Trackball<'a, T: SpiInstance, M: Mode> {
    /// The SPI bus
    spi: Spi<'a, T, M>,
    /// The CS pin
    cs: Output<'a>,
    // in_burst is set if any writes or reads were performed
    in_burst: bool,
    /// Last Dx value
    last_dx: i16,
    /// Last Dy value
    last_dy: i16,
}

pub type TrackballDev = Trackball<'static, SPI0, Async>;

#[embassy_executor::task]
pub async fn run(mut ball: TrackballDev) {
    let res = ball.start().await;
    if let Err(_e) = res {
        error!("Error: {:?}", utils::log::Debug2Format(&_e));
    }
    ball.run().await;
}

impl<'a, I: SpiInstance, M: Mode> Trackball<'a, I, M> {
    /// Create a new Trackball driver
    pub fn new(spi: Spi<'a, I, M>, cs: Output<'a>) -> Self {
        Self {
            spi,
            cs,
            in_burst: false,
            last_dx: 0,
            last_dy: 0,
        }
    }

    pub async fn burst_get(&mut self) -> Result<BurstData, TrackballError> {
        // Write any value to Motion_burst register
        // if any write occured before
        if !self.in_burst {
            self.write(Register::MotionBurst, 0x00).await?;
        }

        // Lower NCS
        self.cs.set_low();
        // Send Motion_burst address
        self.spi
            .transfer_in_place(&mut [Register::MotionBurst as u8])?;

        // NOTE: The datasheet says to wait for 35us here, but it seems to work without it.
        // It seems that embassy_time is not good at waiting for such small values,
        // and simply turning off Timer reduces the processing time of this function from maxium 3 ms to almost 0.

        // tSRAD_MOTBR
        // Timer::after_micros(35).await;

        // Read the 6 bytes of burst data
        let mut buf = [0u8; 6];
        for b in buf.iter_mut() {
            let t_buf = &mut [0x00];
            match self.spi.transfer_in_place(t_buf) {
                Ok(()) => *b = *t_buf.first().unwrap(),
                Err(_) => *b = 0,
            }
        }

        // Raise NCS
        self.cs.set_high();

        // NOTE: Same as tSRAD_MOTBR. temporary disabled.
        //
        // tBEXIT
        // Timer::after_micros(1).await;

        //combine the register values
        let mut data = BurstData {
            motion: (buf[0] & 0x80) != 0,
            dy: ((buf[3] as i16) << 8) | (buf[2] as i16),
            dx: ((buf[5] as i16) << 8) | (buf[4] as i16),
        };
        if buf[0] & 0b111 != 0 {
            error!("Motion burst error");
            self.in_burst = false;
        }
        // if the motion bit is not set, the dx and dy values are not valid
        if !data.motion {
            data.dx = 0;
            data.dy = 0;
        }
        // avoid small glitches
        if data.dx == 1 || data.dx == -1 {
            data.dx = 0;
        }
        if data.dy == 1 || data.dy == -1 {
            data.dy = 0;
        }
        // if the dx or dy values are 0, the sensor is not moving
        if data.dx == 0 && data.dy == 0 {
            data.motion = false;
        }

        Ok(data)
    }

    pub async fn set_cpi(&mut self, cpi: u16) -> Result<(), TrackballError> {
        info!("Setting CPI to {}", cpi);
        let val: u8 = if cpi < 100 {
            0
        } else if cpi > 12000 {
            0x77
        } else {
            ((cpi - 100) / 100) as u8
        };
        self.write(Register::Config1, val).await
    }

    pub async fn get_cpi(&mut self) -> Result<u16, TrackballError> {
        let val = self.read(Register::Config1).await.unwrap_or_default() as u16;
        Ok((val + 1) * 100)
    }

    /// Write to a register on the sensor
    async fn write(&mut self, register: Register, data: u8) -> Result<(), TrackballError> {
        self.cs.set_low();
        // tNCS-SCLK
        Timer::after_micros(1).await;

        self.in_burst = register == Register::MotionBurst;

        // send adress of the register, with MSBit = 1 to indicate it's a write
        self.spi.transfer_in_place(&mut [register as u8 | 0x80])?;
        // send data
        self.spi.transfer_in_place(&mut [data])?;

        // tSCLK-NCS (write)
        Timer::after_micros(35).await;
        self.cs.set_high();

        // tSWW/tSWR minus tSCLK-NCS (write)
        Timer::after_micros(145).await;

        Ok(())
    }

    /// Read from a register on the sensor
    async fn read(&mut self, register: Register) -> Result<u8, TrackballError> {
        self.cs.set_low();
        // tNCS-SCLK
        Timer::after_micros(1).await;

        // send adress of the register, with MSBit = 0 to indicate it's a read
        self.spi.transfer_in_place(&mut [register as u8 & 0x7f])?;

        // tSRAD
        Timer::after_micros(160).await;

        let mut ret = 0;
        let mut buf = [0x00];
        if self.spi.transfer_in_place(&mut buf).is_ok() {
            ret = *buf.first().unwrap();
        }

        // tSCLK-NCS (read)
        Timer::after_micros(1).await;
        self.cs.set_high();

        //  tSRW/tSRR minus tSCLK-NCS
        Timer::after_micros(20).await;

        Ok(ret)
    }

    /// Check if the sensor is connected and has the correct signature
    pub async fn check_signature(&mut self) -> Result<(), TrackballError> {
        let srom = self.read(Register::SromId).await.unwrap_or(0);
        let pid = self.read(Register::ProductId).await.unwrap_or(0);
        let ipid = self.read(Register::InverseProductId).await.unwrap_or(0);

        // signature for SROM 0x04
        if srom != 0x04 || pid != 0x42 || ipid != 0xBD {
            Err(TrackballError::InvalidSignature)
        } else {
            Ok(())
        }
    }

    /// Power up the sensor
    async fn power_up(&mut self) -> Result<(), TrackballError> {
        // sensor reset not active
        // self.reset_pin.set_high().ok();

        // reset the spi bus on the sensor
        self.cs.set_high();
        Timer::after_micros(50).await;
        self.cs.set_low();
        Timer::after_micros(50).await;

        // Write to reset register
        self.write(Register::PowerUpReset, 0x5A).await?;
        // 100 ms delay
        Timer::after_micros(100).await;

        // read registers 0x02 to 0x06 (and discard the data)
        self.read(Register::Motion).await?;
        self.read(Register::DeltaXL).await?;
        self.read(Register::DeltaXH).await?;
        self.read(Register::DeltaYL).await?;
        self.read(Register::DeltaYH).await?;

        // upload the firmware
        self.upload_fw().await?;

        let is_valid_signature = self.check_signature().await;

        // Write 0x00 (rest disable) to Config2 register for wired mouse or 0x20 for
        // wireless mouse design.
        self.write(Register::Config2, 0x00).await?;
        // Tune the angle
        self.write(Register::AngleTune, DEFAULT_ANGLE_TUNE).await?;
        self.write(Register::LiftConfig, 0x02).await?;

        Timer::after_micros(100).await;

        is_valid_signature
    }

    pub async fn start(&mut self) -> Result<(), TrackballError> {
        self.power_up().await?;
        Timer::after_millis(35).await;
        self.set_cpi(DEFAULT_CPI).await?;
        Ok(())
    }

    /// Run the sensor
    pub async fn run(&mut self) {
        Timer::after_millis(250).await;
        let mut ticker = Ticker::every(Duration::from_millis(REFRESH_RATE_MS));
        loop {
            match select(ticker.next(), SENSOR_CMD_CHANNEL.receive()).await {
                Either::First(_) => {
                    let burst_res = self.burst_get().await;
                    if let Ok(burst) = burst_res {
                        if self.last_dx != burst.dx || self.last_dy != burst.dy {
                            if MOUSE_MOVE_CHANNEL.is_full() {
                                error!("Mouse move channel is full");
                            }
                            MOUSE_MOVE_CHANNEL
                                .send(MouseMove {
                                    dx: burst.dx,
                                    dy: burst.dy,
                                    pressure: 0,
                                })
                                .await;
                            self.last_dx = burst.dx;
                            self.last_dy = burst.dy;
                        }
                    } else if let Err(_e) = burst_res {
                        error!("Error: {:?}", utils::log::Debug2Format(&_e));
                    }
                }
                Either::Second(event) => match event {
                    SensorCommand::IncreaseCpi => {
                        let cpi = self.get_cpi().await.unwrap_or(DEFAULT_CPI);
                        let _ = self.set_cpi(cpi + 100).await;
                    }
                    SensorCommand::DecreaseCpi => {
                        let cpi = self.get_cpi().await.unwrap_or(DEFAULT_CPI);
                        let _ = self.set_cpi(cpi - 100).await;
                    }
                },
            }
        }
    }

    async fn upload_fw(&mut self) -> Result<(), TrackballError> {
        // Write 0 to Rest_En bit of Config2 register to disable Rest mode.
        self.write(Register::Config2, 0x00).await?;

        // write 0x1d in SROM_enable reg for initializing
        self.write(Register::SromEnable, 0x1d).await?;

        // wait for 10 ms
        Timer::after_micros(10000).await;

        // write 0x18 to SROM_enable to start SROM download
        self.write(Register::SromEnable, 0x18).await?;

        // lower CS
        self.cs.set_low();

        // first byte is address
        self.spi
            .transfer_in_place(&mut [Register::SromLoadBurst as u8 | 0x80])?;
        Timer::after_micros(15).await;

        // send the rest of the firmware
        for element in firmware::SROM_TRACKING_FW.iter() {
            self.spi.transfer_in_place(&mut [*element])?;
            Timer::after_micros(15).await;
        }

        Timer::after_micros(2).await;
        self.cs.set_high();
        Timer::after_micros(200).await;
        Ok(())
    }

    #[allow(dead_code)]
    pub async fn self_test(&mut self) -> Result<bool, TrackballError> {
        self.write(Register::SromEnable, 0x15).await?;
        Timer::after_micros(10000).await;

        let u = self.read(Register::DataOutUpper).await.unwrap_or(0); // should be 0xBE
        let l = self.read(Register::DataOutLower).await.unwrap_or(0); // should be 0xEF

        Ok(u == 0xBE && l == 0xEF)
    }
}
