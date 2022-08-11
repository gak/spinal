use crate::atlas::{Header, Page};
use crate::{Atlas, SpinalError};
use nom::branch::alt;
use nom::bytes::complete::{tag, take_till, take_while, take_while1};
use nom::character::complete::{alphanumeric1, multispace0, newline, one_of, space0};
use nom::character::is_space;
use nom::error::ParseError;
use nom::multi::{many0, many1};
use nom::sequence::{delimited, preceded};
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
    dbg!("multispace");
    let (s, _) = multispace0(s)?;
    dbg!("many1");
    let (s, pages) = many1(page)(s)?;
    Ok((s, Atlas { pages }))
}

fn page(s: &str) -> IResult<&str, Page> {
    dbg!("page header");
    let (b, header) = header(s)?;
    dbg!("page done");
    // let (b, regions) = many0(region)(b)?;
    let regions = vec![];
    Ok((b, Page { header, regions }))
}

fn header(s: &str) -> IResult<&str, Header> {
    dbg!("header");
    let (s, file) = take_discard(is_newline)(s)?;
    dbg!(file);
    let (s, kvs) = many1(kv)(s)?;
    dbg!(kvs);
    dbg!("done");

    todo!()
}

fn kv(s: &str) -> IResult<&str, (&str, &str)> {
    let (s, _) = take_while(|c| c == ' ')(s)?;
    let (s, key) = take_discard(is_colon)(s)?;
    dbg!(key);
    let (s, _) = space0(s)?;
    let (s, value) = take_discard(is_newline)(s)?;
    dbg!(value);
    // let (s, value) = take_till(is_newline)(s)?;
    // let (s, _) = take_while(is_newline)(s)?;
    Ok((s, (key, value)))
}

/// Take until X and discard X.
fn take_discard<'a, F>(f: F) -> impl FnMut(&'a str) -> IResult<&'a str, &'a str>
where
    F: Fn(char) -> bool + Copy,
{
    move |s: &str| {
        let (s, captured) = take_till(f)(s)?;
        let (s, _) = take_while(f)(s)?;
        Ok((s, captured))
    }
}

fn is_colon(c: char) -> bool {
    c == ':'
}

fn line(s: &str) -> IResult<&str, &str> {
    ws(take_till(is_newline))(s)
}

fn is_newline(c: char) -> bool {
    c == '\n' || c == '\r'
}

fn ws<'a, F: 'a, O, E: ParseError<&'a str>>(
    inner: F,
) -> impl FnMut(&'a str) -> IResult<&'a str, O, E>
where
    F: Fn(&'a str) -> IResult<&'a str, O, E>,
{
    delimited(space0, inner, newline)
}

// fn ws<'a, F, O, E>(inner: F) -> impl FnMut(&'a str) -> IResult<&'a str, O, E>
// where
//     F: Fn(&'a str) -> IResult<&'a str, O, E>,
//     E: ParseError<&'a str>,
// {
//     delimited(space0, inner, newline)
// }

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
