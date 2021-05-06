// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{collections::BTreeMap, iter};

use librad::identities::{delegation, generic::error, sign::Signatures, Verifying};

use crate::librad::identities::generic::*;

#[test]
fn signed_no_signatures() {
    assert_matches!(
        Verifying::from(boring(
            iter::empty().collect::<delegation::Direct>(),
            Signatures::from(BTreeMap::new())
        ))
        .signed(),
        Err(error::Verify::NoSignatures)
    )
}
