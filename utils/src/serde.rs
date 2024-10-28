//! Serialization and deserialization of key events

use crate::rgb_anims::RgbAnimType;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Event {
    Press(u8, u8),
    Release(u8, u8),
    RgbAnim(RgbAnimType),
    RgbAnimChangeLayer(u8),
}

#[derive(Debug)]
pub enum Error {
    Deserialization,
}

/// Deserialize a key event from the serial line
pub fn deserialize(bytes: u32) -> Result<Event, Error> {
    match bytes.to_le_bytes() {
        [b'P', i, j, b'\n'] => Ok(Event::Press(i, j)),
        [b'R', i, j, b'\n'] => Ok(Event::Release(i, j)),
        [b'L', b'o', b'f', b'\n'] => Ok(Event::RgbAnim(RgbAnimType::Off)),
        [b'L', b'L', i, b'\n'] => Ok(Event::RgbAnim(RgbAnimType::SolidColor(i))),
        [b'L', b'W', b'h', b'\n'] => Ok(Event::RgbAnim(RgbAnimType::Wheel)),
        [b'L', b'P', b'u', b'\n'] => Ok(Event::RgbAnim(RgbAnimType::Pulse)),
        [b'L', b'p', i, b'\n'] => Ok(Event::RgbAnim(RgbAnimType::PulseSolid(i))),
        [b'L', b'I', b'n', b'\n'] => Ok(Event::RgbAnim(RgbAnimType::Input)),
        [b'L', b'i', i, b'\n'] => Ok(Event::RgbAnim(RgbAnimType::InputSolid(i))),
        [b'L', b'C', i, b'\n'] => Ok(Event::RgbAnimChangeLayer(i)),
        _ => Err(Error::Deserialization),
    }
}

/// Serialize a key event
pub fn serialize(e: Event) -> u32 {
    match e {
        Event::Press(i, j) => u32::from_le_bytes([b'P', i, j, b'\n']),
        Event::Release(i, j) => u32::from_le_bytes([b'R', i, j, b'\n']),
        Event::RgbAnim(RgbAnimType::Off) => u32::from_le_bytes([b'L', b'o', b'f', b'\n']),
        Event::RgbAnim(RgbAnimType::SolidColor(i)) => u32::from_le_bytes([b'L', b'L', i, b'\n']),
        Event::RgbAnim(RgbAnimType::Wheel) => u32::from_le_bytes([b'L', b'W', b'h', b'\n']),
        Event::RgbAnim(RgbAnimType::Pulse) => u32::from_le_bytes([b'L', b'P', b'u', b'\n']),
        Event::RgbAnim(RgbAnimType::PulseSolid(i)) => u32::from_le_bytes([b'L', b'p', i, b'\n']),
        Event::RgbAnim(RgbAnimType::Input) => u32::from_le_bytes([b'L', b'I', b'n', b'\n']),
        Event::RgbAnim(RgbAnimType::InputSolid(i)) => u32::from_le_bytes([b'L', b'i', i, b'\n']),
        Event::RgbAnimChangeLayer(i) => u32::from_le_bytes([b'L', b'C', i, b'\n']),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ser_de() {
        for (event, serialized) in [
            (Event::Press(1, 2), 0x0a02_0150),
            (Event::Release(1, 2), 0x0a02_0152),
            (Event::Press(0, 255), 0x0aff_0050),
            (Event::Release(255, 0), 0x0a00_ff52),
            (Event::RgbAnim(RgbAnimType::Off), 0x0a66_6f4c),
            (Event::RgbAnim(RgbAnimType::SolidColor(0)), 0x0a00_4c4c),
            (Event::RgbAnim(RgbAnimType::SolidColor(1)), 0x0a01_4c4c),
            (Event::RgbAnim(RgbAnimType::SolidColor(8)), 0x0a08_4c4c),
            (Event::RgbAnim(RgbAnimType::Wheel), 0x0a68_574c),
            (Event::RgbAnim(RgbAnimType::Pulse), 0x0a75_504c),
            (Event::RgbAnim(RgbAnimType::PulseSolid(0)), 0x0a00_704c),
            (Event::RgbAnim(RgbAnimType::PulseSolid(1)), 0x0a01_704c),
            (Event::RgbAnim(RgbAnimType::PulseSolid(8)), 0x0a08_704c),
            (Event::RgbAnim(RgbAnimType::PulseSolid(255)), 0x0aff_704c),
            (Event::RgbAnim(RgbAnimType::Input), 0x0a6e_494c),
            (Event::RgbAnim(RgbAnimType::InputSolid(0)), 0x0a00_694c),
            (Event::RgbAnim(RgbAnimType::InputSolid(1)), 0x0a01_694c),
            (Event::RgbAnim(RgbAnimType::InputSolid(8)), 0x0a08_694c),
            (Event::RgbAnim(RgbAnimType::InputSolid(255)), 0x0aff_694c),
            (Event::RgbAnimChangeLayer(0), 0x0a00_434c),
            (Event::RgbAnimChangeLayer(8), 0x0a08_434c),
        ] {
            let ser = serialize(event);
            println!("{:x} == {:x}", ser, serialized);
            assert_eq!(ser, serialized);
            let de = deserialize(ser).unwrap();
            assert_eq!(event, de);
        }
    }
}
