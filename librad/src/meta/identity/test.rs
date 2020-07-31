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
use sodiumoxide::crypto::sign::ed25519::Seed;

use librad_test::tempdir::WithTmpDir;

const SEED: Seed = Seed([
    20, 21, 6, 102, 102, 57, 20, 67, 219, 198, 236, 108, 148, 15, 182, 52, 167, 27, 29, 81, 181,
    134, 74, 88, 174, 254, 78, 69, 84, 149, 84, 167,
]);

fn new_key_from_seed(seed_value: u8) -> SecretKey {
    let mut seed = SEED;
    seed.0[0] = seed_value;
    SecretKey::from_seed(&seed)
}

lazy_static! {
    pub static ref K1: SecretKey = new_key_from_seed(1);
    pub static ref K2: SecretKey = new_key_from_seed(2);
    pub static ref K3: SecretKey = new_key_from_seed(3);
    pub static ref K4: SecretKey = new_key_from_seed(4);
    pub static ref K5: SecretKey = new_key_from_seed(5);
}

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

fn new_user_doc<'a>(
    store: &IdentityStore,
    text: &str,
    keys: impl IntoIterator<Item = &'a PublicKey>,
) -> (Doc<Untrusted>, Revision) {
    let mut builder = DocBuilder::new_user();
    for k in keys {
        builder.add_key(k.clone()).unwrap();
    }
    let doc = builder.build(Payload::new(text)).unwrap();
    let rev = store.store_doc(&doc, None).unwrap();
    (doc, rev)
}

fn replace_user_doc<'a>(
    store: &IdentityStore,
    text: &str,
    replaces: Revision,
    root: &Revision,
    keys: impl IntoIterator<Item = &'a PublicKey>,
) -> (Doc<Untrusted>, Revision) {
    let mut builder = DocBuilder::new_user();
    let builder = builder.replaces(replaces);
    for k in keys {
        builder.add_key(k.clone()).unwrap();
    }
    let doc = builder.build(Payload::new(text)).unwrap();
    let rev = store.store_doc(&doc, Some(root)).unwrap();
    (doc, rev)
}

#[test]
fn store_and_get_doc() {
    let repo = repo();
    let store = IdentityStore::new(&repo);

    let (doc1, rev) = new_user_doc(&store, "text", &[]);
    let (doc2, root) = store.get_doc(&rev).unwrap();
    assert_eq!(doc1, doc2);
    assert_eq!(rev, root);
}

#[test]
fn store_and_get_identity() {
    let repo = repo();
    let store = IdentityStore::new(&repo);

    let (doc, rev) = new_user_doc(&store, "text", &[]);

    let id1 = store
        .store_identity(IdentityBuilder::new(rev, doc))
        .unwrap();
    let id2 = store.get_identity(id1.commit()).unwrap();
    assert_eq!(id1, id2);
}

#[test]
fn encode_and_decode_signatures() {
    let data = vec![42, 3, 7, 9];

    let mut sigs1 = BTreeMap::new();
    sigs1.insert(K1.public(), K1.sign(&data));
    sigs1.insert(K2.public(), K2.sign(&data));
    sigs1.insert(K3.public(), K3.sign(&data));

    let mut text = "some random text\n\nand some more\n".to_string();
    append_signatures(&mut text, &sigs1);
    text.push_str("\neven more random babble\n");
    let sigs2 = parse_signatures(Some(&text));

    assert_eq!(sigs1.len(), 3);
    assert!(sigs1.contains_key(&K1.public()));
    assert!(sigs1.contains_key(&K2.public()));
    assert!(sigs1.contains_key(&K3.public()));
    assert_eq!(sigs1, sigs2);
}

#[test]
fn sign_and_store_identity() {
    let repo = repo();
    let store = IdentityStore::new(&repo);

    let (doc, rev) = new_user_doc(&store, "text", &[]);

    let id1 = store
        .store_identity(
            IdentityBuilder::new(rev, doc)
                .sign(K1.clone())
                .sign(K2.clone()),
        )
        .unwrap();
    let id2 = store.get_identity(id1.commit()).unwrap();
    assert_eq!(id1.signatures().len(), 2);
    id1.verify_signatures().unwrap();
    id2.verify_signatures().unwrap();
    assert_eq!(id1, id2);
}

