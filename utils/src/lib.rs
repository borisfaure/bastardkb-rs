#![cfg_attr(not(target_arch = "x86_64"), no_std)]

/// Serialization and deserialization of key events
pub mod serde;

/// Compule LED Data to render RGB Animations
pub mod rgb_anims;

/// Pseudo-random number generator
pub mod prng;

/// Logger
pub mod log;
