//! Compule LED Data to render RGB Animations

use crate::log::*;
use crate::prng::XorShift32;
use crate::serde::Error as SerdeError;

/// Number of LEDs on each side
#[cfg(feature = "cnano")]
pub const NUM_LEDS: usize = 18;
#[cfg(feature = "dilemma")]
pub const NUM_LEDS: usize = 36;
/// Keyboard matrix rows
pub const ROWS: usize = 4;
/// Keyboard matrix columns
pub const COLS: usize = 5;

/// RGB Animation Type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum RgbAnimType {
    /// No animation, leds are off
    Off,
    SolidColor(u8), // Color index
    /// Wheel
    Wheel,
    /// Pulse animation with random colors on each pulse
    Pulse,
    /// Pulse animation with solid color
    PulseSolid(u8),
    /// Highlight pressed keys with a random color
    Input,
    /// Highlight pressed keys with solid color
    InputSolid(u8), // Color index
}

impl RgbAnimType {
    /// Serialize the RGB Animation Type to a u8
    pub fn to_u8(&self) -> Result<u8, SerdeError> {
        match self {
            RgbAnimType::Off => Ok(0),
            RgbAnimType::SolidColor(s) if *s < 32 => Ok((1 << 5) | s),
            RgbAnimType::Wheel => Ok(2 << 5),
            RgbAnimType::Pulse => Ok(3 << 5),
            RgbAnimType::PulseSolid(s) if *s < 32 => Ok((4 << 5) | s),
            RgbAnimType::Input => Ok(5 << 5),
            RgbAnimType::InputSolid(s) if *s < 32 => Ok((6 << 5) | s),
            _ => Err(SerdeError::Serialization),
        }
    }

    /// Deserialize the RGB Animation Type from a u8
    pub fn from_u8(value: u8) -> Result<Self, SerdeError> {
        match value >> 5 {
            0 => Ok(RgbAnimType::Off),
            1 => Ok(RgbAnimType::SolidColor(value & 0x1f)),
            2 => Ok(RgbAnimType::Wheel),
            3 => Ok(RgbAnimType::Pulse),
            4 => Ok(RgbAnimType::PulseSolid(value & 0x1f)),
            5 => Ok(RgbAnimType::Input),
            6 => Ok(RgbAnimType::InputSolid(value & 0x1f)),
            _ => Err(SerdeError::Deserialization),
        }
    }
}

/// RGB Color
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct RGB8 {
    /// Red
    pub r: u8,
    /// Green
    pub g: u8,
    /// Blue
    pub b: u8,
}

impl RGB8 {
    /// Create a new RGB8 color
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        RGB8 { r, g, b }
    }

    /// Default color: black
    pub const fn default() -> Self {
        RGB8::new(0, 0, 0)
    }

    /// Create a new RGB8 color from an indexed color
    pub fn indexed(i: u8) -> Self {
        INDEXED_COLORS[i as usize]
    }
}

/// Indexed colors
const INDEXED_COLORS: [RGB8; 11] = [
    // No color
    RGB8::default(),
    // 1/ orange, RAISE
    RGB8::new(0x40, 0x10, 0x00),
    // 2/ green, LOWER
    RGB8::new(0x00, 0x40, 0x00),
    // 3/ purple, MISC
    RGB8::new(0x40, 0, 0x10),
    // 4/ blue, NUMBERS
    RGB8::new(0x00, 0x00, 0x40),
    // 5/ red, TMUX
    RGB8::new(0x07, 0x00, 0x00),
    // 6/ gray, GAMING
    RGB8::new(0x07, 0x07, 0x07),
    // 7/ beige, CAPS
    RGB8::new(0x0f, 0x0f, 0x00),
    // 8/ yellow, QWERTY
    RGB8::new(0x40, 0x30, 0x00),
    // 9/ dark red, MOUSE
    RGB8::new(0x40, 0, 0),
    // 10/ white, ERROR
    RGB8::new(0xff, 0xff, 0xff),
];
/// Default color: red
const DEFAULT_COLOR_INDEX: u8 = 9;
/// Error color: orange
pub const ERROR_COLOR_INDEX: u8 = 10;

impl From<u32> for RGB8 {
    fn from(i: u32) -> Self {
        let r = ((i >> 24) & 0xff) as u8;
        let g = ((i >> 16) & 0xff) as u8;
        let b = ((i >> 8) & 0xff) as u8;
        RGB8 { r, g, b }
    }
}

pub struct RgbAnim {
    /// The current animation frame
    frame: u8,
    /// The current animation
    animation: RgbAnimType,
    /// Saved animation
    saved_animation: Option<RgbAnimType>,