#[test]
fn collaborate_on_identity() {
    let repo = repo();
    let store = IdentityStore::new(&repo);

    // Create and store doc 1
    let (doc1, rev1) = new_user_doc(&store, "T1", &[]);
    let root = &rev1;

    // Create and store doc 2
    let (doc2, rev2) = replace_user_doc(&store, "T2", rev1.clone(), root, &[]);

    // Create and store doc 3
    let (doc3, rev3) = replace_user_doc(&store, "T3", rev2.clone(), root, &[]);

    // Desired history:
    // (id names are id{R}_{B} where R is the doc revision and B is the branch)
    //
    // DOC      BR1      BR2
    //           |        |
    // doc3     id3_1_    |
    //           |    \__ |
    // doc3      |       id3_2
    //           |        |
    // doc2      |      _id2_2
    //           |   __/
    // doc2     id2_1
    //           |
    // doc1     id1

    // Store Doc1 on branch 1
    let id1 = store
        .store_identity(IdentityBuilder::new(rev1.clone(), doc1))
        .unwrap();

    // Store Doc2 on branch 1 (use `with_parent`)
    let id2_1 = store
        .store_identity(IdentityBuilder::with_parent(&id1, rev2.clone(), doc2))
        .unwrap();

    // Store Doc2 on branch 2 taking it from branch 1 (use `duplicate`)
    let id2_2 = store
        .store_identity(IdentityBuilder::duplicate(&id2_1))
        .unwrap();

    // Store Doc3 on branch 2 (use `with_parent`)
    let id3_2 = store
        .store_identity(IdentityBuilder::with_parent(&id2_2, rev3.clone(), doc3))
        .unwrap();

    // Store Doc3 on branch 1 merging it from branch 2 (use `duplicate_other`)
    let id3_1 = store
        .store_identity(IdentityBuilder::duplicate_other(&id2_1, &id3_2))
        .unwrap();

    assert_eq!(id1.root(), root);
    assert_eq!(id1.revision(), &rev1);
    assert_eq!(id1.previous(), None);
    assert_eq!(id1.merged(), None);

    assert_eq!(id2_1.root(), root);
    assert_eq!(id2_1.revision(), &rev2);
    assert_eq!(id2_1.previous(), Some(id1.commit()));
    assert_eq!(id2_1.merged(), None);

    assert_eq!(id2_2.root(), root);
    assert_eq!(id2_2.revision(), &rev2);
    assert_eq!(id2_2.previous(), Some(id2_1.commit()));
    assert_eq!(id2_2.merged(), None);

    assert_eq!(id3_2.root(), root);
    assert_eq!(id3_2.revision(), &rev3);
    assert_eq!(id3_2.previous(), Some(id2_2.commit()));
    assert_eq!(id3_2.merged(), None);

    assert_eq!(id3_1.root(), root);
    assert_eq!(id3_1.revision(), &rev3);
    assert_eq!(id3_1.previous(), Some(id2_1.commit()));
    assert_eq!(id3_1.merged(), Some(id3_2.commit()));

    assert_eq!(store.get_identity(id3_1.commit()).unwrap(), id3_1);
    assert_eq!(store.get_identity(id3_2.commit()).unwrap(), id3_2);
    assert_eq!(store.get_identity(id2_1.commit()).unwrap(), id2_1);
    assert_eq!(store.get_identity(id2_2.commit()).unwrap(), id2_2);
    assert_eq!(store.get_identity(id1.commit()).unwrap(), id1);

    assert_eq!(store.get_parent_identity(&id3_1).unwrap(), id2_1);
    assert_eq!(store.get_parent_identity(&id3_2).unwrap(), id2_2);
    assert_eq!(store.get_parent_identity(&id2_2).unwrap(), id2_1);
    assert_eq!(store.get_parent_identity(&id2_1).unwrap(), id1);

    assert_eq!(store.get_identity(id3_1.merged().unwrap()).unwrap(), id3_2);
}

#[test]
fn check_even_quorum() {
    let repo = repo();
    let store = IdentityStore::new(&repo);

    let (doc, rev) = new_user_doc(
        &store,
        "text",
        &[K1.public(), K2.public(), K3.public(), K4.public()],
    );

    let id0 = store
        .store_identity(IdentityBuilder::new(rev.clone(), doc.clone()))
        .unwrap()
        .check_signatures()
        .unwrap();
    let id1 = store
        .store_identity(IdentityBuilder::new(rev.clone(), doc.clone()).sign(K1.clone()))
        .unwrap()
        .check_signatures()
        .unwrap();
    let id2 = store
        .store_identity(
            IdentityBuilder::new(rev.clone(), doc.clone())
                .sign(K1.clone())
                .sign(K2.clone()),
        )
        .unwrap()
        .check_signatures()
        .unwrap();
    let id3 = store
        .store_identity(
            IdentityBuilder::new(rev.clone(), doc.clone())
                .sign(K1.clone())
                .sign(K2.clone())
                .sign(K3.clone()),
        )
        .unwrap()
        .check_signatures()
        .unwrap();
    let id4 = store
        .store_identity(
            IdentityBuilder::new(rev, doc)
                .sign(K1.clone())
                .sign(K2.clone())
                .sign(K3.clone())
                .sign(K4.clone()),
        )
        .unwrap()
        .check_signatures()
        .unwrap();

    assert!(matches!(id0.check_quorum(), Err(Error::NoCurrentQuorum)));
    assert!(matches!(id1.check_quorum(), Err(Error::NoCurrentQuorum)));
    assert!(matches!(id2.check_quorum(), Err(Error::NoCurrentQuorum)));
    assert!(matches!(id3.check_quorum(), Ok(_)));
    assert!(matches!(id4.check_quorum(), Ok(_)));
}

