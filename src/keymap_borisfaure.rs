use crate::layout::CustomEvent;
use crate::layout::CustomEvent::*;
use core::fmt::Debug;
use keyberon::action::{
    d, k, l, m, Action, HoldTapAction, HoldTapConfig,
    SequenceEvent::{self, Filter, Press, Release, Restore, Tap},
};
use keyberon::key_code::KeyCode::*;
use keyberon::layout::Layout;

/// Keyboard Layout type to mask the number of layers
pub type KBLayout = Layout<10, 4, 9, CustomEvent>;

/// Timeout to consider a key as held
const TIMEOUT: u16 = 200;
/// Disable tap_hold_interval
const TAP_HOLD_INTERVAL: u16 = 0;

/// Helper to create a HoldTapAction
macro_rules! ht {
    ($h:expr, $t:expr) => {
        Action::HoldTap(&HoldTapAction {
            timeout: TIMEOUT,
            tap_hold_interval: TAP_HOLD_INTERVAL,
            config: HoldTapConfig::Default,
            hold: $h,
            tap: $t,
        })
    };
}

/// COLEMAN-DH layer
const L_COLEMAN: usize = 0;
/// LOWER layer
const L_LOWER: usize = 1;
/// RAISE layer
const L_RAISE: usize = 2;
/// NUMBERS layer
const L_NUM: usize = 3;
/// MISC layer
const L_MISC: usize = 4;
/// TMUX layer
const L_TMUX: usize = 5;
/// GAMING layer
const L_GAMING: usize = 6;
/// CAPS layer
const L_CAPS: usize = 7;
/// QWERTY layer
const L_QWERTY: usize = 8;

/// Win when held, or W
const HT_W_W: Action<CustomEvent> = ht!(k(LGui), k(W));
/// Win when held, or O
const HT_W_O: Action<CustomEvent> = ht!(k(RGui), k(O));
/// Win when held, or Y
const HT_W_Y: Action<CustomEvent> = ht!(k(RGui), k(Y));
/// Left Control when held, or A
const HT_C_A: Action<CustomEvent> = ht!(k(LCtrl), k(A));
/// Right Control when held, or SemiColon
const HT_C_SC: Action<CustomEvent> = ht!(k(RCtrl), k(SColon));
/// Right Control when held, or O
const HT_C_O: Action<CustomEvent> = ht!(k(RCtrl), k(O));
/// Left Shift when held, or Z
const HT_S_Z: Action<CustomEvent> = ht!(k(LShift), k(Z));
/// Right Shift when held, or Slash
const HT_S_SL: Action<CustomEvent> = ht!(k(RShift), k(Slash));
/// Left Alt when held, or X
const HT_A_X: Action<CustomEvent> = ht!(k(LAlt), k(X));
/// Left Alt when held, or .
const HT_A_DOT: Action<CustomEvent> = ht!(k(LAlt), k(Dot));

/// Layer 1 (lower) when held, or Space
const HT_1_SP: Action<CustomEvent> = ht!(l(L_LOWER), k(Space));

/// Layer 2 (raise) when held, or BackSpace
const HT_2_BS: Action<CustomEvent> = ht!(l(L_RAISE), k(BSpace));

/// Layer 3 (numbers/Fx) when held, or B
const HT_3_B: Action<CustomEvent> = ht!(l(L_NUM), k(B));
/// Layer 3 (numbers/Fx) when held, or N
const HT_3_N: Action<CustomEvent> = ht!(l(L_NUM), k(N));
/// Layer 3 (numbers/Fx) when held, or V
const HT_3_V: Action<CustomEvent> = ht!(l(L_NUM), k(V));
/// Layer 3 (numbers/Fx) when held, or J
const HT_3_J: Action<CustomEvent> = ht!(l(L_NUM), k(J));
/// Layer 3 (numbers/Fx) when held, or RAlt
const HT_3_RA: Action<CustomEvent> = ht!(l(L_NUM), k(RAlt));
/// Layer 3 (numbers/Fx) when held, or Escape
const HT_3_ESC: Action<CustomEvent> = ht!(l(L_NUM), k(Escape));

/// Layer 4 (misc) when held, or T
const HT_4_T: Action<CustomEvent> = ht!(l(L_MISC), k(T));
/// Layer 4 (misc) when held, or Y
const HT_4_Y: Action<CustomEvent> = ht!(l(L_MISC), k(Y));
/// Layer 4 (misc) when held, or B
const HT_4_B: Action<CustomEvent> = ht!(l(L_MISC), k(B));
/// Layer 4 (misc) when held, or K
const HT_4_K: Action<CustomEvent> = ht!(l(L_MISC), k(K));

