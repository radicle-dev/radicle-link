// This file is part of radicle-link
// <https://github.com/radicle-dev/radicle-link>
//
// Copyright (C) 2019-2020 The Radicle Team <dev@radicle.xyz>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License version 3 or
// later as published by the Free Software Foundation.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

#[derive(Debug, Clone, Eq, PartialEq, thiserror::Error)]
pub enum Error<'a> {
    #[error("The trailers paragraph is missing in the given message")]
    MissingParagraph,

    #[error(
        "One or more trailers are not in the parseable format <token><separator><value>: '{0}'"
    )]
    Unparsable(&'a str),
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Trailer<'a> {
    pub token: Token<'a>,
    pub values: Vec<&'a str>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Token<'a>(&'a str);

/// Parse the trailers of the given message. It looks up the last paragraph
/// of the message and attempts to parse each of its lines as a [Trailer].
/// Fails if no trailers paragraph is found or if at least one trailer
/// fails to be parsed.
pub fn parse<'a>(message: &'a str, separators: &'a str) -> Result<Vec<Trailer<'a>>, Error<'a>> {
    let trailers_paragraph =
        match parser::paragraphs(message.trim_end()).map(|(_, ps)| ps.last().cloned()) {
            Ok(None) | Err(_) => return Err(Error::MissingParagraph),
            Ok(Some(p)) => p,
        };

    match parser::trailers(trailers_paragraph, separators) {
        Ok((rest, trailers)) if rest.is_empty() => Ok(trailers),
        Ok((unparseable, _)) => Err(Error::Unparsable(unparseable)),
        Err(_) => Err(Error::Unparsable(trailers_paragraph)),
    }
}

pub mod parser {
    use super::{Token, Trailer};
    use nom::{
        branch::alt,
        bytes::complete::{tag, take_until, take_while1},
        character::complete::{line_ending, not_line_ending, one_of, space0, space1},
        combinator::rest,
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
    fn token(s: &str) -> IResult<&str, Token> {
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
    fn values(s: &str) -> IResult<&str, Vec<&str>> {
        let (r, opt_inline_value) = until_eol_or_eof(s)?;
        let (r, mut values) = multiline_values(r)?;
        if !opt_inline_value.is_empty() {
            values.insert(0, opt_inline_value)
        }
        Ok((r, values))
    }

    fn multiline_values(s: &str) -> IResult<&str, Vec<&str>> {
        many0(indented_line_contents)(s)
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
                new_trailer("Co-authored-by", vec!["John Doe <john.doe@test.com>"]),
                new_trailer("Ticket", vec!["#42"]),
                new_trailer(
                    "Tested-by",
                    vec!["John <john@test.com>", "Jane <jane@test.com>"]
                ),
                new_trailer("Just-a-token", vec![]),
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
                new_trailer("Co-authored-by", vec!["John Doe <john.doe@test.com>"]),
                new_trailer("Ticket", vec!["#42"]),
                new_trailer(
                    "Tested-by",
                    vec![
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
            Err(Error::Unparsable(
                "John Doe <john.doe@test.com> # Unparsable token due to missing token"
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
            Err(Error::Unparsable(
                "&!#: John Doe <john.doe@test.com> # Unparsable token due to invalid token"
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
                new_trailer("Co-authored-by", vec!["John Doe <john.doe@test.com>"]),
                new_trailer("Ticket", vec!["#42"]),
                new_trailer("Tested-by", vec!["Tester <tester@test.com>"]),
            ])
        )
    }

    #[test]
    fn parse_empty_message() {
        let msg = "";
        assert_eq!(parse(msg, ":"), Err(Error::MissingParagraph))
    }

    fn new_trailer<'a>(token: &'a str, values: Vec<&'a str>) -> Trailer<'a> {
        Trailer {
            token: Token(token),
            values,
        }
    }
}
