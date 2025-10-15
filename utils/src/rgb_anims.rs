//! Compule LED Data to render RGB Animations

use crate::log::*;
use crate::prng::XorShift32;
use crate::serde::Error as SerdeError;

/// Number of LEDs on each side
#[cfg(not(feature = "dilemma"))]
pub const NUM_LEDS: usize = 18;
#[cfg(feature = "dilemma")]
pub const NUM_LEDS: usize = 36;
/// Number of underglow LEDs
pub const UNDERGLOW_LEDS: usize = 18;
/// Keyboard matrix rows
pub const ROWS: usize = 4;
/// Keyboard matrix columns
pub const COLS: usize = 5;
/// Maximum light level per color. Must be usable as a mask
pub const MAX_LIGHT_LEVEL: u8 = 0xaf;

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

/// No color
const NO_COLOR: RGB8 = RGB8::default();
/// Orange color, used for RAISE layer
const ORANGE_COLOR: RGB8 = RGB8::new(0x40, 0x10, 0x00);
/// Green color, used for LOWER layer
const GREEN_COLOR: RGB8 = RGB8::new(0x00, 0x40, 0x00);
/// Purple color, used for MISC layer
const PURPLE_COLOR: RGB8 = RGB8::new(0x40, 0, 0x10);
/// Blue color, used for NUMBERS layer
const BLUE_COLOR: RGB8 = RGB8::new(0x00, 0x00, 0x40);
/// Red color, used for TMUX layer
const RED_COLOR: RGB8 = RGB8::new(0x07, 0, 0);
/// Gray color, used for GAMING layer
const GRAY_COLOR: RGB8 = RGB8::new(0x07, 0x07, 0x07);
/// Beige color, used for CAPS layer
const BEIGE_COLOR: RGB8 = RGB8::new(0x0f, 0x0f, 0x00);
/// Yellow color, used for QWERTY layer
const YELLOW_COLOR: RGB8 = RGB8::new(0x40, 0x30, 0x00);
/// Dark red color, used for MOUSE layer
const DARK_RED_COLOR: RGB8 = RGB8::new(0x40, 0, 0);
/// White color, used for ERROR layer
const WHITE_COLOR: RGB8 = RGB8::new(MAX_LIGHT_LEVEL, MAX_LIGHT_LEVEL, MAX_LIGHT_LEVEL);

/// Indexed colors
const INDEXED_COLORS: [RGB8; 11] = [
    NO_COLOR,
    ORANGE_COLOR,   // 1/ orange, RAISE
    GREEN_COLOR,    // 2/ green, LOWER
    PURPLE_COLOR,   // 3/ purple, MISC
    BLUE_COLOR,     // 4/ blue, NUMBERS
    RED_COLOR,      // 5/ red, TMUX
    GRAY_COLOR,     // 6/ gray, GAMING
    BEIGE_COLOR,    // 7/ beige, CAPS
    YELLOW_COLOR,   // 8/ yellow, QWERTY
    DARK_RED_COLOR, // 9/ dark red, MOUSE
    WHITE_COLOR,    // 10/ white, ERROR
];
/// Default color: dark red
const DEFAULT_COLOR_INDEX: u8 = 9;
/// Error color: orange
pub const ERROR_COLOR_INDEX: u8 = 10;

impl From<u32> for RGB8 {
    fn from(i: u32) -> Self {
        let r = ((i >> 24) as u8) & MAX_LIGHT_LEVEL;
        let g = ((i >> 16) as u8) & MAX_LIGHT_LEVEL;
        let b = ((i >> 8) as u8) & MAX_LIGHT_LEVEL;
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
#[cfg(not(feature = "dilemma"))]
const MATRIX_LED_RIGHT: [[usize; COLS]; ROWS] = [
    [2, 3, 8, 9, 12],
    [1, 4, 7, 10, 13],
    [0, 5, 6, 11, 14],
    [255, 255, 255, 15, 16],
];
#[cfg(feature = "dilemma")]
const MATRIX_LED_RIGHT: [[usize; COLS]; ROWS] = [
    [22, 21, 20, 19, 18],
    [23, 24, 25, 26, 27],
    [32, 31, 30, 29, 28],
    [99, 99, 99, 34, 35],
];
/// Index of leds on the left side
#[cfg(not(feature = "dilemma"))]
const MATRIX_LED_LEFT: [[usize; COLS]; ROWS] = [
    [2, 3, 8, 9, 12],
    [1, 4, 7, 10, 13],
    [0, 5, 6, 11, 14],
    [15, 16, 17, 255, 255],
];
#[cfg(feature = "dilemma")]
const MATRIX_LED_LEFT: [[usize; COLS]; ROWS] = [
    [22, 21, 20, 19, 18],
    [23, 24, 25, 26, 27],
    [32, 31, 30, 29, 28],
    [99, 99, 33, 34, 35],
];

///>>> from math import sin, pi; [int(sin(x/128.0*pi)**4*0xAF) for x in range(128)]
///
const PULSE_TABLE: [u16; 128] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 2, 2, 3, 4, 5, 7, 8, 10, 12, 14, 16, 19, 22, 25, 28,
    31, 35, 39, 43, 48, 52, 57, 62, 67, 72, 78, 83, 89, 94, 100, 105, 111, 116, 122, 127, 132, 137,
    142, 146, 150, 154, 158, 161, 164, 167, 169, 171, 173, 174, 174, 175, 174, 174, 173, 171, 169,
    167, 164, 161, 158, 154, 150, 146, 142, 137, 132, 127, 122, 116, 111, 105, 100, 94, 89, 83, 78,
    72, 67, 62, 57, 52, 48, 43, 39, 35, 31, 28, 25, 22, 19, 16, 14, 12, 10, 8, 7, 5, 4, 3, 2, 2, 1,
    1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
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

    /// Reset the leds
    fn reset(&mut self) {
        for led in self.led_data.iter_mut() {
            *led = RGB8::default();
        }
    }

    /// Set color of all LEDs
    fn fill_color(&mut self, color: RGB8) {
        for led in self.led_data.iter_mut().take(UNDERGLOW_LEDS) {
            *led = color;
        }
    }

    /// Tick the wheel animation
    fn tick_wheel(&mut self) {
        for (i, led) in self.led_data.iter_mut().enumerate().take(UNDERGLOW_LEDS) {
            *led = wheel(
                (((i * (MAX_LIGHT_LEVEL as usize)) as u16 / UNDERGLOW_LEDS as u16
                    + self.frame as u16)
                    & 255) as u8,
            );
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
            RgbAnimType::Off => self.reset(),
            RgbAnimType::SolidColor(idx) => self.fill_color(RGB8::indexed(idx)),
            RgbAnimType::Wheel => self.tick_wheel(),
            RgbAnimType::Pulse => {
                if self.frame.is_multiple_of(128) {
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
        self.reset();
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
        self.reset();
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
