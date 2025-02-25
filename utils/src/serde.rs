//! Serialization and deserialization of key events

use crate::rgb_anims::RgbAnimType;

use crate::sid::Sid;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Event {
    Noop,
    Ping,
    Retransmit(Sid),        // SidSize
    Ack(Sid),               // SidSize
    Press(u8, u8),          // r: [0, 3], c: [0, 4]: 7 bits
    Release(u8, u8),        // r: [0, 3], c: [0, 4]: 7 bits
    RgbAnim(RgbAnimType),   // 8 bits
    RgbAnimChangeLayer(u8), // 4 bits
    SeedRng(u8),            // 8 bits
}

#[derive(Debug, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Error {
    Serialization,
    Deserialization,
}
pub type Message = u32;

impl Event {
    /// whether the event is a retransmit
    pub fn is_retransmit(&self) -> bool {
        matches!(self, Event::Retransmit(_))
    }

    /// whether the event is an ack
    pub fn is_ack(&self) -> bool {
        matches!(self, Event::Ack(_))
    }

    /// whether the event is needs a ack
    pub fn needs_ack(&self) -> bool {
        !matches!(self, Event::Noop | Event::Ack(_) | Event::Retransmit(_))
    }

    /// Convert the event to a u16
    /// The upper 5 bits are the sequence id
    /// Then are 3 bits for the event type
    /// The lower 8 bits are the event data
    pub fn to_u16(&self, sid: Sid) -> Result<u16, Error> {
        let sid = (sid.as_u16()) << 11;
        let (tag, data) = match self {
            Event::Noop => Ok((0b000, 0x33)),
            Event::Ping => Ok((0b000, 0xcc)),
            Event::Retransmit(err) => Ok((0b001, err.as_u16())),
            Event::Ack(ack) => Ok((0b010, ack.as_u16())),
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
pub fn deserialize(bytes: Message) -> Result<(Event, Sid), Error> {
    let crc = (bytes >> 16) as u16;
    let computed_crc = crc16::State::<crc16::KERMIT>::calculate(&bytes.to_le_bytes()[0..2]);
    if crc != computed_crc {
        return Err(Error::Deserialization);
    }
    let bytes = bytes & 0xffff;
    let sid = Sid::from_u32_lsb(bytes >> 11);
    let tag = (bytes >> 8) & 0b111;
    let data = bytes & 0xff;

    match tag {
        0b000 if data == 0x33 => Ok((Event::Noop, sid)),
        0b000 if data == 0xcc => Ok((Event::Ping, sid)),
        0b001 => Ok((Event::Retransmit(Sid::from_u32_lsb(data)), sid)),
        0b010 => Ok((Event::Ack(Sid::from_u32_lsb(data)), sid)),
        0b011 => Ok((Event::Press((data >> 4) as u8, (data & 0xf) as u8), sid)),
        0b100 => Ok((Event::Release((data >> 4) as u8, (data & 0xf) as u8), sid)),
        0b101 => Ok((Event::RgbAnim(RgbAnimType::from_u8(data as u8)?), sid)),
        0b110 => Ok((Event::RgbAnimChangeLayer(data as u8), sid)),
        0b111 => Ok((Event::SeedRng(data as u8), sid)),
        _ => Err(Error::Deserialization),
    }
}

/// Serialize a key event
pub fn serialize(e: Event, sid: Sid) -> Result<Message, Error> {
    let ser = e.to_u16(sid)?;
    let crc: u16 = crc16::State::<crc16::KERMIT>::calculate(&ser.to_le_bytes());
    let bytes = (ser as u32) | ((crc as u32) << 16);
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log::*;
    use crate::rgb_anims::ERROR_COLOR_INDEX;
    use crate::sid::Sid;

    const VALID_EVENTS: [(Event, Sid); 38] = [
        (Event::Noop, Sid::new(0x0)),
        (Event::Noop, Sid::new(0xa)),
        (Event::Noop, Sid::new(31)),
        (Event::Ping, Sid::new(0x0)),
        (Event::Ping, Sid::new(0xa)),
        (Event::Ping, Sid::new(31)),
        (Event::Retransmit(Sid::new(0)), Sid::new(0)),
        (Event::Retransmit(Sid::new(24)), Sid::new(25)),
        (Event::Retransmit(Sid::new(15)), Sid::new(12)),
        (Event::Ack(Sid::new(0)), Sid::new(0)),
        (Event::Ack(Sid::new(13)), Sid::new(26)),
        (Event::Ack(Sid::new(17)), Sid::new(31)),
        (Event::Press(0, 1), Sid::new(1)),
        (Event::Press(1, 0), Sid::new(2)),
        (Event::Press(1, 9), Sid::new(24)),
        (Event::Release(1, 2), Sid::new(17)),
        (Event::Press(0, 4), Sid::new(12)),
        (Event::Release(3, 9), Sid::new(03)),
        (Event::RgbAnim(RgbAnimType::Off), Sid::new(25)),
        (Event::RgbAnim(RgbAnimType::SolidColor(0)), Sid::new(08)),
        (Event::RgbAnim(RgbAnimType::SolidColor(1)), Sid::new(09)),
        (
            Event::RgbAnim(RgbAnimType::SolidColor(ERROR_COLOR_INDEX)),
            Sid::new(31),
        ),
        (Event::RgbAnim(RgbAnimType::Wheel), Sid::new(07)),
        (Event::RgbAnim(RgbAnimType::Pulse), Sid::new(19)),
        (Event::RgbAnim(RgbAnimType::PulseSolid(0)), Sid::new(24)),
        (Event::RgbAnim(RgbAnimType::PulseSolid(1)), Sid::new(20)),
        (Event::RgbAnim(RgbAnimType::PulseSolid(8)), Sid::new(02)),
        (
            Event::RgbAnim(RgbAnimType::PulseSolid(ERROR_COLOR_INDEX)),
            Sid::new(0),
        ),
        (Event::RgbAnim(RgbAnimType::Input), Sid::new(1)),
        (Event::RgbAnim(RgbAnimType::InputSolid(0)), Sid::new(2)),
        (Event::RgbAnim(RgbAnimType::InputSolid(1)), Sid::new(3)),
        (Event::RgbAnim(RgbAnimType::InputSolid(8)), Sid::new(5)),
        (
            Event::RgbAnim(RgbAnimType::InputSolid(ERROR_COLOR_INDEX)),
            Sid::new(7),
        ),
        (Event::RgbAnimChangeLayer(0), Sid::new(11)),
        (Event::RgbAnimChangeLayer(8), Sid::new(13)),
        (Event::SeedRng(0), Sid::new(17)),
        (Event::SeedRng(8), Sid::new(19)),
        (Event::SeedRng(255), Sid::new(21)),
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

    #[test]
    fn test_bad_crc() {
        for (event, sid) in VALID_EVENTS.iter().copied() {
            let ser = serialize(event, sid).unwrap();
            let mut bytes = ser.to_le_bytes();
            bytes[0] = bytes[0].wrapping_add(1);
            let bad_crc = u32::from_le_bytes(bytes);
            assert_eq!(Err(Error::Deserialization), deserialize(bad_crc));
        }
    }
}
