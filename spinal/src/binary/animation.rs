use nom::IResult;
use crate::binary::BinaryParser;

impl BinaryParser {
    pub fn animation(&self) -> impl FnMut(&[u8]) -> IResult<&[u8], Animation> {
        todo!()
    }
}
