use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use std::{
    convert::TryInto,
    io::{self, Write},
};

pub trait DerSerialize {
    fn serialize<W: Write>(&self, writer: W) -> io::Result<usize>;
}

#[derive(Debug)]
pub enum DerError {
    IntValueTooLarge,
    InvalidEncoding,
    Io(io::Error),
    UnexpectedEof,
    UnexpectedTag,
}

impl From<std::num::TryFromIntError> for DerError {
    fn from(_: std::num::TryFromIntError) -> Self {
        DerError::IntValueTooLarge
    }
}

impl From<io::Error> for DerError {
    fn from(e: io::Error) -> Self {
        if e.kind() == io::ErrorKind::UnexpectedEof {
            DerError::UnexpectedEof
        } else {
            DerError::Io(e)
        }
    }
}

impl From<std::convert::Infallible> for DerError {
    fn from(_: std::convert::Infallible) -> Self {
        unreachable!()
    }
}

pub trait DerDeserialize: Sized {
    fn deserialize(bytes: &mut &[u8]) -> Result<Self, DerError>;
}

#[derive(Clone, Copy, Debug, PartialEq)]
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

impl DerDeserialize for Length {
    fn deserialize(bytes: &mut &[u8]) -> Result<Self, DerError> {
        let first_byte = bytes.read_u8()?;

        if first_byte > 127 {
            let mask_len = (first_byte & 0b0111_1111) as usize;
            let len = bytes.read_uint::<BigEndian>(mask_len)?.try_into()?;

            Ok(Length(len))
        } else {
            Ok(Length(u32::from(first_byte)))
        }
    }
}

pub struct Tag(u8);

impl Tag {
    pub const fn new(tag_number: u8) -> Self {
        Tag(tag_number)
    }

    pub const fn universal(self) -> Self {
        Tag(self.0 & 0b0011_1111)
    }

    pub const fn application(self) -> Self {
        Tag(self.0 & 0b0011_1111 | 0b0100_0000)
    }

    pub const fn context_specific(self) -> Self {
        Tag(self.0 & 0b0011_1111 | 0b1000_0000)
    }

    pub const fn private(self) -> Self {
        Tag(self.0 & 0b0011_1111 | 0b1100_0000)
    }

    pub const fn primitive(self) -> Self {
        Tag(self.0 & 0b1101_1111)
    }

    pub const fn constructed(self) -> Self {
        Tag(self.0 & 0b1101_1111 | 0b0010_0000)
    }

    pub const fn into_tag_value(self) -> u8 {
        self.0
    }
}

pub const BOOLEAN: u8 = Tag::new(1).universal().primitive().into_tag_value();
pub const INTEGER: u8 = Tag::new(2).universal().primitive().into_tag_value();
pub const BIT_STRING: u8 = Tag::new(3).universal().primitive().into_tag_value();
pub const OCTET_STRING: u8 = Tag::new(4).universal().primitive().into_tag_value();
pub const NULL: u8 = Tag::new(5).universal().primitive().into_tag_value();
pub const OBJECT_IDENTIFIER: u8 = Tag::new(6).universal().primitive().into_tag_value();

macro_rules! int_encode {
    ($($t:ty),+) => {$(
        impl DerSerialize for $t {
            fn serialize<W: Write>(&self, mut writer: W) -> io::Result<usize> {
                let mut written = 0;

                writer.write_u8(INTEGER)?;
                written += 1;

                let bytes = self.to_be_bytes();
                let mut start = bytes.iter().take_while(|b| **b == 0).count();
                let mut len = std::mem::size_of::<$t>() - start;

                if len == 0 {
                    len = 1;
                    start = bytes.len() - 1;
                }

                written += Length::new(len as u32).serialize(&mut writer)?;
                writer.write_all(&bytes[start..])?;

                Ok(written + len)
            }
        }
    )+}
}

macro_rules! int_decode {
    ($($t:ty),+) => {$(
        impl DerDeserialize for $t {
            fn deserialize(bytes: &mut &[u8]) -> Result<Self, DerError> {
                let tag = bytes.read_u8()?;

                if tag != INTEGER {
                    return Err(DerError::UnexpectedTag);
                }

                let length = Length::deserialize(bytes)?.into_usize();

                if length < std::mem::size_of::<$t>() {
                    Ok(bytes.read_uint128::<BigEndian>(length)?.try_into()?)
                } else {
                    Ok(bytes.read_int128::<BigEndian>(length)?.try_into()?)
                }
            }
        }
    )+}
}

macro_rules! uint_decode {
    ($($t:ty),+) => {$(
        impl DerDeserialize for $t {
            fn deserialize(bytes: &mut &[u8]) -> Result<Self, DerError> {
                let tag = bytes.read_u8()?;

                if tag != INTEGER {
                    return Err(DerError::UnexpectedTag);
                }

                let length = Length::deserialize(bytes)?.into_usize();

                Ok(bytes.read_uint128::<BigEndian>(length)?.try_into()?)
            }
        }
    )+}
}

int_encode!(u8, i8, u16, i16, u32, i32, u64, i64, u128, i128);
int_decode!(i8, i16, i32, i64, i128);
uint_decode!(u8, u16, u32, u64, u128);

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
        // Length serialization
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

        // Length deserialization
        let slice = [0x01];
        assert_eq!(Length::deserialize(&mut &slice[..]).unwrap(), Length(1));

        let slice = [0x82, 0xAB, 0xCD];
        assert_eq!(
            Length::deserialize(&mut &slice[..]).unwrap(),
            Length(0xABCD)
        );

        let slice = [0x83, 0xAB, 0xCD, 0xEF];
        assert_eq!(
            Length::deserialize(&mut &slice[..]).unwrap(),
            Length(0xAB_CDEF)
        );

        let slice = [0x84, 0xAB, 0xCD, 0xEF, 0x57];
        assert_eq!(
            Length::deserialize(&mut &slice[..]).unwrap(),
            Length(0xABCD_EF57)
        );

        let slice = [0x85, 0xAB, 0xCD, 0xEF, 0x88, 0x99];
        assert!(Length::deserialize(&mut &slice[..]).is_err());
    }

    macro_rules! integer_enc_dec {
        ($($t:ty),+) => {
            #[test]
            fn integer_enc_dec() {
                $({
                    println!("testing {}", std::any::type_name::<$t>());
                    let mut buffer = vec![];
                    for _ in 0..100 {
                        let i: $t = rand::random();
                        let len = i.serialize(&mut buffer).unwrap();
                        assert_eq!(<$t>::deserialize(&mut &buffer[..len]).unwrap(), i);
                        buffer.clear();
                    }
                })+
            }
        }
    }

    integer_enc_dec!(u8, i8, u16, i16, u32, i32, u64, i64, u128, i128);
}
