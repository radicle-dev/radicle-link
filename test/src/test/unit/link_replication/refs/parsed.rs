// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{fmt::Debug, num::ParseIntError};

use either::Either::*;
use link_replication::{
    refs::parsed::{parse, Cat, Identity, Parsed, Rad, Refs},
    Urn,
};

use super::PEER;

fn succeed<T>(expected: Parsed<T>, input: &str)
where
    T: Urn + Debug + PartialEq,
{
    let actual = parse::<T>(input.into()).expect("parse should succeed");
    assert_eq!(expected, actual);

    use bstr::{BString, ByteVec as _};
    let mut input = BString::from(input);
    input.insert_str(
        "refs/".len(),
        "remotes/hyn3aar1qghrnjrdi161oks1w3z9s173mxti88ci6qthps8brmp6yo/",
    );
    let actual = parse::<T>(input.as_ref()).expect("parse should succeed");
    assert_eq!(
        Parsed {
            remote: Some(*PEER),
            ..expected
        },
        actual
    );
}

fn fail<T>(input: &str)
where
    T: Urn + Debug,
{
    let res = parse::<T>(input.into());
    assert!(
        res.is_none(),
        "parse expected to fail with input `{}`: {:?}",
        input,
        res
    );
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Usize(usize);

impl Urn for Usize {
    type Error = ParseIntError;

    fn try_from_id(s: impl AsRef<str>) -> Result<Self, Self::Error> {
        s.as_ref().parse().map(Self)
    }

    fn encode_id(&self) -> String {
        self.0.to_string()
    }
}

#[test]
fn not_refs() {
    fail::<Identity>("rifs")
}

#[test]
fn incomplete_input() {
    for x in [
        "",
        "refs",
        "refs/cobs",
        "refs/heads",
        "refs/notes",
        "refs/rad",
        "refs/rad/ids",
        "refs/remotes",
        "refs/remotes/hyn3aar1qghrnjrdi161oks1w3z9s173mxti88ci6qthps8brmp6yo",
        "refs/remotes/hyn3aar1qghrnjrdi161oks1w3z9s173mxti88ci6qthps8brmp6yo/cobs",
        "refs/remotes/hyn3aar1qghrnjrdi161oks1w3z9s173mxti88ci6qthps8brmp6yo/heads",
        "refs/remotes/hyn3aar1qghrnjrdi161oks1w3z9s173mxti88ci6qthps8brmp6yo/notes",
        "refs/remotes/hyn3aar1qghrnjrdi161oks1w3z9s173mxti88ci6qthps8brmp6yo/tags",
        "refs/remotes/hyn3aar1qghrnjrdi161oks1w3z9s173mxti88ci6qthps8brmp6yo/whatever",
        "refs/tags",
        "refs/whatever",
    ] {
        fail::<Usize>(x);
        let mut x = x.to_owned();
        x.push('/');
        fail::<Usize>(x.as_str());
    }
}

#[test]
fn trailing_slash() {
    for x in [
        "refs/heads/main/",
        "refs/remotes/hyn3aar1qghrnjrdi161oks1w3z9s173mxti88ci6qthps8brmp6yo/heads/main/",
        "refs/tags/patches/one/",
    ] {
        fail::<Identity>(x);
    }
}

#[test]
fn invalid_peer_id() {
    assert!(parse::<Identity>("refs/remotes/hyperhyper/rad/id".into()).is_none())
}

#[test]
fn rad_id() {
    succeed::<Identity>(
        Parsed {
            remote: None,
            inner: Left(Rad::Id),
        },
        "refs/rad/id",
    );
}

#[test]
fn rad_self() {
    succeed::<Identity>(
        Parsed {
            remote: None,
            inner: Left(Rad::Me),
        },
        "refs/rad/self",
    );
}

#[test]
fn rad_signed_refs() {
    succeed::<Identity>(
        Parsed {
            remote: None,
            inner: Left(Rad::SignedRefs),
        },
        "refs/rad/signed_refs",
    );
}

#[test]
fn rad_ids() {
    succeed::<Usize>(
        Parsed {
            remote: None,
            inner: Left(Rad::Ids { urn: Usize(42) }),
        },
        "refs/rad/ids/42",
    );
}

#[test]
fn unknown_rad() {
    fail::<Identity>("refs/rad/asdf");
    fail::<Identity>(
        "refs/remotes/hyn3aar1qghrnjrdi161oks1w3z9s173mxti88ci6qthps8brmp6yo/rad/asdf",
    );
}

#[test]
fn rad_extra_input() {
    for x in [
        "refs/rad/id/id",
        "refs/rad/id/id/",
        "refs/rad/self/asdf",
        "refs/rad/signed_refs/qwert",
        "refs/rad/ids/42/blah",
        "refs/remotes/hyn3aar1qghrnjrdi161oks1w3z9s173mxti88ci6qthps8brmp6yo/rad/id/ygasd",
        "refs/remotes/hyn3aar1qghrnjrdi161oks1w3z9s173mxti88ci6qthps8brmp6yo/rad/self/knfbe",
        "refs/remotes/hyn3aar1qghrnjrdi161oks1w3z9s173mxti88ci6qthps8brmp6yo/rad/signed_refs/oiyb",
        "refs/remotes/hyn3aar1qghrnjrdi161oks1w3z9s173mxti88ci6qthps8brmp6yo/rad/ids/32/xy",
    ] {
        fail::<Usize>(x);
    }
}

#[test]
fn heads() {
    succeed::<Identity>(
        Parsed {
            remote: None,
            inner: Right(Refs {
                name: vec!["three".into(), "levels".into(), "deep".into()],
                cat: Cat::Heads,
            }),
        },
        "refs/heads/three/levels/deep",
    );
}

#[test]
fn notes() {
    succeed::<Identity>(
        Parsed {
            remote: None,
            inner: Right(Refs {
                name: vec!["topic".into()],
                cat: Cat::Notes,
            }),
        },
        "refs/notes/topic",
    );
}

#[test]
fn tags() {
    succeed::<Identity>(
        Parsed {
            remote: None,
            inner: Right(Refs {
                name: vec!["cycle".into(), "20211221".into()],
                cat: Cat::Tags,
            }),
        },
        "refs/tags/cycle/20211221",
    );
}

#[test]
fn cobs() {
    succeed::<Identity>(
        Parsed {
            remote: None,
            inner: Right(Refs {
                name: vec!["patch".into(), "1".into()],
                cat: Cat::Cobs,
            }),
        },
        "refs/cobs/patch/1",
    );
}

#[test]
fn unknown_cat() {
    succeed::<Identity>(
        Parsed {
            remote: None,
            inner: Right(Refs {
                name: vec!["snoop".into()],
                cat: Cat::Unknown("dogs".into()),
            }),
        },
        "refs/dogs/snoop",
    );
}
