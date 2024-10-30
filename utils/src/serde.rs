//! Serialization and deserialization of key events

use crate::rgb_anims::RgbAnimType;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Event {
    Hello,
    Error(u8),
    Ack(u8),
    Press(u8, u8),
    Release(u8, u8),
    RgbAnim(RgbAnimType),
    RgbAnimChangeLayer(u8),
}

impl Event {
    /// whether the event is an error
    pub fn is_error(&self) -> bool {
        matches!(self, Event::Error(_))
    }
}

#[derive(Debug)]
pub enum Error {
    Deserialization,
}

/// Deserialize a key event from the serial line
pub fn deserialize(bytes: u32) -> Result<(Event, u8), Error> {
    match bytes.to_le_bytes() {
        [b'H', b'i', b'!', sid] => Ok((Event::Hello, sid)),
        [b'E', b'r', err, sid] => Ok((Event::Error(err), sid)),
        [b'A', b'c', ack, sid] => Ok((Event::Ack(ack), sid)),
        [b'P', i, j, sid] => Ok((Event::Press(i, j), sid)),
        [b'R', i, j, sid] => Ok((Event::Release(i, j), sid)),
        [b'L', b'o', b'f', sid] => Ok((Event::RgbAnim(RgbAnimType::Off), sid)),
        [b'L', b'L', i, sid] => Ok((Event::RgbAnim(RgbAnimType::SolidColor(i)), sid)),
        [b'L', b'W', b'h', sid] => Ok((Event::RgbAnim(RgbAnimType::Wheel), sid)),
        [b'L', b'P', b'u', sid] => Ok((Event::RgbAnim(RgbAnimType::Pulse), sid)),
        [b'L', b'p', i, sid] => Ok((Event::RgbAnim(RgbAnimType::PulseSolid(i)), sid)),
        [b'L', b'I', b'n', sid] => Ok((Event::RgbAnim(RgbAnimType::Input), sid)),
        [b'L', b'i', i, sid] => Ok((Event::RgbAnim(RgbAnimType::InputSolid(i)), sid)),
        [b'L', b'C', i, sid] => Ok((Event::RgbAnimChangeLayer(i), sid)),
        _ => Err(Error::Deserialization),
    }
}

/// Serialize a key event
pub fn serialize(e: Event, sid: u8) -> u32 {
    match e {
        Event::Hello => u32::from_le_bytes([b'H', b'i', b'!', sid]),
        Event::Error(err) => u32::from_le_bytes([b'E', b'r', err, sid]),
        Event::Ack(ack) => u32::from_le_bytes([b'A', b'c', ack, sid]),
        Event::Press(i, j) => u32::from_le_bytes([b'P', i, j, sid]),
        Event::Release(i, j) => u32::from_le_bytes([b'R', i, j, sid]),
        Event::RgbAnim(RgbAnimType::Off) => u32::from_le_bytes([b'L', b'o', b'f', sid]),
        Event::RgbAnim(RgbAnimType::SolidColor(i)) => u32::from_le_bytes([b'L', b'L', i, sid]),
        Event::RgbAnim(RgbAnimType::Wheel) => u32::from_le_bytes([b'L', b'W', b'h', sid]),
        Event::RgbAnim(RgbAnimType::Pulse) => u32::from_le_bytes([b'L', b'P', b'u', sid]),
        Event::RgbAnim(RgbAnimType::PulseSolid(i)) => u32::from_le_bytes([b'L', b'p', i, sid]),
        Event::RgbAnim(RgbAnimType::Input) => u32::from_le_bytes([b'L', b'I', b'n', sid]),
        Event::RgbAnim(RgbAnimType::InputSolid(i)) => u32::from_le_bytes([b'L', b'i', i, sid]),
        Event::RgbAnimChangeLayer(i) => u32::from_le_bytes([b'L', b'C', i, sid]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ser_de() {
        for ((event, sid), serialized) in [
            ((Event::Hello, 0xa), 0x0a21_6948),
            ((Event::Error(0), 0), 0x0000_7245),
            ((Event::Error(42), 251), 0xfb2a_7245),
            ((Event::Error(251), 42), 0x2afb_7245),
            ((Event::Ack(0), 0), 0x0000_6341),
            ((Event::Ack(193), 63), 0x3fc1_6341),
            ((Event::Ack(63), 193), 0xc13f_6341),
            ((Event::Press(0, 1), 1), 0x0101_0050),
            ((Event::Press(1, 0), 2), 0x0200_0150),
            ((Event::Press(1, 2), 124), 0x7c02_0150),
            ((Event::Release(1, 2), 63), 0x3f02_0152),
            ((Event::Press(0, 255), 12), 0x0cff_0050),
            ((Event::Release(255, 0), 34), 0x2200_ff52),
            ((Event::RgbAnim(RgbAnimType::Off), 56), 0x3866_6f4c),
            (
                (Event::RgbAnim(RgbAnimType::SolidColor(0)), 78),
                0x4e00_4c4c,
            ),
            (
                (Event::RgbAnim(RgbAnimType::SolidColor(1)), 90),
                0x5a01_4c4c,
            ),
            (
                (Event::RgbAnim(RgbAnimType::SolidColor(8)), 13),
                0x0d08_4c4c,
            ),
            ((Event::RgbAnim(RgbAnimType::Wheel), 57), 0x3968_574c),
            ((Event::RgbAnim(RgbAnimType::Pulse), 91), 0x5b75_504c),
            (
                (Event::RgbAnim(RgbAnimType::PulseSolid(0)), 24),
                0x1800_704c,
            ),
            (
                (Event::RgbAnim(RgbAnimType::PulseSolid(1)), 68),
                0x4401_704c,
            ),
            (
                (Event::RgbAnim(RgbAnimType::PulseSolid(8)), 02),
                0x0208_704c,
            ),
            (
                (Event::RgbAnim(RgbAnimType::PulseSolid(255)), 0),
                0x00ff_704c,
            ),
            ((Event::RgbAnim(RgbAnimType::Input), 1), 0x016e_494c),
            ((Event::RgbAnim(RgbAnimType::InputSolid(0)), 2), 0x0200_694c),
            ((Event::RgbAnim(RgbAnimType::InputSolid(1)), 3), 0x0301_694c),
            ((Event::RgbAnim(RgbAnimType::InputSolid(8)), 5), 0x0508_694c),
            (
                (Event::RgbAnim(RgbAnimType::InputSolid(255)), 7),
                0x07ff_694c,
            ),
            ((Event::RgbAnimChangeLayer(0), 11), 0x0b00_434c),
            ((Event::RgbAnimChangeLayer(8), 13), 0x0d08_434c),
        ] {
            let ser = serialize(event, sid);
            println!("{:x} == {:x}", ser, serialized);
            assert_eq!(ser, serialized);
            let de = deserialize(ser).unwrap();
            assert_eq!(sid, de.1);
            assert_eq!(event, de.0);
        }
    }
}
