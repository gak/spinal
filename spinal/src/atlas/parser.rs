use crate::atlas::{Header, Page};
use crate::{Atlas, SpinalError};
use nom::bytes::complete::take_till;
use nom::character::complete::{multispace0, newline, one_of, space0};
use nom::character::{is_newline, is_space};
use nom::error::ParseError;
use nom::multi::{many0, many1};
use nom::sequence::delimited;
use nom::IResult;

/// Parses a Spine atlas file.
///
/// See http://esotericsoftware.com/spine-atlas-format
pub struct AtlasParser;

impl AtlasParser {
    pub fn parse(s: &str) -> Result<Atlas, SpinalError> {
        let (_, atlas) = parser(s).unwrap();
        Ok(atlas)
    }
}

fn parser(s: &str) -> IResult<&str, Atlas> {
    // Remove any whitespace or new lines.
    let (s, _) = multispace0(s)?;
    let (s, pages) = many1(page)(s)?;
    Ok((s, Atlas { pages }))
}

fn page(s: &str) -> IResult<&str, Page> {
    let (b, header) = header(s)?;
    // let (b, regions) = many0(region)(b)?;
    let regions = vec![];
    Ok((b, Page { header, regions }))
}

fn header(s: &str) -> IResult<&str, Header> {
    let (s, _) = ws(take_till(is_newline))(s)?;
    todo!()
}

fn ws<'a, F, O, E>(inner: F) -> impl FnMut(&'a str) -> IResult<&'a str, O, E>
where
    F: Fn(&'a str) -> IResult<&'a str, O, E>,
    E: ParseError<&'a str>,
{
    delimited(space0, inner, newline)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header() {
        let s = r#"
page1.png
   size: 640, 480
   format: RGBA8888
   filter: Linear, Linear
   repeat: none
   pma: true
dagger
   bounds: 372, 100, 26, 108
head
   index: 0
   bounds: 2, 21, 103, 81
   rotate: 90

page2.png
   size: 640, 480
   format: RGB565
   filter: Nearest, Nearest
   repeat: x
bg-dialog
   index: -1
   rotate: false
   bounds: 519, 223, 17, 38
   offsets: 2, 2, 21, 42
   split: 10, 10, 29, 10
   pad: -1, -1, 28, 10
   
       "#;
        let atlas = AtlasParser::parse(s).unwrap();
        dbg!(atlas);
    }
}
