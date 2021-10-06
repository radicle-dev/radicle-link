// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::str::FromStr;

#[derive(Clone, Copy, Debug)]
pub enum Case {
    Lower,
    Upper,
    Pascal,
    Camel,
    Snake,
    ScreamingSnake,
    Kebab,
}

impl From<Case> for convert_case::Case {
    fn from(c: Case) -> Self {
        match c {
            Case::Lower => convert_case::Case::Lower,
            Case::Upper => convert_case::Case::Upper,
            Case::Pascal => convert_case::Case::Pascal,
            Case::Camel => convert_case::Case::Camel,
            Case::Snake => convert_case::Case::Snake,
            Case::ScreamingSnake => convert_case::Case::ScreamingSnake,
            Case::Kebab => convert_case::Case::Kebab,
        }
    }
}

impl FromStr for Case {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "lowercase" => Ok(Self::Lower),
            "UPPERCASE" => Ok(Self::Upper),
            "PascalCase" => Ok(Self::Pascal),
            "camelCase" => Ok(Self::Camel),
            "snake_case" => Ok(Self::Snake),
            "SCREAMING_SNAKE_CASE" => Ok(Self::ScreamingSnake),
            "kebab-case" => Ok(Self::Kebab),
            _ => Err("unsupported `rename_all` value"),
        }
    }
}

pub fn convert(s: &str, case: Option<Case>) -> String {
    use convert_case::Casing;

    match case {
        None => s.to_string(),
        Some(case) => s.to_case(case.into()),
    }
}
