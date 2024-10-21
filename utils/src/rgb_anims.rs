//! Compule LED Data to render RGB Animations

/// Number of LEDs on each side
pub const NUM_LEDS: usize = 18;

/// RGB Animation Type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RgbAnimType {
    /// No animation, leds are off
    Off,
    SolidColor(u8), // u8 is the color in the form 3-bit red, 3-bit green,
    // 2-bit blue
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

/// RGB Color
#[derive(Debug, Clone, Copy, Default, PartialEq)]
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
}

impl From<u8> for RGB8 {
    fn from(i: u8) -> Self {
        let r = (i >> 5) & 0b111;
        let g = (i >> 2) & 0b111;
        let b = i & 0b11;
        RGB8 {
            r: r << 5 | r << 2 | (r >> 1),
            g: g << 5 | g << 2 | (g >> 1),
            b: b << 6 | b << 4 | b << 2 | b,
        }
    }
}

pub struct RgbAnim {
    /// The current animation frame
    pub frame: u8,
    /// The current animation
    pub animation: RgbAnimType,

    /// The LED data
    pub led_data: [RGB8; NUM_LEDS],
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

impl RgbAnim {
    /// Create a new RGB Animation
    pub fn new() -> Self {
        RgbAnim {
            frame: 0,
            animation: RgbAnimType::Pulse,
            led_data: [RGB8::default(); NUM_LEDS],
        }
    }

    /// Set color of all LEDs
    pub fn set_color(&mut self, color: RGB8) {
        for led in self.led_data.iter_mut() {
            *led = color;
        }
    }

    pub fn tick_wheel(&mut self) {
        for (i, led) in self.led_data.iter_mut().enumerate().take(NUM_LEDS) {
            *led = wheel((((i * 256) as u16 / NUM_LEDS as u16 + self.frame as u16) & 255) as u8);
        }
    }

    pub fn tick(&mut self) {
        self.frame = self.frame.wrapping_add(1);
        match self.animation {
            RgbAnimType::Off => self.set_color(RGB8::default()),
            RgbAnimType::SolidColor(color) => self.set_color(RGB8::from(color)),
            RgbAnimType::Wheel => self.tick_wheel(),
            _ => todo!(),
        }
    }
}
