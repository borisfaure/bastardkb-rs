/// Mouse move event
#[derive(Debug, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct MouseMove {
    /// Delta X
    pub dx: i16,
    /// Delta Y
    pub dy: i16,
}

impl MouseMove {
    /// Create a new mouse move event
    pub fn new(dx: i16, dy: i16) -> Self {
        MouseMove { dx, dy }
    }

    /// To u32
    pub fn to_u32(&self) -> u32 {
        ((self.dx as u16 as u32) << 16) | (self.dy as u16 as u32)
    }

    /// From u32
    pub fn from_u32(v: u32) -> Self {
        MouseMove {
            dx: (v >> 16) as i16,
            dy: v as i16,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ser_de() {
        for (dx, dy) in &[
            (0, 0),
            (1, 0),
            (0, 1),
            (1, 1),
            (-1, 0),
            (0, -1),
            (-1, -1),
            (i8::MAX as i16, i8::MAX as i16),
            (i8::MIN as i16, i8::MIN as i16),
            (i16::MAX, i16::MAX),
            (i16::MIN, i16::MIN),
        ] {
            let m = MouseMove::new(*dx, *dy);
            let v = m.to_u32();
            let m2 = MouseMove::from_u32(v);
            assert_eq!(m, m2);
        }
    }
}
