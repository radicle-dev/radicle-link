// Copyright (c) 2014-2019 Geoffroy Couprie.
// SPDX-License-Identifier: MIT

use nom::{
    branch::alt,
    bytes::streaming::{tag, take_while1},
    character::streaming::{char, hex_digit0, multispace1, one_of},
    combinator::{map, map_res, recognize, value},
    error::{FromExternalError, ParseError},
    multi::fold_many0,
    sequence::{delimited, pair, preceded},
    IResult,
};
use unicode_normalization::UnicodeNormalization;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the escape sequences \\x00 - \\x7F are not supported")]
    ControlCode,
    #[error(transparent)]
    ParseInt(#[from] std::num::ParseIntError),
}

fn is_nonescaped_string_char(c: char) -> bool {
    let cv = c as u32;
    (cv >= 0x20) && (cv != 0x22) && (cv != 0x5C)
}

// Fail on an ASCII control code, i.e. \x00 - \x7F
fn control_code<'a, E>(input: &'a str) -> IResult<&'a str, &'a str, E>
where
    E: ParseError<&'a str> + FromExternalError<&'a str, Error>,
{
    let control_sequence = pair(one_of("01234567"), hex_digit0);
    map_res(pair(tag("\\x"), control_sequence), |_| Err(Error::ControlCode))(input)
}

// One or more unescaped text characters
fn nonescaped_string<'a, E>(input: &'a str) -> IResult<&'a str, &'a str, E>
where
    E: ParseError<&'a str>,
{
    take_while1(is_nonescaped_string_char)(input)
}

fn escape_code<'a, E>(input: &'a str) -> IResult<&'a str, &'a str, E>
where
    E: ParseError<&'a str> + FromExternalError<&'a str, Error>,
{
    alt((
        control_code,
        recognize(pair(
            tag("\\"),
            alt((
                tag("\""),
                tag("\\"),
                tag("/"),
                tag("b"),
                tag("f"),
                tag("n"),
                tag("r"),
                tag("t"),
                tag("u"),
            )),
        )),
    ))(input)
}

/// Parse a backslash, followed by any amount of whitespace. This is used later
/// to discard any escaped whitespace.
fn parse_escaped_whitespace<'a, E: ParseError<&'a str>>(
    input: &'a str,
) -> IResult<&'a str, &'a str, E> {
    preceded(char('\\'), multispace1)(input)
}

/// A string fragment contains a fragment of a string being parsed: either
/// a non-empty Literal (a series of non-escaped characters), a single
/// parsed escaped character, or a block of escaped whitespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StringFragment<'a> {
    Literal(&'a str),
    EscapedChar(&'a str),
    EscapedWS,
}

fn parse_fragment<'a, E>(input: &'a str) -> IResult<&'a str, StringFragment<'a>, E>
where
    E: ParseError<&'a str> + FromExternalError<&'a str, Error>,
{
    alt((
        map(nonescaped_string, StringFragment::Literal),
        map(escape_code, StringFragment::EscapedChar),
        value(StringFragment::EscapedWS, parse_escaped_whitespace),
    ))(input)
}

/// Parse a string. Use a loop of parse_fragment and push all of the fragments
/// into an output string.
pub fn parse<'a, E>(input: &'a str) -> IResult<&'a str, String, E>
where
    E: ParseError<&'a str> + FromExternalError<&'a str, Error>,
{
    let build_string = fold_many0(
        parse_fragment,
        String::new,
        |mut string, fragment| {
            match fragment {
                StringFragment::Literal(s) | StringFragment::EscapedChar(s) => string.push_str(s),
                StringFragment::EscapedWS => {},
            }
            string
        },
    );

    // Normalize Form C the resulting string
    map(delimited(char('"'), build_string, char('"')), |s| {
        s.nfc().fold(String::new(), |mut acc, ch| {
            let mut buf = [0; 4];
            let s = ch.encode_utf8(&mut buf);
            acc.push_str(s);
            acc
        })
    })(input)
}

