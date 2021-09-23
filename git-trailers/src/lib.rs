// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{borrow::Cow, convert::TryFrom, fmt, ops::Deref};

use thiserror::Error;

#[derive(Debug, Clone, Eq, PartialEq, Error)]
#[non_exhaustive]
pub enum Error {
    #[error("the trailers paragraph is missing in the given message")]
    MissingParagraph,

    #[error("trailing data after trailers section: '{0}")]
    Trailing(String),

    #[error(transparent)]
    Parse(#[from] nom::Err<(String, nom::error::ErrorKind)>),
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Trailer<'a> {
    pub token: Token<'a>,
    pub values: Vec<Cow<'a, str>>,
}

impl<'a> Trailer<'a> {
    pub fn display(&'a self, separator: &'a str) -> Display<'a> {
        Display {
            trailer: self,
            separator,
        }
    }

    pub fn to_owned(&self) -> OwnedTrailer {
        OwnedTrailer::from(self)
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Token<'a>(&'a str);

/// A version of the Trailer<'a> which owns it's token and values. Useful for
/// when you need to carry trailers around in a long lived data structure.
pub struct OwnedTrailer {
    token: OwnedToken,
    values: Vec<String>,
}

pub struct OwnedToken(String);

impl<'a> From<&Trailer<'a>> for OwnedTrailer {
    fn from(t: &Trailer<'a>) -> Self {
        OwnedTrailer {
            token: OwnedToken(t.token.0.to_string()),
            values: t.values.iter().map(|v| v.to_string()).collect(),
        }
    }
}

impl<'a> From<Trailer<'a>> for OwnedTrailer {
    fn from(t: Trailer<'a>) -> Self {
        (&t).into()
    }
}

impl<'a> From<&'a OwnedTrailer> for Trailer<'a> {
    fn from(t: &'a OwnedTrailer) -> Self {
        Trailer {
            token: Token(t.token.0.as_str()),
            values: t.values.iter().map(Cow::from).collect(),
        }
    }
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum InvalidToken {
    #[error("trailing characters: '{0}'")]
    Trailing(String),

    #[error(transparent)]
    Parse(#[from] nom::Err<(String, nom::error::ErrorKind)>),
}

impl<'a> TryFrom<&'a str> for Token<'a> {
    type Error = InvalidToken;

    fn try_from(s: &'a str) -> Result<Self, Self::Error> {
        match parser::token(s) {
            Ok((rest, token)) if rest.is_empty() => Ok(token),
            Ok((trailing, _)) => Err(InvalidToken::Trailing(trailing.to_owned())),
            Err(e) => Err(e.to_owned().into()),
        }
    }
}

impl Deref for Token<'_> {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.0
    }
}

pub struct Display<'a> {
    trailer: &'a Trailer<'a>,
    separator: &'a str,
}

impl<'a> fmt::Display for Display<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}{}{}",
            self.trailer.token.deref(),
            self.separator,
            self.trailer.values.join("\n  ")
        )
    }
}

pub trait Separator<'a> {
    fn sep_for(&self, token: &Token) -> &'a str;
}

impl<'a> Separator<'a> for &'a str {
    fn sep_for(&self, _: &Token) -> &'a str {
        self
    }
}

impl<'a, F> Separator<'a> for F
where
    F: Fn(&Token) -> &'a str,
{
    fn sep_for(&self, token: &Token) -> &'a str {
        self(token)
    }
}

pub struct DisplayMany<'a, S> {
    separator: S,
    trailers: &'a [Trailer<'a>],
}

impl<'a, S> fmt::Display for DisplayMany<'a, S>
where
    S: Separator<'a>,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        for (i, trailer) in self.trailers.iter().enumerate() {
            if i > 0 {
                writeln!(f)?
            }

            write!(
                f,
                "{}",
                trailer.display(self.separator.sep_for(&trailer.token))
            )?
        }

        Ok(())
    }
}