/// Layer 5 (tmux) when held, or F
const HT_5_F: Action<CustomEvent> = ht!(l(L_TMUX), k(F));
/// Layer 5 (tmux) when held, or T
const HT_5_T: Action<CustomEvent> = ht!(l(L_TMUX), k(T));

/// Shift-Insert
const S_INS: Action<CustomEvent> = m(&[LShift, Insert].as_slice());

/// Caps Mode
const CAPS: Action<CustomEvent> = k(CapsLock);
/// Caps from LED change
const VCAPS: Action<CustomEvent> = d(L_CAPS);

/// Unset Caps Mode
const UNCAPS: Action<CustomEvent> = k(CapsLock);
/// Unset Caps from LED change
const VUNCAPS: Action<CustomEvent> = d(L_COLEMAN);

/// Num Lock Mode
const NUMLCK: Action<CustomEvent> = k(NumLock);
/// Num Lock from LED change
const VNUM: Action<CustomEvent> = d(L_NUM);

/// Unset Num Lock Mode
const UNNUM: Action<CustomEvent> = k(NumLock);
/// Unset Num Lock from LED change
const VUNNUM: Action<CustomEvent> = d(L_COLEMAN);

/// Change default layer to GAMING
const GAME: Action<CustomEvent> = d(L_GAMING);
/// Change default layer to QWERTY
const QWERTY: Action<CustomEvent> = d(L_QWERTY);
/// Change default layer to COLEMAN_DH
const COLEMAN: Action<CustomEvent> = d(L_COLEMAN);

/// A shortcut to create a `Action::Sequence`, useful to
/// create compact layout.
const fn seq<T, K>(events: &'static &'static [SequenceEvent<K>]) -> Action<T, K>
where
    T: 'static + Debug,
    K: 'static + Debug,
{
    Action::Sequence(events)
}

/// à or À
const A_GRV: Action<CustomEvent> = seq(&[
    Filter(&[LShift, RShift].as_slice()),
    Tap(RAlt),
    Tap(Grave),
    Restore,
    Tap(A),
]
.as_slice());
/// è or È
const E_GRV: Action<CustomEvent> = seq(&[
    Filter(&[LShift, RShift].as_slice()),
    Tap(RAlt),
    Tap(Grave),
    Restore,
    Tap(E),
]
.as_slice());
/// ù or Ù
const U_GRV: Action<CustomEvent> = seq(&[
    Filter(&[LShift, RShift].as_slice()),
    Tap(RAlt),
    Tap(Grave),
    Restore,
    Tap(U),
]
.as_slice());
/// é or É
const E_ACU: Action<CustomEvent> = seq(&[
    Filter(&[LShift, RShift].as_slice()),
    Tap(RAlt),
    Tap(Quote),
    Restore,
    Tap(E),
]
.as_slice());
/// ê or Ê
const E_CIR: Action<CustomEvent> = seq(&[
    Filter(&[LShift, RShift].as_slice()),
    Tap(RAlt),
    Press(LShift),
    Tap(Kb6),
    Release(LShift),
    Restore,
    Tap(E),
]
.as_slice());
/// î or Î
const I_CIR: Action<CustomEvent> = seq(&[
    Filter(&[LShift, RShift].as_slice()),
    Tap(RAlt),
    Press(LShift),
    Tap(Kb6),
    Release(LShift),
    Restore,
    Tap(I),
]
.as_slice());
/// ô or Ô
const O_CIR: Action<CustomEvent> = seq(&[
    Filter(&[LShift, RShift].as_slice()),
    Tap(RAlt),
    Press(LShift),
    Tap(Kb6),
    Release(LShift),
    Restore,
    Tap(O),
]
.as_slice());
/// ç or Ç
const C_CED: Action<CustomEvent> = seq(&[
    Filter(&[LShift, RShift].as_slice()),
    Tap(RAlt),
    Tap(Comma),
    Restore,
    Tap(C),
]
.as_slice());
/// œ or Œ
const OE: Action<CustomEvent> = seq(&[
    Filter(&[LShift, RShift].as_slice()),
    Tap(RAlt),
    Restore,
    Tap(O),
    Tap(E),
]
.as_slice());
/// €
const EURO: Action<CustomEvent> = seq(&[
    Filter(&[LShift, RShift].as_slice()),
    Tap(RAlt),
    Tap(Equal),
    Press(LShift),
    Tap(E),
    Release(LShift),
    Restore,
]
.as_slice());
/// …
const DOTS: Action<CustomEvent> = seq(&[
    Filter(&[LShift, RShift].as_slice()),
    Tap(RAlt),
    Tap(Dot),
    Tap(Dot),
    Restore,
]
.as_slice());