    /// The LED data
    led_data: [RGB8; NUM_LEDS],

    /// Whether the animation is on the right side
    is_right: bool,

    /// current color
    color: RGB8,

    /// PRNG
    prng: XorShift32,
}

/// Input a value 0 to 255 to get a color value
/// The colours are a transition r - g - b - back to r.
fn wheel(mut wheel_pos: u8) -> RGB8 {
    wheel_pos = 255 - wheel_pos;
    if wheel_pos < 85 {
        return RGB8::new(255 - wheel_pos * 3, 0, wheel_pos * 3);
    }
    if wheel_pos < 170 {
        wheel_pos -= 85;
        return RGB8::new(0, wheel_pos * 3, 255 - wheel_pos * 3);
    }
    wheel_pos -= 170;
    RGB8::new(wheel_pos * 3, 255 - wheel_pos * 3, 0)
}

/// Index of leds on the right side
const MATRIX_LED_RIGHT: [[usize; COLS]; ROWS] = [
    [2, 3, 8, 9, 12],
    [1, 4, 7, 10, 13],
    [0, 5, 6, 11, 14],
    [255, 255, 255, 15, 16],
];
/// Index of leds on the left side
const MATRIX_LED_LEFT: [[usize; COLS]; ROWS] = [
    [2, 3, 8, 9, 12],
    [1, 4, 7, 10, 13],
    [0, 5, 6, 11, 14],
    [15, 16, 17, 255, 255],
];

///>>> from math import sin, pi; [int(sin(x/128.0*pi)**4*255) for x in range(128)]
const PULSE_TABLE: [u16; 128] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 2, 3, 4, 5, 6, 8, 10, 12, 15, 17, 20, 24, 28, 32, 36,
    41, 46, 51, 57, 63, 70, 76, 83, 91, 98, 106, 113, 121, 129, 138, 146, 154, 162, 170, 178, 185,
    193, 200, 207, 213, 220, 225, 231, 235, 240, 244, 247, 250, 252, 253, 254, 255, 254, 253, 252,
    250, 247, 244, 240, 235, 231, 225, 220, 213, 207, 200, 193, 185, 178, 170, 162, 154, 146, 138,
    129, 121, 113, 106, 98, 91, 83, 76, 70, 63, 57, 51, 46, 41, 36, 32, 28, 24, 20, 17, 15, 12, 10,
    8, 6, 5, 4, 3, 2, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

impl RgbAnim {
    /// Create a new RGB Animation
    pub fn new(is_right: bool, seed: u32) -> Self {
        RgbAnim {
            frame: 0,
            animation: RgbAnimType::SolidColor(0),
            saved_animation: None,
            led_data: [RGB8::default(); NUM_LEDS],
            is_right,
            color: RGB8::indexed(DEFAULT_COLOR_INDEX),
            prng: XorShift32::new(seed),
        }
    }

    /// Get the LED index for a key
    fn get_led_index(&self, i: u8, j: u8) -> usize {
        if self.is_right {
            MATRIX_LED_RIGHT[i as usize][(9 - j) as usize]
        } else {
            MATRIX_LED_LEFT[i as usize][j as usize]
        }
    }

    /// Set color of all LEDs
    fn fill_color(&mut self, color: RGB8) {
        for led in self.led_data.iter_mut() {
            *led = color;
        }
    }

    /// Tick the wheel animation
    fn tick_wheel(&mut self) {
        for (i, led) in self.led_data.iter_mut().enumerate().take(NUM_LEDS) {
            *led = wheel((((i * 256) as u16 / NUM_LEDS as u16 + self.frame as u16) & 255) as u8);
        }
    }

    /// Tick the pulse Animation
    fn tick_pulse(&mut self) {
        let pulse_index = (self.frame as usize) & 127;
        let pulse = PULSE_TABLE[pulse_index];
        let color = RGB8 {
            r: (u16::from(self.color.r) * pulse / 255) as u8,
            g: (u16::from(self.color.g) * pulse / 255) as u8,
            b: (u16::from(self.color.b) * pulse / 255) as u8,
        };
        self.fill_color(color);
    }

    /// Set a random color as main color
    fn new_random_color(&mut self) -> RGB8 {
        RGB8::from(self.prng.random())
    }

