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

    /// Iterator for sequence id
    /// If @end is reached, the iterator will return None
    pub fn iter(&self, end: Sid) -> SidIter {
        SidIter {
            sid: *self,
            end,
            eof: false,
        }
    }
}

/// Iterator for sequence id
pub struct SidIter {
    /// Current sequence id
    sid: Sid,
    /// End sequence id (inclusive)
    end: Sid,
    /// Whether the end has been reached
    eof: bool,
}
impl core::iter::Iterator for SidIter {
    type Item = Sid;

    fn next(&mut self) -> Option<Sid> {
        if self.eof {
            return None;
        }
        let r = Some(self.sid);
        self.sid.next();
        if self.sid == self.end {
            self.eof = true;
        }
        r
    }
}

impl PartialOrd for Sid {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.v.cmp(&other.v))
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

    /// Get the value at the sequence id
    /// Does not remove it
    pub fn get(&self, sid: Sid) -> Option<T> {
        self.arr[sid.as_usize()]
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
    fn test_sid_iter() {
        let sid = Sid::new(3);
        let mut iter = sid.iter(Sid::new(6));
        assert_eq!(iter.next(), Some(Sid::new(3)));
        assert_eq!(iter.next(), Some(Sid::new(4)));
        assert_eq!(iter.next(), Some(Sid::new(5)));
        assert_eq!(iter.next(), None);
        let sid = Sid::new(30);
        let mut iter = sid.iter(Sid::new(1));
        assert_eq!(iter.next(), Some(Sid::new(30)));
        assert_eq!(iter.next(), Some(Sid::new(31)));
        assert_eq!(iter.next(), Some(Sid::new(0)));
        assert_eq!(iter.next(), None);

        let sid = Sid::new(17);
        let mut iter = sid.iter(Sid::new(17));
        let mut count = 0;
        while let Some(_) = iter.next() {
            count += 1;
        }
        assert_eq!(count, SID_MAX_U8 as usize + 1);

        let sid = Sid::new(17);
        let mut iter = sid.iter(Sid::new(18));
        let mut count = 0;
        while let Some(_) = iter.next() {
            count += 1;
        }
        assert_eq!(count, 1);
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
        assert_eq!(buf.get(Sid::new(0)), Some(1));
        assert_eq!(buf.get(Sid::new(1)), Some(2));
        assert_eq!(buf.get(Sid::new(2)), Some(3));
        assert_eq!(buf.take(Sid::new(24)), None);
        assert_eq!(buf.take(Sid::new(1)), Some(2));
        assert_eq!(buf.take(Sid::new(1)), None);
        assert_eq!(buf.take(Sid::new(0)), Some(1));
        assert_eq!(buf.take(Sid::new(0)), None);
        assert_eq!(buf.take(Sid::new(2)), Some(3));
        assert_eq!(buf.take(Sid::new(2)), None);
        assert!(buf.is_empty());
    }
}
