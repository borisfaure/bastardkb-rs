//! Compule LED Data to render RGB Animations

/// Number of LEDs on each side
pub const NUM_LEDS: usize = 18;
/// Keyboard matrix rows
pub const ROWS: usize = 4;
/// Keyboard matrix columns
pub const COLS: usize = 5;

/// Default color: red
pub const DEFAULT_COLOR_U8: u8 = 0b1110_0000;

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
    frame: u8,
    /// The current animation
    animation: RgbAnimType,

    /// The LED data
    led_data: [RGB8; NUM_LEDS],

    /// Whether the animation is on the right side
    is_right: bool,
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
    [12, 9, 8, 3, 2],
    [13, 10, 7, 4, 1],
    [14, 11, 6, 5, 0],
    [15, 16, 255, 255, 255],
];
/// Index of leds on the left side
const MATRIX_LED_LEFT: [[usize; COLS]; ROWS] = [
    [2, 3, 8, 9, 12],
    [1, 4, 7, 10, 13],
    [0, 5, 6, 11, 14],
    [255, 255, 15, 16, 17],
];

impl RgbAnim {
    /// Create a new RGB Animation
    pub fn new(is_right: bool) -> Self {
        RgbAnim {
            frame: 0,
            animation: RgbAnimType::Pulse,
            led_data: [RGB8::default(); NUM_LEDS],
            is_right,
        }
    }

    /// Get the LED index for a key
    fn get_led_index(&self, i: u8, j: u8) -> usize {
        if self.is_right {
            MATRIX_LED_RIGHT[i as usize][j as usize]
        } else {
            MATRIX_LED_LEFT[i as usize][j as usize]
        }
    }

    /// Set color of all LEDs
    fn set_color(&mut self, color: RGB8) {
        for led in self.led_data.iter_mut() {
            *led = color;
        }
    }

    pub fn tick_wheel(&mut self) {
        for (i, led) in self.led_data.iter_mut().enumerate().take(NUM_LEDS) {
            *led = wheel((((i * 256) as u16 / NUM_LEDS as u16 + self.frame as u16) & 255) as u8);
        }
    }

    pub fn tick(&mut self) -> &[RGB8; NUM_LEDS] {
        self.frame = self.frame.wrapping_add(1);
        match self.animation {
            RgbAnimType::Off => self.set_color(RGB8::default()),
            RgbAnimType::SolidColor(color) => self.set_color(RGB8::from(color)),
            RgbAnimType::Wheel => self.tick_wheel(),
            _ => todo!(),
        }
        &self.led_data
    }

    pub fn on_key_event(&mut self, i: u8, j: u8, is_press: bool) {
        match self.animation {
            RgbAnimType::Input => {
                self.led_data[self.get_led_index(i, j)] = if is_press {
                    wheel(self.frame)
                } else {
                    RGB8::default()
                };
            }
            RgbAnimType::InputSolid(color) => {
                self.led_data[self.get_led_index(i, j)] = if is_press {
                    RGB8::from(color)
                } else {
                    RGB8::default()
                };
            }
            _ => {}
        }
    }

    pub fn next_animation(&mut self) {
        self.set_animation(match self.animation {
            RgbAnimType::Off => RgbAnimType::SolidColor(DEFAULT_COLOR_U8),
            RgbAnimType::SolidColor(_) => RgbAnimType::Wheel,
            RgbAnimType::Wheel => RgbAnimType::Pulse,
            RgbAnimType::Pulse => RgbAnimType::PulseSolid(DEFAULT_COLOR_U8),
            RgbAnimType::PulseSolid(_) => RgbAnimType::Input,
            RgbAnimType::Input => RgbAnimType::InputSolid(DEFAULT_COLOR_U8),
            RgbAnimType::InputSolid(_) => RgbAnimType::Off,
        });
    }

    pub fn set_animation(&mut self, anim: RgbAnimType) {
        self.animation = anim;
        self.frame = 0;
        self.set_color(RGB8::default());
    }
}