/// Tmux: new window
const T_NEW: Action<CustomEvent> = seq(&[Press(LCtrl), Tap(A), Release(LCtrl), Tap(C)].as_slice());
/// Tmux: previous window
const T_PRV: Action<CustomEvent> = seq(&[Press(LCtrl), Tap(A), Release(LCtrl), Tap(P)].as_slice());
/// Tmux: next window
const T_NXT: Action<CustomEvent> = seq(&[Press(LCtrl), Tap(A), Release(LCtrl), Tap(N)].as_slice());
/// Tmux: last window
const T_LST: Action<CustomEvent> = seq(&[Press(LCtrl), Tap(A), Release(LCtrl), Tap(L)].as_slice());
/// Tmux: command
const T_CMD: Action<CustomEvent> = seq(&[
    Press(LCtrl),
    Tap(A),
    Release(LCtrl),
    Press(LShift),
    Tap(SColon),
    Release(LShift),
]
.as_slice());
/// Tmux: copy
const T_CPY: Action<CustomEvent> =
    seq(&[Press(LCtrl), Tap(A), Release(LCtrl), Tap(LBracket)].as_slice());
/// Tmux: paste
const T_PST: Action<CustomEvent> =
    seq(&[Press(LCtrl), Tap(A), Release(LCtrl), Tap(RBracket)].as_slice());
/// Tmux: scroll up
const T_UP: Action<CustomEvent> =
    seq(&[Press(LCtrl), Tap(A), Release(LCtrl), Tap(PgUp)].as_slice());
/// Tmux: scroll down
const T_DWN: Action<CustomEvent> =
    seq(&[Press(LCtrl), Tap(A), Release(LCtrl), Tap(PgDown)].as_slice());
/// Tmux: move
const T_MOV: Action<CustomEvent> =
    seq(&[Press(LCtrl), Tap(A), Release(LCtrl), Tap(Dot)].as_slice());
/// Tmux: rename
const T_RNM: Action<CustomEvent> =
    seq(&[Press(LCtrl), Tap(A), Release(LCtrl), Tap(Comma)].as_slice());
/// Tmux: go to window 1
const T_1: Action<CustomEvent> = seq(&[Press(LCtrl), Tap(A), Release(LCtrl), Tap(Kb1)].as_slice());
/// Tmux: go to window 2
const T_2: Action<CustomEvent> = seq(&[Press(LCtrl), Tap(A), Release(LCtrl), Tap(Kb2)].as_slice());
/// Tmux: go to window 3
const T_3: Action<CustomEvent> = seq(&[Press(LCtrl), Tap(A), Release(LCtrl), Tap(Kb3)].as_slice());
/// Tmux: go to window 4
const T_4: Action<CustomEvent> = seq(&[Press(LCtrl), Tap(A), Release(LCtrl), Tap(Kb4)].as_slice());
/// Tmux: go to window 5
const T_5: Action<CustomEvent> = seq(&[Press(LCtrl), Tap(A), Release(LCtrl), Tap(Kb5)].as_slice());
/// Tmux: go to window 6
const T_6: Action<CustomEvent> = seq(&[Press(LCtrl), Tap(A), Release(LCtrl), Tap(Kb6)].as_slice());
/// Tmux: go to window 7
const T_7: Action<CustomEvent> = seq(&[Press(LCtrl), Tap(A), Release(LCtrl), Tap(Kb7)].as_slice());
/// Tmux: go to window 8
const T_8: Action<CustomEvent> = seq(&[Press(LCtrl), Tap(A), Release(LCtrl), Tap(Kb8)].as_slice());
/// Tmux: go to window 9
const T_9: Action<CustomEvent> = seq(&[Press(LCtrl), Tap(A), Release(LCtrl), Tap(Kb9)].as_slice());
/// Tmux: go to window 0
const T_0: Action<CustomEvent> = seq(&[Press(LCtrl), Tap(A), Release(LCtrl), Tap(Kb0)].as_slice());

/// Mouse move up
const MU: Action<CustomEvent> = Action::Custom(MouseUp);
/// Mouse move down
const MD: Action<CustomEvent> = Action::Custom(MouseDown);
/// Mouse move left
const ML: Action<CustomEvent> = Action::Custom(MouseLeft);
/// Mouse move right
const MR: Action<CustomEvent> = Action::Custom(MouseRight);
/// Mouse left click
const MLC: Action<CustomEvent> = Action::Custom(MouseLeftClick);
/// Mouse right click
const MRC: Action<CustomEvent> = Action::Custom(MouseRightClick);
/// Mouse middle click
const MMC: Action<CustomEvent> = Action::Custom(MouseMiddleClick);
/// Mouse scroll up
const MSU: Action<CustomEvent> = Action::Custom(MouseScrollUp);
/// Mouse scroll down
const MSD: Action<CustomEvent> = Action::Custom(MouseScrollDown);

