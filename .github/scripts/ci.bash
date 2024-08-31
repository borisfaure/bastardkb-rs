#!/usr/bin/env bash
# Script for running check on your rust projects.
set -e
set -x
set -u

declare -A SIDES
SIDES=(
    [0]="right"
    [1]="left"
)
declare -A KEYMAPS
KEYMAPS=(
    [0]="keymap_borisfaure"
    [1]="keymap_basic"
    [2]="keymap_test"
)
declare -A EXAMPLES
EXAMPLES=(
    [0]="blinky_led"
)


run_doc() {
    rustup component add rust-docs
    for EXAMPLE in "${EXAMPLES[@]}"
    do
        cargo doc --example "$EXAMPLE"
    done
    for SIDE in "${SIDES[@]}"
    do
        for KEYMAP in "${KEYMAPS[@]}"
        do
            cargo doc --no-default-features --features "$SIDE,$KEYMAP"
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
    for SIDE in "${SIDES[@]}"
    do
        for KEYMAP in "${KEYMAPS[@]}"
        do
            cargo clippy --no-default-features --features "$SIDE,$KEYMAP" -- -D warnings
        done
    done
}

run_check() {
    for EXAMPLE in "${EXAMPLES[@]}"
    do
        cargo check --example "$EXAMPLE"
    done
    for SIDE in "${SIDES[@]}"
    do
        for KEYMAP in "${KEYMAPS[@]}"
        do
            cargo check --no-default-features --features "$SIDE,$KEYMAP"
        done
    done
}

run_test() {
    cargo test
    for SIDE in "${SIDES[@]}"
    do
        for KEYMAP in "${KEYMAPS[@]}"
        do
            cargo test --no-default-features --features "$SIDE,$KEYMAP"
        done
    done
}

run_build() {
    cargo install flip-link
    for EXAMPLE in "${EXAMPLES[@]}"
    do
        cargo build --example "$EXAMPLE"
    done
    for SIDE in "${SIDES[@]}"
    do
        for KEYMAP in "${KEYMAPS[@]}"
        do
            cargo build --no-default-features --features "$SIDE,$KEYMAP"
        done
    done
}

run_build_release() {
    cargo install flip-link
    for EXAMPLE in "${EXAMPLES[@]}"
    do
        cargo build --release --example "$EXAMPLE"
    done
    for SIDE in "${SIDES[@]}"
    do
        for KEYMAP in "${KEYMAPS[@]}"
        do
            cargo build --release --no-default-features --features "$SIDE,$KEYMAP"
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
