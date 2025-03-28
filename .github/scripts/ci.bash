#!/usr/bin/env bash
# Script for running check on your rust projects.
set -e
set -x
set -u

declare -A KEYMAPS
KEYMAPS=(
    [0]="keymap_borisfaure"
    [1]="keymap_basic"
    [2]="keymap_test"
)
declare -A EXAMPLES
EXAMPLES=(
    [0]="blinky_led"
    [1]="blinky_led_liatris"
    [2]="encoder"
    [3]="pio_comms"
    [4]="pio_ping"
    [5]="pio_ping_pong"
    [6]="protocol"
    [7]="rgb_leds"
)
declare -A MODELS
MODELS=(
    [0]="dilemma"
    [1]="cnano"
)


run_doc() {
    rustup component add rust-docs
    for EXAMPLE in "${EXAMPLES[@]}"
    do
        cargo doc --example "$EXAMPLE"
    done
    for MODEL in "${MODELS[@]}"
    do
        for KEYMAP in "${KEYMAPS[@]}"
        do
            cargo doc --no-default-features --features "${KEYMAP},${MODEL}"
        done
    done
}

run_fmt() {
    rustup component add rustfmt
    cargo fmt --check
}

run_clippy() {
    rustup component add clippy-preview
    for EXAMPLE in "${EXAMPLES[@]}"
    do
        cargo clippy --example "$EXAMPLE" -- -D warnings
    done
    for MODEL in "${MODELS[@]}"
    do
        for KEYMAP in "${KEYMAPS[@]}"
        do
            cargo clippy --no-default-features --features "${KEYMAP},${MODEL}" -- -D warnings
        done
    done
}

run_check() {
    for EXAMPLE in "${EXAMPLES[@]}"
    do
        cargo check --example "$EXAMPLE"
    done
    for MODEL in "${MODELS[@]}"
    do
        for KEYMAP in "${KEYMAPS[@]}"
        do
            cargo check --no-default-features --features "${KEYMAP},${MODEL}"
        done
    done
}

run_test() {
    cargo test -p utils --target "x86_64-unknown-linux-gnu"
}

run_build() {
    cargo install flip-link
    for EXAMPLE in "${EXAMPLES[@]}"
    do
        cargo build --example "$EXAMPLE"
    done
    for MODEL in "${MODELS[@]}"
    do
        for KEYMAP in "${KEYMAPS[@]}"
        do
            cargo build --no-default-features --features "${KEYMAP},${MODEL}"
        done
    done
}

run_build_release() {
    cargo install flip-link
    for EXAMPLE in "${EXAMPLES[@]}"
    do
        cargo build --release --example "$EXAMPLE"
    done
    for MODEL in "${MODELS[@]}"
    do
        for KEYMAP in "${KEYMAPS[@]}"
        do
            cargo build --release --no-default-features --features "${KEYMAP},${MODEL}"
        done
    done
}

case $1 in
    doc)
        run_doc
        ;;
    fmt)
        run_fmt
        ;;
    check)
        run_check
        ;;
    clippy)
        run_clippy
        ;;
    test)
        run_test
        ;;
    build)
        run_build
        ;;
    build-release)
        run_build_release
        ;;
esac
