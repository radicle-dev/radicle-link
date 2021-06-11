// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{collections::BTreeSet, convert::TryFrom as _, thread, time::Duration};

use librad::{
    git::{
        storage::watch::{EventKind, NamespaceEvent},
        Urn,
    },
    git_ext::RefLike,
    keys::SecretKey,
};

use crate::{librad::git::storage::storage, logging, rad::identities::TestProject};

#[test]
fn namespaces() {
    logging::init();

    let store = storage(SecretKey::new());
    let (watcher, events) = store.watch().namespaces().unwrap();
    let TestProject { project, owner } = TestProject::create(&store).unwrap();

    let events = events
        .into_iter()
        .map(|NamespaceEvent { path, kind }| {
            (
                Urn::try_from(RefLike::try_from(path.to_str().unwrap()).unwrap()).unwrap(),
                kind,
            )
        })
        .collect::<BTreeSet<_>>();

    assert_eq!(
        vec![
            (project.urn(), EventKind::Create),
            (owner.urn(), EventKind::Create)
        ]
        .into_iter()
        .collect::<BTreeSet<_>>(),
        events
    )
}
