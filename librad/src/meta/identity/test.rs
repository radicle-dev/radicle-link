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

use super::*;

use git2::Repository;
use serde::{Deserialize, Serialize};

use librad_test::tempdir::WithTmpDir;

type TmpRepository = WithTmpDir<Repository>;

fn repo() -> TmpRepository {
    WithTmpDir::new(|path| {
        Repository::init(path).map_err(|err| {
            std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Cannot init temporary git repo: {}", err),
            )
        })
    })
    .unwrap()
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
struct Payload {
    pub text: String,
}

impl Payload {
    pub fn new(text: &str) -> Self {
        Self {
            text: text.to_owned(),
        }
    }
}

#[test]
fn store_and_get_doc() {
    let repo = repo();
    let store = IdentityStore::new(&repo);

    let doc1 = DocBuilder::new_user().build(Payload::new("text")).unwrap();
    let rev = store.store_doc(&doc1, None).unwrap();
    let (doc2, root) = store.get_doc(&rev).unwrap();
    assert_eq!(doc1, doc2);
    assert_eq!(rev, root);
}

#[test]
fn store_and_get_identity() {
    let repo = repo();
    let store = IdentityStore::new(&repo);

    let doc = DocBuilder::new_user().build(Payload::new("text")).unwrap();
    let rev = store.store_doc(&doc, None).unwrap();

    let id1 = store
        .store_identity(IdentityBuilder::new(rev, doc))
        .unwrap();
    let id2 = store.get_identity(id1.commit()).unwrap();
    assert_eq!(id1, id2);
}
