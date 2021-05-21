//! Helpers for parsing stuff by macro
//!
//! The macros in the macro crate assume that this is imported as `parse_helpers`.

use std::convert::TryInto;

use nom::IResult;
use nom::combinator::{map, map_res};
use nom::multi::{fill, length_data};
use nom::number::complete::{le_u16, le_u32, le_u64, le_f32, le_f64};
use nom::sequence::tuple;

pub struct InvalidDiscriminant {
    pub discriminant: usize
}

pub trait Parse where Self: Sized {
    fn parse<'a>(input: &'a [u8]) -> IResult<&'a [u8], Self>;
    fn serialize<O: std::io::Write>(&self, output: &mut O) -> std::io::Result<()>;
}

macro_rules! simple_parse {
    ($t:ty, $parser:expr) => {
        impl Parse for $t {
            fn parse<'a>(input: &'a [u8]) -> IResult<&'a [u8], Self> {
                $parser(input)
            }
        
            fn serialize<O: std::io::Write>(&self, output: &mut O) -> std::io::Result<()> {
                output.write_all(&self.to_le_bytes())
            }
        }
    }
}

simple_parse!(u16, le_u16);
simple_parse!(u32, le_u32);
simple_parse!(u64, le_u64);
simple_parse!(f32, le_f32);
simple_parse!(f64, le_f64);

impl<T: Parse> Parse for vek::Vec3<T> {
    fn parse<'a>(input: &'a [u8]) -> IResult<&'a [u8], Self> {
        let (rest, (x, y, z)) = tuple((<T as Parse>::parse, <T as Parse>::parse, <T as Parse>::parse))(input)?;
        Ok((rest, vek::Vec3 { x, y ,z }))
    }

    fn serialize<O: std::io::Write>(&self, output: &mut O) -> std::io::Result<()> {
        self.x.serialize(output)?;
        self.y.serialize(output)?;
        self.z.serialize(output)?;
        Ok(())
    }
}

impl<T: Parse + Default> Parse for vek::Mat4<T> {
    fn parse<'a>(input: &'a [u8]) -> IResult<&'a [u8], Self> {
        let mut out: [T; 16] = Default::default();
        let (rest, ()) = fill(<T as Parse>::parse, &mut out)(input)?;
        Ok((rest, vek::Mat4::from_col_array(out)))
    }

    fn serialize<O: std::io::Write>(&self, output: &mut O) -> std::io::Result<()> {
        let buf = self.as_col_slice();
        for i in buf {
            i.serialize(output)?;
        }
        Ok(())
    }
}

impl Parse for String {
    fn parse<'a>(input: &'a [u8]) -> IResult<&'a [u8], Self> {
        map_res(length_data(le_u32), |i: &[u8]| -> Result<String, std::str::Utf8Error> {
            let st = std::str::from_utf8(i)?;
            Ok(String::from(st))
        })(input)
    }

    fn serialize<O: std::io::Write>(&self, output: &mut O) -> std::io::Result<()> {
        let len: u32 = self.len().try_into().or_else(|v| Err(std::io::ErrorKind::InvalidInput))?;

        len.serialize(output)?;
        output.write_all(self.as_bytes())
    }
}

impl Parse for crate::hashindex::Hash {
    fn parse<'a>(input: &'a [u8]) -> IResult<&'a [u8], Self> {
        map(le_u64, crate::hashindex::Hash)(input)
    }
    fn serialize<O: std::io::Write>(&self, output: &mut O) -> std::io::Result<()> {
        self.0.serialize(output)
    }
}