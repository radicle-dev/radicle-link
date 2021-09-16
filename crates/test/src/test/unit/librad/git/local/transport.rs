// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use librad::{
    git::{
        local::transport::visible_remotes_glob,
        storage::glob::Pattern as _,
        types::Namespace,
        Urn,
    },
    reflike,
};

#[test]
fn visible_remotes_glob_seems_legit() {
    let urn = Urn::new(git2::Oid::zero().into());
    let glob = visible_remotes_glob(&urn);

    assert!(glob.matches(
        reflike!("refs/namespaces")
            .join(Namespace::from(&urn))
            .join(reflike!("refs/remotes/lolek/heads/next"))
            .as_str()
    ));
    assert!(glob.matches(
        reflike!("refs/namespaces")
            .join(Namespace::from(&urn))
            .join(reflike!("refs/remotes/bolek/tags/v0.99"))
            .as_str()
    ));
    assert!(!glob.matches("refs/heads/master"));
    assert!(!glob.matches("refs/namespaces/othernamespace/refs/remotes/tola/heads/next"));
    assert!(!glob.matches(
        reflike!("refs/namespaces")
            .join(Namespace::from(&urn))
            .join(reflike!("refs/heads/hidden"))
            .as_str()
    ));
}
