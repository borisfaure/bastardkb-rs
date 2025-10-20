//! Logging utilities

#[cfg(all(not(target_arch = "x86_64"), feature = "defmt"))]
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

// No-op implementations for embedded targets without defmt
#[cfg(all(not(target_arch = "x86_64"), not(feature = "defmt")))]
#[macro_export]
macro_rules! error {
    ($($arg:tt)*) => {{}};
}

#[cfg(all(not(target_arch = "x86_64"), not(feature = "defmt")))]
#[macro_export]
macro_rules! warn {
    ($($arg:tt)*) => {{}};
}

#[cfg(all(not(target_arch = "x86_64"), not(feature = "defmt")))]
#[macro_export]
macro_rules! info {
    ($($arg:tt)*) => {{}};
}

#[cfg(all(not(target_arch = "x86_64"), not(feature = "defmt")))]
#[macro_export]
macro_rules! debug {
    ($($arg:tt)*) => {{}};
}

#[cfg(all(not(target_arch = "x86_64"), not(feature = "defmt")))]
#[macro_export]
macro_rules! trace {
    ($($arg:tt)*) => {{}};
}

// Re-export at module level for convenience
#[cfg(all(not(target_arch = "x86_64"), not(feature = "defmt")))]
pub use crate::{debug, error, info, trace, warn};

#[cfg(all(not(target_arch = "x86_64"), not(feature = "defmt")))]
/// Wrapper to implement Display for Debug (no-op for non-defmt)
pub struct Debug2Format<'a, T: ?Sized>(pub &'a T);

#[cfg(all(not(target_arch = "x86_64"), not(feature = "defmt")))]
impl<'a, T: ?Sized> core::fmt::Display for Debug2Format<'a, T> {
    fn fmt(&self, _f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        Ok(())
    }
}
