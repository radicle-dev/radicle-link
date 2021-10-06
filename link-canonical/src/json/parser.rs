// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::ops::Neg;

use nom::{
    branch::alt,
    bytes::streaming::{tag, take_while},
    character::streaming::{char, digit0, one_of},
    combinator::{cut, map, recognize, value},
    error::{context, ContextError, FromExternalError, ParseError},
    multi::separated_list0,
    sequence::{self, preceded, separated_pair, terminated},
};

use crate::{
    json::{ToCjson as _, Value},
    Cstring,
};

mod string;

pub fn json<'a, E>(i: &'a str) -> nom::IResult<&'a str, Value, E>
where
    E: ParseError<&'a str> + ContextError<&'a str> + FromExternalError<&'a str, string::Error>,
{
    preceded(sp, alt((string, number, object, array, boolean, null)))(i)
}

fn object<'a, E>(i: &'a str) -> nom::IResult<&'a str, Value, E>
where
    E: ParseError<&'a str> + ContextError<&'a str> + FromExternalError<&'a str, string::Error>,
{
    context(
        "object",
        preceded(
            char('{'),
            cut(terminated(
                map(members, |members| {
                    Value::Object(members.into_iter().collect())
                }),
                preceded(sp, char('}')),
            )),
        ),
    )(i)
}

fn members<'a, E>(i: &'a str) -> nom::IResult<&'a str, Vec<(Cstring, Value)>, E>
where
    E: ParseError<&'a str> + ContextError<&'a str> + FromExternalError<&'a str, string::Error>,
{
    separated_list0(preceded(sp, char(',')), pair)(i)
}

fn pair<'a, E>(i: &'a str) -> nom::IResult<&'a str, (Cstring, Value), E>
where
    E: ParseError<&'a str> + ContextError<&'a str> + FromExternalError<&'a str, string::Error>,
{
    separated_pair(preceded(sp, cstring), cut(preceded(sp, char(':'))), json)(i)
}

fn array<'a, E>(i: &'a str) -> nom::IResult<&'a str, Value, E>
where
    E: ParseError<&'a str> + ContextError<&'a str> + FromExternalError<&'a str, string::Error>,
{
    map(
        context(
            "array",
            preceded(
                char('['),
                cut(terminated(
                    separated_list0(preceded(sp, char(',')), json),
                    preceded(sp, char(']')),
                )),
            ),
        ),
        |values| Value::Array(values.into_iter().collect()),
    )(i)
}

fn null<'a, E>(i: &'a str) -> nom::IResult<&'a str, Value, E>
where
    E: ParseError<&'a str>,
{
    value(Value::Null, tag("null"))(i)
}

fn boolean<'a, E>(i: &'a str) -> nom::IResult<&'a str, Value, E>
where
    E: ParseError<&'a str> + ContextError<&'a str>,
{
    context(
        "bool",
        alt((
            value(Value::Bool(true), tag("true")),
            value(Value::Bool(false), tag("false")),
        )),
    )(i)
}

fn string<'a, E>(i: &'a str) -> nom::IResult<&'a str, Value, E>
where
    E: ParseError<&'a str> + ContextError<&'a str> + FromExternalError<&'a str, string::Error>,
{
    context("string", map(cstring, |s| s.into_cjson()))(i)
}

fn cstring<'a, E>(i: &'a str) -> nom::IResult<&'a str, Cstring, E>
where
    E: ParseError<&'a str> + FromExternalError<&'a str, string::Error>,
{
    map(string::parse, Cstring::from)(i)
}

fn number<'a, E>(i: &'a str) -> nom::IResult<&'a str, Value, E>
where
    E: ParseError<&'a str> + ContextError<&'a str>,
{
    context("number", alt((signed, unsigned)))(i)
}

fn digits<'a, E>(i: &'a str) -> nom::IResult<&'a str, &str, E>
where
    E: ParseError<&'a str>,
{
    alt((
        tag("0"),
        recognize(sequence::pair(one_of("123456789"), digit0)),
    ))(i)
}

fn signed<'a, E>(i: &'a str) -> nom::IResult<&'a str, Value, E>
where
    E: ParseError<&'a str>,
{
    preceded(
        minus,
        map(digits, |digits: &'a str| {
            digits.parse::<i64>().unwrap().neg().into_cjson()
        }),
    )(i)
}

fn unsigned<'a, E>(i: &'a str) -> nom::IResult<&'a str, Value, E>
where
    E: ParseError<&'a str>,
{
    map(digits, |digits: &'a str| {
        digits.parse::<u64>().unwrap().into_cjson()
    })(i)
}

fn minus<'a, E>(i: &'a str) -> nom::IResult<&'a str, char, E>
where
    E: ParseError<&'a str>,
{
    char('-')(i)
}

fn sp<'a, E: ParseError<&'a str>>(i: &'a str) -> nom::IResult<&'a str, &'a str, E> {
    let chars = " \t\r\n";

    // nom combinators like `take_while` return a function. That function is the
    // parser,to which we can pass the input
    take_while(move |c| chars.contains(c))(i)
}
