//! Serialization and deserialization of key events

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Event {
    Press(u8, u8),
    Release(u8, u8),
    RgbOff,
    RgbLayer(u8), // Layer index
    RgbSnake,
    RgbPulse,
    RgbPulseSingle(u8), // Color index
    RgbInput,
    RgbInputSingle(u8), // Color index
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
        [b'L', b'o', b'f', b'\n'] => Ok(Event::RgbOff),
        [b'L', b'L', i, b'\n'] => Ok(Event::RgbLayer(i)),
        [b'L', b'S', b'n', b'\n'] => Ok(Event::RgbSnake),
        [b'L', b'P', b'u', b'\n'] => Ok(Event::RgbPulse),
        [b'L', b'p', i, b'\n'] => Ok(Event::RgbPulseSingle(i)),
        [b'L', b'I', b'n', b'\n'] => Ok(Event::RgbInput),
        [b'L', b'i', i, b'\n'] => Ok(Event::RgbInputSingle(i)),
        _ => Err(Error::Deserialization),
    }
}

/// Serialize a key event
pub fn serialize(e: Event) -> u32 {
    match e {
        Event::Press(i, j) => u32::from_le_bytes([b'P', i, j, b'\n']),
        Event::Release(i, j) => u32::from_le_bytes([b'R', i, j, b'\n']),
        Event::RgbOff => u32::from_le_bytes([b'L', b'o', b'f', b'\n']),
        Event::RgbLayer(i) => u32::from_le_bytes([b'L', b'L', i, b'\n']),
        Event::RgbSnake => u32::from_le_bytes([b'L', b'S', b'n', b'\n']),
        Event::RgbPulse => u32::from_le_bytes([b'L', b'P', b'u', b'\n']),
        Event::RgbPulseSingle(i) => u32::from_le_bytes([b'L', b'p', i, b'\n']),
        Event::RgbInput => u32::from_le_bytes([b'L', b'I', b'n', b'\n']),
        Event::RgbInputSingle(i) => u32::from_le_bytes([b'L', b'i', i, b'\n']),
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
            (Event::RgbOff, 0x0a66_6f4c),
            (Event::RgbLayer(0), 0x0a00_4c4c),
            (Event::RgbLayer(1), 0x0a01_4c4c),
            (Event::RgbLayer(8), 0x0a08_4c4c),
            (Event::RgbSnake, 0x0a6e_534c),
            (Event::RgbPulse, 0x0a75_504c),
            (Event::RgbPulseSingle(0), 0x0a00_704c),
            (Event::RgbPulseSingle(1), 0x0a01_704c),
            (Event::RgbPulseSingle(8), 0x0a08_704c),
            (Event::RgbPulseSingle(255), 0x0aff_704c),
            (Event::RgbInput, 0x0a6e_494c),
            (Event::RgbInputSingle(0), 0x0a00_694c),
            (Event::RgbInputSingle(1), 0x0a01_694c),
            (Event::RgbInputSingle(8), 0x0a08_694c),
            (Event::RgbInputSingle(255), 0x0aff_694c),
        ] {
            let ser = serialize(event);
            println!("{:x} == {:x}", ser, serialized);
            assert_eq!(ser, serialized);
            let de = deserialize(ser).unwrap();
            assert_eq!(event, de);
        }
    }
}
