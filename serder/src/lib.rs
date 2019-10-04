use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use std::io::{self, Write};

pub trait DerSerialize {
    fn serialize<W: Write>(&self, writer: W) -> io::Result<usize>;
}

pub enum DerErrorKind {
    InvalidEncoding,
    UnexpectedTag,
}

pub struct DerError {
    pub pos: usize,
    pub kind: DerErrorKind,
}

pub trait DerDeserialize: Sized {
    fn deserialize(bytes: &[u8]) -> Result<Self, DerError>;
}

#[derive(Clone, Copy, Debug)]
pub struct Length(u32);

impl Length {
    pub fn new(n: u32) -> Self {
        assert!(n > 0);

        Length(n)
    }

    pub const fn value(self) -> u32 {
        self.0
    }

    pub fn into_usize(self) -> usize {
        self.0 as usize
    }
}

impl DerSerialize for Length {
    fn serialize<W: Write>(&self, mut writer: W) -> io::Result<usize> {
        let written = if self.0 > 127 {
            // Long form, use the smallest encoding for the value
            //
            let bytes = self.0.to_be_bytes();
            let start = bytes.iter().take_while(|b| **b == 0).count();
            let len = 4 - start;

            writer.write_u8(0x80 | len as u8)?;
            writer.write_all(&bytes[start..])?;

            len + 1
        } else {
            // Short form
            writer.write_u8(self.0 as u8)?;

            1
        };

        Ok(written)
    }
}

// macro_rules! int_

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! zero {
        ($n:ident) => {
            for i in $n.iter_mut() {
                *i = 0;
            }
        };
    }

    #[test]
    fn length_encoding() {
        let mut slice = [0u8; 5];

        assert_eq!(Length::new(1).serialize(&mut slice[..]).unwrap(), 1);
        assert_eq!(slice, [1, 0, 0, 0, 0]);
        zero!(slice);

        assert_eq!(Length::new(128).serialize(&mut slice[..]).unwrap(), 2);
        assert_eq!(slice, [0x81, 0x80, 0x00, 0x00, 0x00]);
        zero!(slice);

        assert_eq!(
            Length::new(0x0000_A5B5).serialize(&mut slice[..]).unwrap(),
            3
        );
        assert_eq!(slice, [0x82, 0xA5, 0xB5, 0x00, 0x00]);
        zero!(slice);

        assert_eq!(
            Length::new(0x00A5_B5C5).serialize(&mut slice[..]).unwrap(),
            4
        );
        assert_eq!(slice, [0x83, 0xA5, 0xB5, 0xC5, 0x00]);
        zero!(slice);

        assert_eq!(
            Length::new(0xA5B5_C5D5).serialize(&mut slice[..]).unwrap(),
            5
        );
        assert_eq!(slice, [0x84, 0xA5, 0xB5, 0xC5, 0xD5]);
        zero!(slice);
    }
}
