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

use std::{
    fmt::{Debug, Display},
    str::FromStr,
};

use librad::internal::canonical::Cjson;
use pretty_assertions::assert_eq;

pub fn json_roundtrip<A>(a: A)
where
    for<'de> A: Debug + PartialEq + serde::Serialize + serde::Deserialize<'de>,
{
    assert_eq!(
        a,
        serde_json::from_str(&serde_json::to_string(&a).unwrap()).unwrap()
    )
}

pub fn cjson_roundtrip<A>(a: A)
where
    for<'de> A: Debug + PartialEq + serde::Serialize + serde::Deserialize<'de>,
{
    assert_eq!(
        a,
        serde_json::from_slice(&Cjson(&a).canonical_form().unwrap()).unwrap()
    )
}

pub fn cbor_roundtrip<A>(a: A)
where
    for<'de> A: Debug + PartialEq + minicbor::Encode + minicbor::Decode<'de>,
{
    assert_eq!(a, minicbor::decode(&minicbor::to_vec(&a).unwrap()).unwrap())
}

pub fn str_roundtrip<A>(a: A)
where
    A: Debug + PartialEq + Display + FromStr,
    <A as FromStr>::Err: Debug,
{
    assert_eq!(a, a.to_string().parse().unwrap())
}
