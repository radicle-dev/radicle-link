// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{borrow::Cow, convert::TryFrom, fmt, ops::Deref};

use thiserror::Error;

#[derive(Debug, Clone, Eq, PartialEq, Error)]
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
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Token<'a>(&'a str);

#[derive(Debug, Error)]
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
        &self.0
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
    fn values<'a>(s: &'a str) -> IResult<&'a str, Vec<Cow<'a, str>>> {
        let (r, opt_inline_value) = until_eol_or_eof(s)?;
        let (r, mut values) = multiline_values(r)?;
        if !opt_inline_value.is_empty() {
            values.insert(0, opt_inline_value.into())
        }
        Ok((r, values))
    }

    fn multiline_values<'a>(s: &'a str) -> IResult<&'a str, Vec<Cow<'a, str>>> {
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

#[cfg(test)]
mod tests {
    use super::*;

    use pretty_assertions::assert_eq;

    #[test]
    fn parse_message_with_valid_trailers() {
        let msg = r#"Subject

A multiline
description.

Co-authored-by: John Doe <john.doe@test.com>
Ticket: #42
Tested-by:
    John <john@test.com>
    Jane <jane@test.com>
Just-a-token:

"#;
        assert_eq!(
            parse(msg, ":"),
            Ok(vec![
                new_trailer("Co-authored-by", &["John Doe <john.doe@test.com>"]),
                new_trailer("Ticket", &["#42"]),
                new_trailer(
                    "Tested-by",
                    &["John <john@test.com>", "Jane <jane@test.com>"]
                ),
                new_trailer("Just-a-token", &[]),
            ])
        )
    }

    #[test]
    fn parse_message_trailers_with_custom_separators() {
        let separators = ":=$";
        let msg = r#"Subject

A multiline
description.

Co-authored-by: John Doe <john.doe@test.com>
Ticket = #42
Tested-by $User <user@test.com>
    John <john@test.com>
    Jane <jane@test.com>
"#;
        assert_eq!(
            parse(msg, separators),
            Ok(vec![
                new_trailer("Co-authored-by", &["John Doe <john.doe@test.com>"]),
                new_trailer("Ticket", &["#42"]),
                new_trailer(
                    "Tested-by",
                    &[
                        "User <user@test.com>",
                        "John <john@test.com>",
                        "Jane <jane@test.com>"
                    ]
                ),
            ])
        )
    }

    #[test]
    fn parse_message_trailers_with_missing_token() {
        let msg = r#"Subject

Good-trailer: true
John Doe <john.doe@test.com> # Unparsable token due to missing token"#;
        assert_eq!(
            parse(msg, ":"),
            Err(Error::Trailing(
                "John Doe <john.doe@test.com> # Unparsable token due to missing token".to_owned()
            ))
        )
    }

    #[test]
    fn parse_message_trailers_with_invalid_token() {
        let msg = r#"Subject

Good-trailer: true
&!#: John Doe <john.doe@test.com> # Unparsable token due to invalid token"#;
        assert_eq!(
            parse(msg, ":"),
            Err(Error::Trailing(
                "&!#: John Doe <john.doe@test.com> # Unparsable token due to invalid token"
                    .to_owned()
            ))
        )
    }

    #[test]
    fn parse_message_with_only_trailers() {
        let msg = r#"Co-authored-by: John Doe <john.doe@test.com>
Ticket: #42
Tested-by: Tester <tester@test.com>
"#;
        assert_eq!(
            parse(msg, ":"),
            Ok(vec![
                new_trailer("Co-authored-by", &["John Doe <john.doe@test.com>"]),
                new_trailer("Ticket", &["#42"]),
                new_trailer("Tested-by", &["Tester <tester@test.com>"]),
            ])
        )
    }

    #[test]
    fn parse_empty_message() {
        let msg = "";
        assert_eq!(parse(msg, ":"), Err(Error::MissingParagraph))
    }

    #[test]
    fn display_static() {
        let msg = r#"Tested-by: Alice
  Bob
  Carol
  Dylan
Acked-by: Eve"#;

        let parsed = parse(msg, ":").unwrap();
        let rendered = format!("{}", display(": ", &parsed));
        assert_eq!(&rendered, msg);
    }

    #[test]
    fn display_dynamic() {
        let msg = r#"Co-authored-by: John Doe <john.doe@test.com>
Tested-by: Tester <tester@test.com>
Fixes #42"#;

        let parsed = parse(msg, ":#").unwrap();
        let rendered = format!(
            "{}",
            display(
                |t: &Token| if t.deref() == "Fixes" { " #" } else { ": " },
                &parsed
            )
        );
        assert_eq!(rendered, msg)
    }

    fn new_trailer<'a>(token: &'a str, values: &[&'a str]) -> Trailer<'a> {
        Trailer {
            token: Token(token),
            values: values.iter().map(|s| Cow::from(*s)).collect(),
        }
    }
}
