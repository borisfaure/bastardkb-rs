[![CI](https://github.com/borisfaure/cnano-rs/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/borisfaure/cnano-rs/actions/workflows/ci.yml)

# Rust Firmware for the Charybdis Nano keyboard

This firmware written in Rust is targetted for the
[Charybdis Nano keyboard](https://bastardkb.com/product/charybdis-nano-kit/).
It uses the [Elite-C Holder](https://github.com/Bastardkb/Elite-C-holder) with
a [Liatris Microcontroller](https://splitkb.com/products/liatris).

Two modifications have been made on the Elite-C Holder:

- The handness of the keyboard is detected by adding a 5.1kOhm resistor on R6
  marking.  The firmware reads the value on pin 15 of the MCU to detect the
  handness of the keyboard.  On the left side, the value is 0 since it is
  connected to the Ground pin.  On the right side, the value is 1 since it
  is connected to the VCC pin.
- A wire has been added between the pin 29 of the MCU and the unmarked pin of
  the jack connector.  The pin 29 was supposed to control the R1 row of the
  keyboard but it does not exist on the Nano model.  This way, full duplex
  communication is possible between the two halves of the keyboard.

The firmware is based on the [Keyberon library](https://github.com/TeXitoi/keyberon).

## Features

- Multi layers keymaps
- Multiple keymaps
- Hold Tap actions
- Sequences
- CapsLock & NumLock

## On CapsLock & NumLock support

The firmware generates an event on Col 0, Row 3 when the CapsLock led changes
states.  This is not a wired element but can be used to support CapsLock on
the keymap, to have a different behavior when CapsLock is set.

The same occurs with NumLock but the event is on Col 1, Row 3.

## What's missing

- No support for controlling the trackball
- RGB underglow leds
- One Shot Actions
- ...


## Installing the needed tools

Considering one has rust installed by [rustup.rs](https://rustup.rs), then
one has to run the following commands:

```shell
cargo install cargo-binutils
rustup component add llvm-tools-preview
cargo install probe-rs --features cli
cargo install elf2uf2-rs
```

## Compile & Flashing

The possible keymaps are:

- `keymap_basic`
- `keymap_borisfaure`
- `keymap_test`

### With probe-rs

In case a [probe-rs](https://probe.rs/) compatible debugger is available, the
firmware can be flashed using the following command for the `keymap_basic`:

```shell
cargo f --release --no-default-features --features="keymap_borisfaure"
```

### By installing a UF2 file on the device

The firmware can be compiled to a UF2 file using the following command:

```shell
cargo build --release --no-default-features --features="keymap_basic"
elf2uf2-rs target/thumbv6m-none-eabi/release/cnano-rs cnano-rs.uf2
```

Then, the UF2 file can be copied to the device.


## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  http://www.apache.org/licenses/LICENSE-2.0)

- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

