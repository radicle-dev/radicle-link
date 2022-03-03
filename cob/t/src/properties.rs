// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use librad::{
    collaborative_objects::{ObjRefMatch, ObjRefMatcher},
    git::types::{Namespace, Reference},
};
use link_crypto_test::gen::gen_peer_id;
use link_identities_test::gen::urn::gen_urn;
use proptest::prelude::*;

use crate::gen::{gen_objectid, gen_typename};

proptest! {
    #[test]
    fn objmatcher_remote(
        project_urn in gen_urn(),
        remote in gen_peer_id(),
        typename in gen_typename(),
        object_id in gen_objectid()) {
            let matcher = ObjRefMatcher::new(&project_urn, &typename);
            let reference = Reference::rad_collaborative_object(
                Namespace::from(project_urn),
                remote,
                typename,
                object_id
            );
            println!("reference: {}", reference);
            assert_eq!(matcher.match_ref(reference.to_string().as_str()), ObjRefMatch::Remote(object_id));
    }
}

proptest! {
    #[test]
    fn objmatcher_local(
        project_urn in gen_urn(),
        typename in gen_typename(),
        object_id in gen_objectid()) {
            let matcher = ObjRefMatcher::new(&project_urn, &typename);
            let reference = Reference::rad_collaborative_object(
                Namespace::from(project_urn),
                None,
                typename,
                object_id
            );
            println!("Reference: {}", reference);
            assert_eq!(matcher.match_ref(reference.to_string().as_str()), ObjRefMatch::Local(object_id));
    }
}
