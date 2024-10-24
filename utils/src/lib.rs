#![cfg_attr(not(feature = "std"), no_std)]

/// Serialization and deserialization of key events
pub mod serde;

/// Compule LED Data to render RGB Animations
pub mod rgb_anims;

/// Pseudo-random number generator
pub mod prng;
