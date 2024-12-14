// Maximum sequence id
pub const SID_MAX: Sid = Sid::max();

/// Sequence id
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Sid {
    /// internal value
    v: u8,
}
impl Sid {
    /// Create a new sequence id
    pub const fn new(v: u8) -> Self {
        Self { v }
    }
    /// Get the next sequence id
    pub fn next(&mut self) {
        if self.v == 31 {
            self.v = 0;
        } else {
            self.v += 1;
        }
    }
    /// As usize
    pub fn as_usize(&self) -> usize {
        self.v as usize
    }

    /// As u16
    pub fn as_u16(&self) -> u16 {
        self.v as u16
    }

    /// From u32 lsb
    pub const fn from_u32_lsb(v: u32) -> Self {
        Self { v: v as u8 }
    }

    /// Get the maximum sequence id
    pub const fn max() -> Self {
        Self { v: 31 }
    }
}

// Implement Display for Sid
impl core::fmt::Display for Sid {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_max() {
        assert_eq!(Sid::max(), Sid::new(31));
    }

    #[test]
    fn test_next() {
        let mut sid = Sid::new(0);
        sid.next();
        assert_eq!(sid, Sid::new(1));
        sid.next();
        assert_eq!(sid, Sid::new(2));
        sid.v = 31;
        sid.next();
        assert_eq!(sid, Sid::new(0));
    }
}
