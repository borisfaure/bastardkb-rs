//! Serialization and deserialization of key events

use crate::log::*;
use crate::rgb_anims::RgbAnimType;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Event {
    Hello,
    Error(u8),              // SidSize
    Ack(u8),                // SidSize
    Press(u8, u8),          // r: [0, 3], c: [0, 4]: 7 bits
    Release(u8, u8),        // r: [0, 3], c: [0, 4]: 7 bits
    RgbAnim(RgbAnimType),   // 8 bits
    RgbAnimChangeLayer(u8), // 4 bits
    SeedRng(u8),            // 8 bits
}

#[derive(Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Error {
    Serialization,
    Deserialization,
}

const SID_MAX: u8 = 31;

impl Event {
    /// whether the event is an error
    pub fn is_error(&self) -> bool {
        matches!(self, Event::Error(_))
    }

    /// Convert the event to a u16
    /// The upper 5 bits are the sequence id
    /// Then are 3 bits for the event type
    /// The lower 8 bits are the event data
    pub fn to_u16(&self, sid: u8) -> Result<u16, Error> {
        if sid > SID_MAX {
            error!("sid must be less than {}", SID_MAX);
            return Err(Error::Serialization);
        }
        let sid = (sid as u16) << 11;
        let (tag, data) = match self {
            Event::Hello => Ok((0b000, 0)),
            Event::Error(err) if *err <= SID_MAX => Ok((0b001, *err as u16)),
            Event::Error(_) => Err(Error::Serialization),
            Event::Ack(ack) if *ack <= SID_MAX => Ok((0b010, *ack as u16)),
            Event::Ack(_) => Err(Error::Serialization),
            Event::Press(r, c) if *r <= 3 && *c <= 9 => {
                Ok((0b011, ((*r as u16) << 4) | (*c as u16)))
            }
            Event::Press(_, _) => Err(Error::Serialization),
            Event::Release(r, c) if *r <= 3 && *c <= 9 => {
                Ok((0b100, ((*r as u16) << 4) | (*c as u16)))
            }
            Event::Release(_, _) => Err(Error::Serialization),
            Event::RgbAnim(anim) => Ok((0b101, anim.to_u8()? as u16)),
            Event::RgbAnimChangeLayer(layer) => Ok((0b110, *layer as u16)),
            Event::SeedRng(seed) => Ok((0b111, *seed as u16)),
        }?;
        Ok(sid | (tag << 8) | data)
    }
}

/// Deserialize a key event from the serial line
pub fn deserialize(bytes: u32) -> Result<(Event, u8), Error> {
    let crc = (bytes >> 16) as u16;
    let computed_crc = crc16::State::<crc16::KERMIT>::calculate(&bytes.to_le_bytes()[0..2]);
    if crc != computed_crc {
        return Err(Error::Deserialization);
    }
    let bytes = bytes & 0xffff;
    let sid = (bytes >> 11) as u8;
    let tag = (bytes >> 8) & 0b111;
    let data = bytes & 0xff;

    match tag {
        0b000 => Ok((Event::Hello, sid)),
        0b001 => Ok((Event::Error(data as u8), sid)),
        0b010 => Ok((Event::Ack(data as u8), sid)),
        0b011 => Ok((Event::Press((data >> 4) as u8, (data & 0xf) as u8), sid)),
        0b100 => Ok((Event::Release((data >> 4) as u8, (data & 0xf) as u8), sid)),
        0b101 => Ok((Event::RgbAnim(RgbAnimType::from_u8(data as u8)?), sid)),
        0b110 => Ok((Event::RgbAnimChangeLayer(data as u8), sid)),
        0b111 => Ok((Event::SeedRng(data as u8), sid)),
        _ => Err(Error::Deserialization),
    }
}

/// Serialize a key event
pub fn serialize(e: Event, sid: u8) -> Result<u32, Error> {
    let ser = e.to_u16(sid)?;
    let crc: u16 = crc16::State::<crc16::KERMIT>::calculate(&ser.to_le_bytes());
    let bytes = (ser as u32) | ((crc as u32) << 16);
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rgb_anims::ERROR_COLOR_INDEX;

    const VALID_EVENTS: [(Event, u8); 33] = [
        (Event::Hello, 0xa),
        (Event::Error(0), 0),
        (Event::Error(24), 25),
        (Event::Error(15), 12),
        (Event::Ack(0), 0),
        (Event::Ack(13), 26),
        (Event::Ack(17), 31),
        (Event::Press(0, 1), 1),
        (Event::Press(1, 0), 2),
        (Event::Press(1, 9), 24),
        (Event::Release(1, 2), 17),
        (Event::Press(0, 4), 12),
        (Event::Release(3, 9), 03),
        (Event::RgbAnim(RgbAnimType::Off), 25),
        (Event::RgbAnim(RgbAnimType::SolidColor(0)), 08),
        (Event::RgbAnim(RgbAnimType::SolidColor(1)), 09),
        (
            Event::RgbAnim(RgbAnimType::SolidColor(ERROR_COLOR_INDEX)),
            31,
        ),
        (Event::RgbAnim(RgbAnimType::Wheel), 07),
        (Event::RgbAnim(RgbAnimType::Pulse), 19),
        (Event::RgbAnim(RgbAnimType::PulseSolid(0)), 24),
        (Event::RgbAnim(RgbAnimType::PulseSolid(1)), 20),
        (Event::RgbAnim(RgbAnimType::PulseSolid(8)), 02),
        (
            Event::RgbAnim(RgbAnimType::PulseSolid(ERROR_COLOR_INDEX)),
            0,
        ),
        (Event::RgbAnim(RgbAnimType::Input), 1),
        (Event::RgbAnim(RgbAnimType::InputSolid(0)), 2),
        (Event::RgbAnim(RgbAnimType::InputSolid(1)), 3),
        (Event::RgbAnim(RgbAnimType::InputSolid(8)), 5),
        (
            Event::RgbAnim(RgbAnimType::InputSolid(ERROR_COLOR_INDEX)),
            7,
        ),
        (Event::RgbAnimChangeLayer(0), 11),
        (Event::RgbAnimChangeLayer(8), 13),
        (Event::SeedRng(0), 17),
        (Event::SeedRng(8), 19),
        (Event::SeedRng(255), 21),
    ];

    #[test]
    fn test_ser_de() {
        for (event, sid) in VALID_EVENTS.iter().copied() {
            debug!("SER: Event: {:?}, sid: {}", event, sid);
            let ser = serialize(event, sid).unwrap();
            let de = deserialize(ser).unwrap();
            debug!("DE: Event: {:?}, sid: {}", de.0, de.1);
            assert_eq!(sid, de.1);
            assert_eq!(event, de.0);
        }
    }
}
