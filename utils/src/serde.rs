//! Serialization and deserialization of key events

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Event {
    Press(u8, u8),
    Release(u8, u8),
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
        _ => Err(Error::Deserialization),
    }
}

/// Serialize a key event
pub fn serialize(e: Event) -> u32 {
    match e {
        Event::Press(i, j) => u32::from_le_bytes([b'P', i, j, b'\n']),
        Event::Release(i, j) => u32::from_le_bytes([b'R', i, j, b'\n']),
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
        ] {
            let ser = serialize(event);
            println!("{:x} == {:x}", ser, serialized);
            assert_eq!(ser, serialized);
            let de = deserialize(ser).unwrap();
            assert_eq!(event, de);
        }
    }
}