/// Parse the trailers of the given message. It looks up the last paragraph
/// of the message and attempts to parse each of its lines as a [Trailer].
/// Fails if no trailers paragraph is found or if at least one trailer
/// fails to be parsed.
pub fn parse<'a>(message: &'a str, separators: &'a str) -> Result<Vec<Trailer<'a>>, Error> {
    let trailers_paragraph =
        match parser::paragraphs(message.trim_end()).map(|(_, ps)| ps.last().cloned()) {
            Ok(None) | Err(_) => return Err(Error::MissingParagraph),
            Ok(Some(p)) => p,
        };

    match parser::trailers(trailers_paragraph, separators) {
        Ok((rest, trailers)) if rest.is_empty() => Ok(trailers),
        Ok((unparseable, _)) => Err(Error::Trailing(unparseable.to_owned())),
        Err(e) => Err(e.to_owned().into()),
    }
}

/// Render a slice of trailers.
///
/// The `separator` can be either a string slice, or a closure which may choose
/// a different separator for each [`Token`] encountered. Note that multiline
/// trailers are rendered with a fixed indent, so the result is not
/// layout-preserving.
pub fn display<'a, S>(separator: S, trailers: &'a [Trailer<'a>]) -> DisplayMany<'a, S>
where
    S: Separator<'a>,
{
    DisplayMany {
        separator,
        trailers,
    }
}

pub mod parser {
    use std::borrow::Cow;

    use super::{Token, Trailer};
    use nom::{
        branch::alt,
        bytes::complete::{tag, take_until, take_while1},
        character::complete::{line_ending, not_line_ending, one_of, space0, space1},
        combinator::{map, rest},
        multi::{many0, separated_list},
        sequence::{delimited, preceded, separated_pair, terminated},
        IResult,
    };

    const EMPTY_LINE: &str = "\n\n";

    pub fn paragraphs(s: &str) -> IResult<&str, Vec<&str>> {
        separated_list(tag(EMPTY_LINE), paragraph)(s)
    }

    pub fn paragraph(s: &str) -> IResult<&str, &str> {
        alt((take_until(EMPTY_LINE), rest))(s)
    }

    /// Parse all the possible trailers.
    /// It stops when it can no longer parse valid trailers.
    pub fn trailers<'a>(s: &'a str, separators: &'a str) -> IResult<&'a str, Vec<Trailer<'a>>> {
        many0(|s| trailer(s, separators))(s)
    }

    /// Parse a trailer, which can have an inlined or multilined value.
    pub fn trailer<'a>(s: &'a str, separators: &'a str) -> IResult<&'a str, Trailer<'a>> {
        let parser = separated_pair(token, |s| separator(separators, s), values);
        let (rest, (token, values)) = parser(s)?;
        Ok((rest, Trailer { token, values }))
    }

    /// Parse a trailer token.
    pub(super) fn token(s: &str) -> IResult<&str, Token> {
        take_while1(|c: char| c.is_alphanumeric() || c == '-')(s)
            .map(|(i, token_str)| (i, Token(token_str)))
    }

    /// Parse the trailer separator, which can be delimited by spaces.
    fn separator<'a>(separators: &'a str, s: &'a str) -> IResult<&'a str, char> {
        delimited(space0, one_of(separators), space0)(s)
    }

    /// Parse the trailer values, which gathers the value after the separator
    /// (if any) and possible following multilined values, indented by a
    /// space.
    fn values(s: &str) -> IResult<&str, Vec<Cow<'_, str>>> {
        let (r, opt_inline_value) = until_eol_or_eof(s)?;
        let (r, mut values) = multiline_values(r)?;
        if !opt_inline_value.is_empty() {
            values.insert(0, opt_inline_value.into())
        }
        Ok((r, values))
    }

    fn multiline_values(s: &str) -> IResult<&str, Vec<Cow<'_, str>>> {
        many0(map(indented_line_contents, Cow::from))(s)
    }

    fn until_eol_or_eof(s: &str) -> IResult<&str, &str> {
        alt((until_eol, rest))(s)
    }

    /// Parse an indented line, i.e, a line that starts with a space.
    /// Extracts the line contents, ignoring the indentation and the
    /// new line character.
    fn indented_line_contents(s: &str) -> IResult<&str, &str> {
        preceded(space1, until_eol_or_eof)(s)
    }

    /// Consume the input until the end of the line, ignoring the new line
    /// character.
    fn until_eol(s: &str) -> IResult<&str, &str> {
        terminated(not_line_ending, line_ending)(s)
    }
}
