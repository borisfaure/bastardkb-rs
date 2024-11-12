//! Logging utilities

#[cfg(not(target_arch = "x86_64"))]
pub use defmt::*;

#[cfg(target_arch = "x86_64")]
pub use log::*;