#[rustfmt::skip]
/// Layout
pub static LAYERS: keyberon::layout::Layers<10, 4, 9, CustomEvent> = keyberon::layout::layout! {
   { /* 0: Coleman-DH */
[  Q         {HT_W_W}   F          P         {HT_4_B}    {HT_4_K}   L         U        {HT_W_Y}     ;        ],
[ {HT_C_A}    R         S         {HT_5_T}    G           M        {HT_3_N}   E         I          {HT_C_O}  ],
[ {HT_S_Z}   {HT_A_X}   C          D         {HT_3_V}    {HT_3_J}   H         ,        {HT_A_DOT}  {HT_S_SL} ],
[ {VCAPS}    {VNUM}    {HT_3_ESC} {HT_1_SP}   Tab         Enter    {HT_2_BS} {HT_3_RA}  n           n        ],
    } { /* 1: LOWER */
        [ !  #  $    '(' ')'      ^       &      {S_INS}    *      ~   ],
        [ =  -  '`'  '{' '}'      n       n       PgUp    PgDown  '\\' ],
        [ @  &  %    '[' ']'      n       n         n      '\''    '"' ],
        [ t  t  n     n   n       Enter  Space   Delete     t      t   ],
    } { /* 2: RAISE */
        [ {QWERTY}  n    {E_ACU}  {E_CIR}  {E_GRV}      PgUp   {U_GRV}  {I_CIR}  {O_CIR}  Home  ],
        [ {A_GRV}  '_'    +        &        |           RAlt    Left     Up       Down    Right ],
        [ {EURO}   {OE}  {C_CED}  {CAPS}   {NUMLCK}     PgDown  Menu    PScreen  {DOTS}   End   ],
        [ t         t    Escape   BSpace    Tab         n       n        n        t       t     ],
    } { /* 3: NUMBERS Fx */
        [ .  4  5   6         =         /    F1         F2   F3   F4   ],
        [ 0  1  2   3         -         *    F5         F6   F7   F8   ],
        [ ,  7  8   9         +         +    F9         F10  F11  F12  ],
        [ t {VUNNUM} {UNNUM}  {HT_1_SP} Tab     Enter  {HT_2_BS}    n    t    t   ],
    } { /* 4: MISC and Mouse */
        [ Pause  {GAME}           {COLEMAN}    {QWERTY}      n      {MSU}  n      n     n     n   ],
        [ n      VolDown          Mute         VolUp         n       n    {ML}  {MD}   {MU}  {MR} ],
        [ n MediaPreviousSong  MediaPlayPause MediaNextSong  n      {MSD}  n      n     n     n   ],
        [ t      t                {MLC}        {MMC}      {MRC}     {MLC} {MMC}  {MRC}  t     t   ],
    } { /* 5: TMUX */
        [ {T_6}   {T_7} {T_8}   {T_9}   {T_0}      {T_1}   {T_2}  {T_3}   {T_4}   {T_5}   ],
        [ {T_LST}  n     n       n       n          n     {T_PRV} {T_UP}  {T_DWN} {T_NXT} ],
        [  n       n    {T_NEW} {T_CPY} {T_PST}     n       n     {T_RNM} {T_MOV} {T_PST} ],
        [  t       t     n       n       n         {T_CMD}  n      n       t       t      ],
    } { /* 6: Gaming */
        [ Q  W  E   R         T              {HT_4_Y} U          I  {HT_W_O}     P       ],
        [ A  S  D   F         G               H       J          K   L         {HT_C_SC} ],
        [ Z  X  C   V         B               N       M          ,  {HT_A_DOT} {HT_S_SL} ],
        [ t  t  n  {HT_1_SP}  Tab             Enter   {HT_2_BS}  n   t          t        ],
    } { /* 7: Caps */
[  Q         {HT_W_W}   F         P         {HT_4_B}    {HT_4_K}   L        U  {HT_W_Y}     ;        ],
[ {HT_C_A}    R         S        {HT_5_T}    G           M         N        E   I          {HT_C_O}  ],
[ {HT_S_Z}   {HT_A_X}   C         D         {HT_3_V}    {HT_3_J}   H        ,  {HT_A_DOT}  {HT_S_SL} ],
[ {VUNCAPS}   t        {UNCAPS}  {HT_1_SP}   '_'         Enter   {HT_2_BS}  n   t           t        ],
    } { /* 8: QWERTY */
[  Q         {HT_W_W}   E       R         {HT_4_T}       {HT_4_Y}   U         I    {HT_W_O}     P        ],
[ {HT_C_A}    S         D      {HT_5_F}    G              H         J         K     L          {HT_C_SC} ],
[ {HT_S_Z}   {HT_A_X}   C       V         {HT_3_B}       {HT_3_N}   M         ,    {HT_A_DOT}  {HT_S_SL} ],
[  t          t        Escape  {HT_1_SP}   Tab           Enter    {HT_2_BS}  RAlt   t           t        ],
    }
};