#[test]
fn check_odd_quorum() {
    let repo = repo();
    let store = IdentityStore::new(&repo);

    let (doc, rev) = new_user_doc(
        &store,
        "text",
        &[
            K1.public(),
            K2.public(),
            K3.public(),
            K4.public(),
            K5.public(),
        ],
    );

    let id0 = store
        .store_identity(IdentityBuilder::new(rev.clone(), doc.clone()))
        .unwrap()
        .check_signatures()
        .unwrap();
    let id1 = store
        .store_identity(IdentityBuilder::new(rev.clone(), doc.clone()).sign(K1.clone()))
        .unwrap()
        .check_signatures()
        .unwrap();
    let id2 = store
        .store_identity(
            IdentityBuilder::new(rev.clone(), doc.clone())
                .sign(K1.clone())
                .sign(K2.clone()),
        )
        .unwrap()
        .check_signatures()
        .unwrap();
    let id3 = store
        .store_identity(
            IdentityBuilder::new(rev.clone(), doc.clone())
                .sign(K1.clone())
                .sign(K2.clone())
                .sign(K3.clone()),
        )
        .unwrap()
        .check_signatures()
        .unwrap();
    let id4 = store
        .store_identity(
            IdentityBuilder::new(rev.clone(), doc.clone())
                .sign(K1.clone())
                .sign(K2.clone())
                .sign(K3.clone())
                .sign(K4.clone()),
        )
        .unwrap()
        .check_signatures()
        .unwrap();
    let id5 = store
        .store_identity(
            IdentityBuilder::new(rev, doc)
                .sign(K1.clone())
                .sign(K2.clone())
                .sign(K3.clone())
                .sign(K4.clone())
                .sign(K5.clone()),
        )
        .unwrap()
        .check_signatures()
        .unwrap();

    assert!(matches!(id0.check_quorum(), Err(Error::NoCurrentQuorum)));
    assert!(matches!(id1.check_quorum(), Err(Error::NoCurrentQuorum)));
    assert!(matches!(id2.check_quorum(), Err(Error::NoCurrentQuorum)));
    assert!(matches!(id3.check_quorum(), Ok(_)));
    assert!(matches!(id4.check_quorum(), Ok(_)));
    assert!(matches!(id5.check_quorum(), Ok(_)));
}

#[test]
fn check_wrong_quorum() {
    let repo = repo();
    let store = IdentityStore::new(&repo);

    let (doc, rev) = new_user_doc(&store, "text", &[K1.public(), K2.public()]);

    let id1 = store
        .store_identity(IdentityBuilder::new(rev.clone(), doc.clone()).sign(K5.clone()))
        .unwrap()
        .check_signatures()
        .unwrap();
    let id2 = store
        .store_identity(
            IdentityBuilder::new(rev.clone(), doc.clone())
                .sign(K4.clone())
                .sign(K5.clone()),
        )
        .unwrap()
        .check_signatures()
        .unwrap();
    let id3 = store
        .store_identity(
            IdentityBuilder::new(rev.clone(), doc.clone())
                .sign(K3.clone())
                .sign(K4.clone())
                .sign(K5.clone()),
        )
        .unwrap()
        .check_signatures()
        .unwrap();
    let id4 = store
        .store_identity(
            IdentityBuilder::new(rev.clone(), doc.clone())
                .sign(K2.clone())
                .sign(K3.clone())
                .sign(K4.clone())
                .sign(K5.clone()),
        )
        .unwrap()
        .check_signatures()
        .unwrap();
    let id5 = store
        .store_identity(
            IdentityBuilder::new(rev, doc)
                .sign(K1.clone())
                .sign(K2.clone())
                .sign(K3.clone())
                .sign(K4.clone())
                .sign(K5.clone()),
        )
        .unwrap()
        .check_signatures()
        .unwrap();

    assert!(matches!(id1.check_quorum(), Err(Error::NoCurrentQuorum)));
    assert!(matches!(id2.check_quorum(), Err(Error::NoCurrentQuorum)));
    assert!(matches!(id3.check_quorum(), Err(Error::NoCurrentQuorum)));
    assert!(matches!(id4.check_quorum(), Err(Error::NoCurrentQuorum)));
    assert!(matches!(id5.check_quorum(), Ok(_)));
}
