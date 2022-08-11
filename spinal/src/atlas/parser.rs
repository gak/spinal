use crate::atlas::{Header, Page};
use crate::{Atlas, SpinalError};
use nom::branch::alt;
use nom::bytes::complete::{tag, take_till, take_while, take_while1};
use nom::character::complete::{alphanumeric1, multispace0, newline, one_of, space0};
use nom::error::{ErrorKind, ParseError};
use nom::multi::{many0, many1};
use nom::sequence::{delimited, preceded, separated_pair, terminated};
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
    // Remove any initial whitespace or new lines.
    let (s, _) = multispace0(s)?;
    let (s, pages) = many1(page)(s)?;
    Ok((s, Atlas { pages }))
}

fn page(s: &str) -> IResult<&str, Page> {
    dbg!("page header", s);
    let (s, header) = header(s)?;
    dbg!("page done", header, s);
    // // let (b, regions) = many0(region)(b)?;
    // let regions = vec![];
    // Ok((b, Page { header, regions }))
    todo!()
}

fn header(s: &str) -> IResult<&str, Header> {
    let (s, name) = title(s)?;
    let name = if let Entry::Title(name) = name {
        name
    } else {
        panic!("Not a title: {:?}", name);
    };
    
    dbg!(name);
    dbg!(s);
    let (s, entries) = many1(kv)(s)?;
    dbg!(entries);
    let mut header = Header{
        name: name.1.to_string(),
        ..Default::default()
    }
    for entry in entries {
        dbg!(entry);


    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum Entry<'a> {
    KeyValue((&'a str, &'a str)),
    Title(&'a str),
    EmptyLine,
}

/// A parser that either returns a key value pair or if the line does not have a semicolon,
/// just return the enum of the string.
fn entry(s: &str) -> IResult<&str, Entry> {
    // Leading whitespace.
    let (s, _) = take_while(is_whitespace)(s)?;

    alt((empty_line, kv, title))(s)
}

fn empty_line(s: &str) -> IResult<&str, Entry> {
    let (s, _) = newline(s)?;
    Ok((s, Entry::EmptyLine))
}

fn title(s: &str) -> IResult<&str, Entry> {
    let (s, title) = terminated(symbols, tag("\n"))(s)?;
    Ok((s, Entry::Title(title)))
}

fn kv(s: &str) -> IResult<&str, Entry> {
    let (s, (key, value)) = separated_pair(symbols, colon_separator, symbols)(s)?;
    let (s, _) = tag("\n")(s)?;
    Ok((s, Entry::KeyValue((key, value))))
}

fn symbols(s: &str) -> IResult<&str, &str> {
    take_while1(is_not_a_separator)(s)
}

fn is_not_a_separator(c: char) -> bool {
    !is_colon(c) && !is_newline(c)
}

fn colon_separator(s: &str) -> IResult<&str, ()> {
    let (s, _) = space0(s)?;
    let (s, _) = tag(":")(s)?;
    let (s, _) = space0(s)?;
    Ok((s, ()))
}

fn is_colon(c: char) -> bool {
    c == ':'
}

fn is_newline(c: char) -> bool {
    c == '\n'
}

fn is_whitespace(c: char) -> bool {
    c == ' ' || c == '\t'
}

fn ws<'a, F: 'a, O, E: ParseError<&'a str>>(
    inner: F,
) -> impl FnMut(&'a str) -> IResult<&'a str, O, E>
where
    F: Fn(&'a str) -> IResult<&'a str, O, E>,
{
    delimited(space0, inner, newline)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries() {
        let tests = vec![
            ("  \n", Entry::EmptyLine),
            ("  \n", Entry::EmptyLine),
            ("  title\n", Entry::Title("title")),
            ("k: v\n", Entry::KeyValue(("k", "v"))),
            ("   size: 640, 480\n", Entry::KeyValue(("size", "640, 480"))),
        ];
        for (inp, expected) in tests {
            println!("{:?}, {:?}", inp, expected);
            let (s, actual) = entry(inp).unwrap();
            assert_eq!(actual, expected, "{}", inp);
            assert_eq!(s, "");
        }
    }

    #[test]
    fn full() {
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
