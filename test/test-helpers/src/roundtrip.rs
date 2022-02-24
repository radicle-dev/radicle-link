// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    fmt::{Debug, Display},
    str::FromStr,
};

use link_canonical::Cjson;
use pretty_assertions::assert_eq;

pub fn json<A>(a: A)
where
    for<'de> A: Debug + PartialEq + serde::Serialize + serde::Deserialize<'de>,
{
    assert_eq!(
        a,
        serde_json::from_str(&serde_json::to_string(&a).unwrap()).unwrap()
    )
}

pub fn cjson<A>(a: A)
where
    for<'de> A: Debug + PartialEq + serde::Serialize + serde::Deserialize<'de>,
{
    assert_eq!(
        a,
        serde_json::from_slice(&Cjson(&a).canonical_form().unwrap()).unwrap()
    )
}

pub fn cbor<A>(a: A)
where
    for<'de> A: Debug + PartialEq + minicbor::Encode + minicbor::Decode<'de>,
{
    assert_eq!(a, minicbor::decode(&minicbor::to_vec(&a).unwrap()).unwrap())
}

pub fn str<A>(a: A)
where
    A: Debug + PartialEq + Display + FromStr,
    <A as FromStr>::Err: Debug,
{
    assert_eq!(a, a.to_string().parse().unwrap())
}
