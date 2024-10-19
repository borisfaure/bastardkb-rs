use crate::layout::CustomEvent::{self, *};
use keyberon::action::Action;
use keyberon::layout::Layout;

/// Keyboard Layout type to mask the number of layers
pub type KBLayout = Layout<10, 4, 2, CustomEvent>;

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

#[rustfmt::skip]
/// Layout
pub static LAYERS: keyberon::layout::Layers<10, 4, 2, CustomEvent> = keyberon::layout::layout! {
    { // 0: Base Layer
        [ Q  W  E  R  T      Y  U  I  O  P ],
        [ A  S  D  F  G      H  J  K  L  ; ],
        [ Z  X  C  V  B      N  M  ,  .  / ],
        [ 3  n  1  2  n      n  n  5  n  4 ],
    } { // Unreachable
        [ n  n  n  n  n      n  n  n  n  n ],
        [ n  n  n  n  n      n  n  n  n  n ],
        [ n  n  n  n  n      n  n  n  n  n ],
        [ n {BIW} {INC} {DEC} {MLC}      {MRC} {MMC} {RGB} n  n ],
    }
};
