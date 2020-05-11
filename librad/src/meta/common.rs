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

use std::{fmt, str::FromStr};

use ::url as the_url;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub const RAD_VERSION: u8 = 2;

pub type Label = String;

/// An RFC2822-ish email address.
///
/// While we validate the `local-part`, and ensure that the entire address is
/// shorter than 255 characters, we don't care much about the `domain`: pseudo
/// email addresses of the form `<local-part>@<some hash value>` are generally
/// acceptable within Radicle.
///
/// The validation logic is mostly stolen from the `addr` resp. `publicsuffix`
/// crates.
#[derive(Clone, Debug, PartialEq)]
pub struct EmailAddr {
    local: String,
    domain: String,
}

impl EmailAddr {
    pub fn parse(addr: &str) -> Result<Self, email::Error> {
        Self::from_str(addr)
    }

    pub fn local(&self) -> &str {
        &self.local
    }

    pub fn domain(&self) -> &str {
        &self.domain
    }
}

pub mod email {
    use regex::RegexSet;
    use thiserror::Error;

    lazy_static! {
        pub static ref LOCAL: RegexSet = {
            // these characters can be anywhere in the expresion
            let global = r#"[[:alnum:]!#$%&'*+/=?^_`{|}~-]"#;
            // non-ascii characters (an also be unquoted)
            let non_ascii = r#"[^\x00-\x7F]"#;
            // the pattern to match
            let quoted = r#"["(),\\:;<>@\[\]. ]"#;
            // combined regex
            let combined = format!(r#"({}*{}*)"#, global, non_ascii);

            let exprs = vec![
                // can be any combination of allowed characters
                format!(r#"^{}+$"#, combined),
                // can be any combination of allowed charaters
                // separated by a . in between
                format!(r#"^({0}+[.]?{0}+)+$"#, combined),
                // can be a quoted string with allowed plus
                // additional characters
                format!(r#"^"({}*{}*)*"$"#, combined, quoted),
            ];

            RegexSet::new(exprs).unwrap()
        };
    }

    #[derive(Debug, Error)]
    pub enum Error {
        #[error("Email address exceeds 254 character limit")]
        AddrTooLong,

        #[error("Invalid local-part of email address")]
        InvalidLocalPart,

        #[error("Invalid domain of email address")]
        InvalidDomain,
    }
}

impl FromStr for EmailAddr {
    type Err = email::Error;

    fn from_str(addr: &str) -> Result<Self, Self::Err> {
        if addr.chars().count() > 254 {
            return Err(Self::Err::AddrTooLong);
        }

        let mut parts = addr.rsplitn(2, '@');

        let domain = match parts.next() {
            Some(domain) => domain,
            None => return Err(Self::Err::InvalidDomain),
        };
        let local = match parts.next() {
            Some(local) => local,
            None => return Err(Self::Err::InvalidLocalPart),
        };

        if local.chars().count() > 64
            || (!local.starts_with('"') && local.contains(".."))
            || !email::LOCAL.is_match(local)
        {
            return Err(Self::Err::InvalidLocalPart);
        }

        Ok(Self {
            local: local.to_owned(),
            domain: domain.to_owned(),
        })
    }
}

impl fmt::Display for EmailAddr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}@{}", self.local, self.domain)
    }
}

impl Serialize for EmailAddr {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for EmailAddr {
    fn deserialize<D>(deserializer: D) -> Result<EmailAddr, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        EmailAddr::parse(&s).map_err(serde::de::Error::custom)
    }
}

/// A generic URL.
///
/// The implementation is based on the [WHATWG Specification](https://url.spec.whatwg.org/).
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct Url(the_url::Url);

impl Url {
    pub fn parse(s: &str) -> Result<Self, url::ParseError> {
        Self::from_str(s)
    }
}

pub mod url {
    use ::url as the_url;
    use thiserror::Error;

    #[derive(Debug, Error)]
    #[error("Error parsing Url: {0}")]
    pub struct ParseError(#[source] the_url::ParseError);

    impl ParseError {
        pub(crate) fn new(err: the_url::ParseError) -> Self {
            Self(err)
        }
    }
}

impl FromStr for Url {
    type Err = url::ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        the_url::Url::parse(s)
            .map(Url)
            .map_err(url::ParseError::new)
    }
}

impl fmt::Display for Url {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    const SIMPLE_EMAIL: &str = "leboeuf@example.org";
    const SIMPLE_URL: &str = "http://example.org/xyz";

    #[test]
    fn test_email_roundtrip() {
        let addr = EmailAddr::parse(SIMPLE_EMAIL).expect("Invalid EmailAddr");
        assert_eq!(addr.to_string(), SIMPLE_EMAIL.to_string())
    }

    #[test]
    fn test_email_serde() {
        let addr = EmailAddr::parse(SIMPLE_EMAIL).expect("Invalid EmailAddr");
        let ser = serde_json::to_string(&addr).unwrap();
        let de = serde_json::from_str(&ser).unwrap();
        assert_eq!(ser, format!("\"{}\"", SIMPLE_EMAIL));
        assert_eq!(addr, de)
    }

    #[test]
    fn test_url_rountrip() {
        let url = Url::parse(SIMPLE_URL).expect("Invalid URL");
        assert_eq!(url.to_string(), SIMPLE_URL.to_string())
    }

    #[test]
    fn test_url_serde() {
        let url = Url::parse(SIMPLE_URL).expect("Invalid URL");
        let ser = serde_json::to_string(&url).unwrap();
        let de = serde_json::from_str(&ser).unwrap();
        assert_eq!(ser, format!("\"{}\"", url));
        assert_eq!(url, de)
    }
}
