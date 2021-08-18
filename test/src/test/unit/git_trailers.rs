// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{borrow::Cow, convert::TryFrom as _, ops::Deref as _};

use git_trailers::{display, parse, Error, Token, Trailer};
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
            "&!#: John Doe <john.doe@test.com> # Unparsable token due to invalid token".to_owned()
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
        token: Token::try_from(token).unwrap(),
        values: values.iter().map(|s| Cow::from(*s)).collect(),
    }
}
