use crate::core::CustomEvent::{self, *};
use crate::keys::{FULL_COLS, ROWS};
use core::fmt::Debug;
use keyberon::action::{
    Action,
    SequenceEvent::{self, *},
};
use keyberon::key_code::KeyCode::*;
use keyberon::layout::Layout;

/// Number of layers
pub const NB_LAYERS: usize = 2;

/// Keyboard Layout type to mask the number of layers
pub type KBLayout = Layout<FULL_COLS, ROWS, NB_LAYERS, CustomEvent>;

/// A shortcut to create a `Action::Sequence`, useful to
/// create compact layout.
const fn seq<T, K>(events: &'static &'static [SequenceEvent<K>]) -> Action<T, K>
where
    T: 'static + Debug,
    K: 'static + Debug,
{
    Action::Sequence(events)
}

/// write `qwe`
const QQ: Action<CustomEvent> = seq(&[Tap(Q), Tap(W), Tap(E)].as_slice());
/// write `aze`
const AA: Action<CustomEvent> = seq(&[Tap(A), Tap(Z), Tap(E)].as_slice());

/// Mouse left click
const MLC: Action<CustomEvent> = Action::Custom(MouseLeftClick);
/// Mouse right click
const MRC: Action<CustomEvent> = Action::Custom(MouseRightClick);
/// Mouse middle click
const MMC: Action<CustomEvent> = Action::Custom(MouseWheelClick);
/// Ball is Wheel
const BIW: Action<CustomEvent> = Action::Custom(BallIsWheel);
/// Increase sensor CPI
#[cfg(feature = "cnano")]
const INC: Action<CustomEvent> = Action::Custom(IncreaseCpi);
#[cfg(feature = "dilemma")]
const INC: Action<CustomEvent> = Action::NoOp;
/// Decrease sensor CPI
#[cfg(feature = "cnano")]
const DEC: Action<CustomEvent> = Action::Custom(DecreaseCpi);
#[cfg(feature = "dilemma")]
const DEC: Action<CustomEvent> = Action::NoOp;
/// RGB LED control
const RGB: Action<CustomEvent> = Action::Custom(NextLedAnimation);
/// Reset to USB Mass Storage
const RST: Action<CustomEvent> = Action::Custom(ResetToUsbMassStorage);
/// Wheel up
#[cfg(feature = "cnano")]
const WHUP: Action<CustomEvent> = Action::NoOp;
#[cfg(feature = "dilemma")]
const WHUP: Action<CustomEvent> = Action::Custom(WheelUp);
/// Wheel down
#[cfg(feature = "cnano")]
const WHDN: Action<CustomEvent> = Action::NoOp;
#[cfg(feature = "dilemma")]
const WHDN: Action<CustomEvent> = Action::Custom(WheelDown);

/// No mouse action
const NOM: Action<CustomEvent> = Action::Custom(NoMouseAction);

// Virtual mouse key row/col
pub const VIRTUAL_MOUSE_KEY: (u8, u8) = (3, 0);

#[rustfmt::skip]
/// Layout
pub static LAYERS: keyberon::layout::Layers<FULL_COLS, ROWS, NB_LAYERS, CustomEvent> = keyberon::layout::layout! {
    { // 0: Base Layer
        [ {QQ}  W   E   R  T      Y  U  I  O  P ],
        [  A   S   D   F  G      H  J  K  L  ; ],
        [  Z   X   C   V  B      N  M  ,  .  / ],
        [  n   n  (1)  2  3      4  5  6  n  n ],
    } { /* 1: LOWER */
        [  !   #  $    '(' ')'     ^       &       |       *    {RST} ],
        [ {AA}  -  '`'  '{' '}'    Left    Down    Up     Right  '\\' ],
        [ {WHUP} {WHDN} n  n  n    {RGB}   n       n      n     {NOM} ],
        [ {INC} {DEC} {BIW} n  RAlt Escape  Delete  {MLC} {MMC} {MRC} ],
    }
};
