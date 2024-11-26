use crate::core::CustomEvent::{self, *};
use crate::keys::{FULL_COLS, ROWS};
use keyberon::action::Action;
use keyberon::layout::Layout;

/// Number of layers
pub const NB_LAYERS: usize = 2;

/// Keyboard Layout type to mask the number of layers
pub type KBLayout = Layout<FULL_COLS, ROWS, NB_LAYERS, CustomEvent>;

/// Mouse left click
const MLC: Action<CustomEvent> = Action::Custom(MouseLeftClick);
/// Mouse right click
const MRC: Action<CustomEvent> = Action::Custom(MouseRightClick);
/// Mouse middle click
const MMC: Action<CustomEvent> = Action::Custom(MouseWheelClick);
/// Ball is Wheel
const BIW: Action<CustomEvent> = Action::Custom(BallIsWheel);
/// Increase sensor CPI
const INC: Action<CustomEvent> = Action::Custom(IncreaseCpi);
/// Decrease sensor CPI
const DEC: Action<CustomEvent> = Action::Custom(DecreaseCpi);

/// RGB LED control
const RGB: Action<CustomEvent> = Action::Custom(NextLedAnimation);
/// Reset to USB Mass Storage
const RST: Action<CustomEvent> = Action::Custom(ResetToUsbMassStorage);

#[rustfmt::skip]
/// Layout
pub static LAYERS: keyberon::layout::Layers<FULL_COLS, ROWS, NB_LAYERS, CustomEvent> = keyberon::layout::layout! {
    { // 0: Base Layer
        [ Q  W  E  R  T      Y  U  I  O  P ],
        [ A  S  D  F  G      H  J  K  L  ; ],
        [ Z  X  C  V  B      N  M  ,  .  / ],
        [ n  n  1  2  3      4  5  n  n  n ],
    } { // Unreachable
        [ n  n  n  n  n      n  n  n  n  n ],
        [ n  n  n  n  n      n  n  n  n  n ],
        [ n  n  n  n  n      n  n  n  n  n ],
        [ n {BIW} {INC} {DEC} {MLC}      {MRC} {MMC} {RGB} n {RST} ],
    }
};
