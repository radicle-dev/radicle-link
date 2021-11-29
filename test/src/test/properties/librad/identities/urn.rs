// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{convert::TryFrom, fmt::Debug};

use multihash::{Multihash, MultihashRef};
use proptest::prelude::*;

use librad::identities::urn::{HasProtocol, Urn};

use crate::{librad::identities::urn::gen_urn, roundtrip::*};

/// All serialisation roundtrips [`Urn`] must pass
pub fn trippin<R, E>(urn: Urn<R>)
where
    R: Clone + Debug + PartialEq + TryFrom<Multihash, Error = E> + HasProtocol,
    for<'a> R: TryFrom<MultihashRef<'a>>,
    for<'a> &'a R: Into<Multihash>,
    E: std::error::Error + Send + Sync + 'static,
{
    str_roundtrip(urn.clone());
    json_roundtrip(urn.clone());
    cbor_roundtrip(urn);
}

proptest! {
    #[test]
    fn roundtrip(urn in gen_urn()) {
        trippin(urn)
    }
}
