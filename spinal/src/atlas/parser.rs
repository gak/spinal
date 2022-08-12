use crate::atlas::{Bounds, Header, Page, Region};
use crate::{Atlas, SpinalError};
use bevy_math::Vec2;
use bevy_utils::HashMap;
use nom::branch::alt;
use nom::bytes::complete::{tag, take_till, take_while, take_while1};
use nom::character::complete::{alphanumeric1, multispace0, newline, one_of, space0};
use nom::combinator::eof;
use nom::error::{ErrorKind, ParseError};
use nom::multi::{many0, many1};
use nom::sequence::{delimited, preceded, separated_pair, terminated, tuple};
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
    let (s, first_page) = page(s)?;
    let mut pages = vec![first_page];
    let (s, more_pages) = many0(preceded(newline, page))(s)?;
    pages.extend(more_pages);
    Ok((s, Atlas { pages }))
}

fn page(s: &str) -> IResult<&str, Page> {
    dbg!("page header", s);
    let (s, header) = header(s)?;
    dbg!("page done", &header, s);
    let (s, regions) = many1(region)(s)?;
    Ok((s, Page { header, regions }))
}

fn header(s: &str) -> IResult<&str, Header> {
    let (s, name) = title(s)?;
    let (s, entries) = many1(kv)(s)?;
    let entries: HashMap<&str, &str> = entries.into_iter().collect();
    let (_, size) = terminated(vec2, eof)(entries.get("size").unwrap())?; // TODO: handle error
    let premultiplied_alpha = if let Some(pma) = entries.get("pma") {
        boolean(pma)?.1
    } else {
        false
    };

    let header = Header {
        name: name.to_string(),
        size,
        premultiplied_alpha,
    };

    Ok((s, header))
}

fn region(s: &str) -> IResult<&str, Region> {
    let (s, name) = title(s)?;
    let (s, entries) = many1(kv)(s)?;
    let entries: HashMap<&str, &str> = entries.into_iter().collect();
    if entries.get("index").is_some() {
        todo!("region index");
    }
    let (_, bounds) = bounds(entries.get("bounds").unwrap())?; // TODO: required for now

    let region = Region {
        name: name.to_string(),
        bounds: Some(bounds),
        ..Default::default()
    };
    Ok((s, region))
}

fn title(s: &str) -> IResult<&str, &str> {
    let (s, _) = take_while(is_whitespace)(s)?;
    let (s, title) = terminated(symbols, tag("\n"))(s)?;
    Ok((s, title))
}

fn kv(s: &str) -> IResult<&str, (&str, &str)> {
    let (s, _) = take_while(is_whitespace)(s)?;
    let (s, (key, value)) = separated_pair(symbols, colon_separator, symbols)(s)?;
    let (s, _) = tag("\n")(s)?;
    Ok((s, (key, value)))
}

// fn separated_values<F, S, O, SO>(f: F, separator: sep: S) -> IResult<&str, Vec<O>>
// where
//     F: FnMut(&str) -> IResult<&str, O>,
//     S: FnMut(&str) -> IResult<&str, SO>,
// {
//
//
// }
//
fn bounds(s: &str) -> IResult<&str, Bounds> {
    let (s, (x, _, y, _, w, _, h)) = tuple((
        float,
        comma_separator,
        float,
        comma_separator,
        float,
        comma_separator,
        float,
    ))(s)?;

    let bounds = Bounds {
        position: Vec2::new(x, y),
        size: Vec2::new(w, h),
    };
    Ok((s, bounds))
}

fn boolean(s: &str) -> IResult<&str, bool> {
    if s == "true" {
        Ok((s, true))
    } else if s == "false" {
        Ok((s, false))
    } else {
        panic!("Unknown bool value: {}", s)
    }
}

fn vec2(s: &str) -> IResult<&str, Vec2> {
    let (s, (x, y)) = separated_pair(float, comma_separator, float)(s)?;
    Ok((s, Vec2 { x, y }))
}

fn float(s: &str) -> IResult<&str, f32> {
    let (s, n) = take_while1(|c: char| c.is_ascii_digit() || c == '.' || c == '-')(s)?;
    Ok((s, n.parse::<f32>().unwrap()))
}

fn empty_line(s: &str) -> IResult<&str, ()> {
    let (s, _) = take_while(is_whitespace)(s)?;
    let (s, _) = newline(s)?;
    Ok((s, ()))
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

fn comma_separator(s: &str) -> IResult<&str, ()> {
    let (s, _) = space0(s)?;
    let (s, _) = tag(",")(s)?;
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

#[cfg(test)]
mod tests {
    use super::*;

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
   bounds: 2, 21, 103, 81
   rotate: 90

page2.png
   size: 640, 480
   format: RGB565
   filter: Nearest, Nearest
   repeat: x
bg-dialog
   rotate: false
   bounds: 519, 223, 17, 38
   offsets: 2, 2, 21, 42
   split: 10, 10, 29, 10
   pad: -1, -1, 28, 10
   
       "#;
        let atlas = AtlasParser::parse(s).unwrap();
        dbg!(&atlas);
        assert_eq!(atlas.pages.len(), 2);
        let page = &atlas.pages[0];
        assert_eq!(page.header.name, "page1.png");
        assert_eq!(page.header.size, Vec2::new(640.0, 480.0));
        assert!(page.header.premultiplied_alpha);
        assert_eq!(page.regions.len(), 2);
    }
}
