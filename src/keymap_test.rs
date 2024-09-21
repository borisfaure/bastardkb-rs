use crate::layout::CustomEvent;
use core::fmt::Debug;
use keyberon::action::{
    Action,
    SequenceEvent::{self, *},
};
use keyberon::key_code::KeyCode::*;
use keyberon::layout::Layout;

/// Keyboard Layout type to mask the number of layers
pub type KBLayout = Layout<10, 4, 2, CustomEvent>;

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

#[rustfmt::skip]
/// Layout
pub static LAYERS: keyberon::layout::Layers<10, 4, 2, CustomEvent> = keyberon::layout::layout! {
    { // 0: Base Layer
        [ {QQ}  W   E   R  T      Y  U  I  O  P ],
        [  A   S   D   F  G      H  J  K  L  ; ],
        [  Z   X   C   V  B      N  M  ,  .  / ],
        [  n   n  (1)  2  3      4  5  6  n  n ],
    } { /* 1: LOWER */
        [  !   #  $    '(' ')'    ^       &       |       *      ~   ],
        [ {AA}  -  '`'  '{' '}'    Left    Down    Up     Right  '\\' ],
        [  @   &  %    '[' ']'    n       n       Home   '\''   '"'  ],
        [  n   n  n     n  RAlt   Escape  Delete  n       n      n   ],
    }
};
