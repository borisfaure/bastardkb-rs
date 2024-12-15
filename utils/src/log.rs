//! Logging utilities

#[cfg(not(target_arch = "x86_64"))]
pub use defmt::*;

#[cfg(target_arch = "x86_64")]
pub use log::*;

#[cfg(target_arch = "x86_64")]
use std::fmt;

#[cfg(target_arch = "x86_64")]
/// Wrapper to implement Display for Debug
pub struct Debug2Format<'a, T: fmt::Debug + ?Sized>(pub &'a T);

#[cfg(target_arch = "x86_64")]
/// impl Display for Debug2Format
impl<'a, T: fmt::Debug + ?Sized> fmt::Display for Debug2Format<'a, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}
