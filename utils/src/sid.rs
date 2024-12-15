// Maximum sequence id
pub const SID_MAX: Sid = Sid::max();

/// Maximum sequence id as u8
const SID_MAX_U8: u8 = 31;

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
        if self.v == SID_MAX_U8 {
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
        Self { v: SID_MAX_U8 }
    }
}

// Implement Display for Sid
impl core::fmt::Display for Sid {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.v)
    }
}

/// Circular buffer to store values by sequence id
#[derive(Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct CircBuf<T> {
    /// Array of values
    arr: [Option<T>; (SID_MAX_U8 + 1) as usize],
    /// Number of elements
    count: usize,
}

impl<T: Copy> CircBuf<T> {
    /// Create a new circular buffer
    pub fn new() -> Self {
        Self {
            arr: [None; (SID_MAX_U8 + 1) as usize],
            count: 0,
        }
    }

    /// Get the value at the sequence id, and remove it
    pub fn take(&mut self, sid: Sid) -> Option<T> {
        let pos = sid.as_usize();
        let v = self.arr[pos];
        if v.is_some() {
            self.count -= 1;
        }
        self.arr[pos] = None;
        v
    }

    /// Insert a value at the sequence id
    pub fn insert(&mut self, sid: Sid, val: T) {
        let pos = sid.as_usize();
        if self.arr[pos].is_none() {
            self.count += 1;
        }
        self.arr[pos] = Some(val);
    }

    /// Remove a value at the sequence id
    pub fn remove(&mut self, sid: Sid) {
        self.take(sid);
    }

    /// Whether the container is empty
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
}

impl<T: Copy> Default for CircBuf<T> {
    fn default() -> Self {
        Self::new()
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

    #[test]
    fn test_circ_buf() {
        let mut buf = CircBuf::new();
        assert!(buf.is_empty());
        buf.insert(Sid::new(0), 1);
        assert!(!buf.is_empty());
        assert_eq!(buf.take(Sid::new(0)), Some(1));
        assert!(buf.is_empty());
        buf.insert(Sid::new(0), 1);
        buf.insert(Sid::new(1), 2);
        buf.insert(Sid::new(2), 3);
        assert_eq!(buf.take(Sid::new(1)), Some(2));
        assert_eq!(buf.take(Sid::new(1)), None);
        assert_eq!(buf.take(Sid::new(0)), Some(1));
        assert_eq!(buf.take(Sid::new(0)), None);
        assert_eq!(buf.take(Sid::new(2)), Some(3));
        assert_eq!(buf.take(Sid::new(2)), None);
        assert!(buf.is_empty());
    }
}
