// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use crate::librad::git::refs::gen_refs;
use librad::git::refs::Refs;
use proptest::prelude::*;

proptest! {
    #[test]
    fn serde_isomorphism(refs in gen_refs()) {
        let serialized = serde_json::to_vec(&refs).unwrap();
        let deserialized: Refs = serde_json::from_slice(&serialized).unwrap();
        assert_eq!(refs, deserialized);
    }
}