    /// Tick the animation
    pub fn tick(&mut self) -> &[RGB8; NUM_LEDS] {
        match self.animation {
            RgbAnimType::Off => self.fill_color(RGB8::default()),
            RgbAnimType::SolidColor(idx) => self.fill_color(RGB8::indexed(idx)),
            RgbAnimType::Wheel => self.tick_wheel(),
            RgbAnimType::Pulse => {
                if self.frame % 128 == 0 {
                    self.color = self.new_random_color();
                }
                self.tick_pulse()
            }
            RgbAnimType::PulseSolid(_) => self.tick_pulse(),
            RgbAnimType::Input => (),
            RgbAnimType::InputSolid(_) => (),
        }
        self.frame = self.frame.wrapping_add(1);
        &self.led_data
    }

    pub fn on_key_event(&mut self, i: u8, j: u8, is_press: bool) {
        match self.animation {
            RgbAnimType::Input => {
                self.led_data[self.get_led_index(i, j)] = if is_press {
                    RGB8::from(self.prng.random())
                } else {
                    RGB8::default()
                };
            }
            RgbAnimType::InputSolid(color) => {
                self.led_data[self.get_led_index(i, j)] = if is_press {
                    RGB8::indexed(color)
                } else {
                    RGB8::default()
                };
            }
            _ => {}
        }
    }

    /// Cycle to the next animation
    pub fn next_animation(&mut self) -> RgbAnimType {
        // Reset the frame
        self.frame = 0;
        // Shutdown the leds
        self.fill_color(RGB8::default());
        let anim = if let Some(saved_animation) = self.saved_animation {
            saved_animation
        } else {
            self.animation
        };

        match anim {
            RgbAnimType::Off => {
                self.animation = RgbAnimType::SolidColor(0);
                self.fill_color(RGB8::indexed(0));
            }
            RgbAnimType::SolidColor(0) => {
                self.animation = RgbAnimType::SolidColor(DEFAULT_COLOR_INDEX);
                self.fill_color(RGB8::indexed(DEFAULT_COLOR_INDEX));
            }
            RgbAnimType::SolidColor(_) => {
                self.animation = RgbAnimType::Wheel;
            }
            RgbAnimType::Wheel => {
                self.animation = RgbAnimType::Pulse;
            }
            RgbAnimType::Pulse => {
                self.animation = RgbAnimType::PulseSolid(DEFAULT_COLOR_INDEX);
                self.color = RGB8::indexed(DEFAULT_COLOR_INDEX);
            }
            RgbAnimType::PulseSolid(_) => {
                self.animation = RgbAnimType::Input;
                self.color = self.new_random_color();
            }
            RgbAnimType::Input => {
                self.animation = RgbAnimType::InputSolid(DEFAULT_COLOR_INDEX);
                self.color = RGB8::indexed(DEFAULT_COLOR_INDEX);
            }
            RgbAnimType::InputSolid(_) => {
                self.animation = RgbAnimType::Off;
                self.color = RGB8::indexed(DEFAULT_COLOR_INDEX);
            }
        }
        if self.saved_animation.is_some() {
            self.saved_animation = Some(self.animation);
        }
        self.animation
    }

    /// Set the Animation
    pub fn set_animation(&mut self, animation: RgbAnimType) {
        info!("Set animation: {:?}", animation);
        self.animation = animation;
        if self.saved_animation.is_some() {
            self.saved_animation = Some(animation);
        }
        self.frame = 0;
        self.fill_color(RGB8::default());
    }

    /// Set the color of all leds to a solid color, temporarily
    pub fn temporarily_solid_color(&mut self, color: u8) {
        self.frame = 0;
        if self.animation == RgbAnimType::Off {
            return;
        }
        if self.saved_animation.is_none() {
            self.saved_animation = Some(self.animation);
        }
        self.animation = RgbAnimType::SolidColor(color);
        self.fill_color(RGB8::indexed(color));
    }

    /// Restore the animation
    pub fn restore_animation(&mut self) {
        self.frame = 0;
        if let Some(animation) = self.saved_animation {
            self.animation = animation;
            self.saved_animation = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rgb_anim_type_serde() {
        let types = [
            RgbAnimType::Off,
            RgbAnimType::SolidColor(0),
            RgbAnimType::SolidColor(31),
            RgbAnimType::Wheel,
            RgbAnimType::Pulse,
            RgbAnimType::PulseSolid(0),
            RgbAnimType::PulseSolid(31),
            RgbAnimType::Input,
            RgbAnimType::InputSolid(0),
            RgbAnimType::InputSolid(31),
        ];
        for t in types.iter() {
            let value = t.to_u8().unwrap();
            let t2 = RgbAnimType::from_u8(value).unwrap();
            assert_eq!(*t, t2);
        }
    }
}
