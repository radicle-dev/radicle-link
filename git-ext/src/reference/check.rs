// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use thiserror::Error;

pub struct Options {
    /// If `false`, the refname must contain at least one `/`.
    pub allow_onelevel: bool,
    /// If `true`, the refname may contain exactly one `*` character.
    pub allow_pattern: bool,
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    #[error("empty input")]
    Empty,
    #[error("lone '@' character")]
    LoneAt,
    #[error("consecutive or trailing slash")]
    Slash,
    #[error("ends with '.lock'")]
    DotLock,
    #[error("consecutive dots ('..')")]
    DotDot,
    #[error("at-open-brace ('@{{')")]
    AtOpenBrace,
    #[error("invalid character {0:?}")]
    InvalidChar(char),
    #[error("component starts with '.'")]
    StartsDot,
    #[error("component ends with '.'")]
    EndsDot,
    #[error("control character")]
    Control,
    #[error("whitespace")]
    Space,
    #[error("must contain at most one '*'")]
    Pattern,
    #[error("must contain at least one '/'")]
    OneLevel,
}

/// Validate that a string slice is a valid refname.
pub fn ref_format(opts: Options, s: &str) -> Result<(), Error> {
    match s {
        "" => Err(Error::Empty),
        "@" => Err(Error::LoneAt),
        _ => {
            let mut globs = 0usize;
            let mut parts = 0usize;

            for x in s.split('/') {
                if x.is_empty() {
                    return Err(Error::Slash);
                }

                parts += 1;

                if x.ends_with(".lock") {
                    return Err(Error::DotLock);
                }

                for (i, y) in x.chars().zip(x.chars().cycle().skip(1)).enumerate() {
                    match y {
                        ('.', '.') => return Err(Error::DotDot),
                        ('@', '{') => return Err(Error::AtOpenBrace),

                        ('\0', _) => return Err(Error::InvalidChar('\0')),
                        ('\\', _) => return Err(Error::InvalidChar('\\')),
                        ('~', _) => return Err(Error::InvalidChar('~')),
                        ('^', _) => return Err(Error::InvalidChar('^')),
                        (':', _) => return Err(Error::InvalidChar(':')),
                        ('?', _) => return Err(Error::InvalidChar('?')),
                        ('[', _) => return Err(Error::InvalidChar('[')),

                        ('*', _) => globs += 1,

                        ('.', _) if i == 0 => return Err(Error::StartsDot),
                        ('.', _) if i == x.len() - 1 => return Err(Error::EndsDot),

                        (z, _) if z.is_ascii_control() => return Err(Error::Control),
                        (z, _) if z.is_whitespace() => return Err(Error::Space),

                        _ => continue,
                    }
                }
            }

            if parts < 2 && !opts.allow_onelevel {
                Err(Error::OneLevel)
            } else if globs > 1 && opts.allow_pattern {
                Err(Error::Pattern)
            } else if globs > 0 && !opts.allow_pattern {
                Err(Error::InvalidChar('*'))
            } else {
                Ok(())
            }
        },
    }
}
