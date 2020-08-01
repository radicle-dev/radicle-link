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
use cache::{test::NullVerificationCache, MemoryCache};

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

fn new_user_doc(
    store: &IdentityStore,
    text: &str,
    keys: &[&SecretKey],
) -> (Doc<Untrusted>, Revision) {
    let mut builder = DocBuilder::new_user();
    for k in keys {
        builder.add_key(k.public()).unwrap();
    }
    let doc = builder.build(Payload::new(text)).unwrap();
    let rev = store.store_doc(&doc, None).unwrap();
    (doc, rev)
}

fn replace_user_doc(
    store: &IdentityStore,
    text: &str,
    replaces: &Revision,
    root: &Revision,
    keys: &[&SecretKey],
) -> (Doc<Untrusted>, Revision) {
    let mut builder = DocBuilder::new_user();
    let builder = builder.replaces(replaces.clone());
    for k in keys {
        builder.add_key(k.public()).unwrap();
    }
    let doc = builder.build(Payload::new(text)).unwrap();
    let rev = store.store_doc(&doc, Some(root)).unwrap();
    (doc, rev)
}

fn new_identity(
    store: &IdentityStore,
    doc: &Doc<Untrusted>,
    rev: &Revision,
    keys: &[&SecretKey],
) -> Identity<Signed> {
    let mut builder = IdentityBuilder::new(rev.clone(), doc.clone());
    for k in keys {
        builder = builder.sign(k);
    }
    store
        .store_identity(builder)
        .unwrap()
        .check_signatures()
        .unwrap()
}

fn with_parent_identity(
    store: &IdentityStore,
    doc: &Doc<Untrusted>,
    rev: &Revision,
    parent: &Identity<Signed>,
    keys: &[&SecretKey],
) -> Identity<Signed> {
    let mut builder = IdentityBuilder::with_parent(parent, rev.clone(), doc.clone());
    for k in keys {
        builder = builder.sign(k);
    }
    store
        .store_identity(builder)
        .unwrap()
        .check_signatures()
        .unwrap()
}

fn duplicate_identity(
    store: &IdentityStore,
    parent: &Identity<Signed>,
    keys: &[&SecretKey],
) -> Identity<Signed> {
    let mut builder = IdentityBuilder::duplicate(parent);
    for k in keys {
        builder = builder.sign(k);
    }
    store
        .store_identity(builder)
        .unwrap()
        .check_signatures()
        .unwrap()
}

fn duplicate_other_identity(
    store: &IdentityStore,
    parent: &Identity<Signed>,
    other: &Identity<Signed>,
    keys: &[&SecretKey],
) -> Identity<Signed> {
    let mut builder = IdentityBuilder::duplicate_other(parent, other);
    for k in keys {
        builder = builder.sign(k);
    }
    store
        .store_identity(builder)
        .unwrap()
        .check_signatures()
        .unwrap()
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
        .store_identity(IdentityBuilder::new(rev, doc).sign(&K1).sign(&K2))
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
    let (doc2, rev2) = replace_user_doc(&store, "T2", &rev1, root, &[]);

    // Create and store doc 3
    let (doc3, rev3) = replace_user_doc(&store, "T3", &rev2, root, &[]);

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

    let (doc, rev) = new_user_doc(&store, "text", &[&K1, &K2, &K3, &K4]);

    let id0 = new_identity(&store, &doc, &rev, &[]);
    let id1 = new_identity(&store, &doc, &rev, &[&K1]);
    let id2 = new_identity(&store, &doc, &rev, &[&K1, &K2]);
    let id3 = new_identity(&store, &doc, &rev, &[&K1, &K2, &K3]);
    let id4 = new_identity(&store, &doc, &rev, &[&K1, &K2, &K3, &K4]);

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

    let (doc, rev) = new_user_doc(&store, "text", &[&K1, &K2, &K3, &K4, &K5]);

    let id0 = new_identity(&store, &doc, &rev, &[]);
    let id1 = new_identity(&store, &doc, &rev, &[&K1]);
    let id2 = new_identity(&store, &doc, &rev, &[&K1, &K2]);
    let id3 = new_identity(&store, &doc, &rev, &[&K1, &K2, &K3]);
    let id4 = new_identity(&store, &doc, &rev, &[&K1, &K2, &K3, &K4]);
    let id5 = new_identity(&store, &doc, &rev, &[&K1, &K2, &K3, &K4, &K5]);

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

    let (doc, rev) = new_user_doc(&store, "text", &[&K1, &K2]);

    let id1 = new_identity(&store, &doc, &rev, &[&K5]);
    let id2 = new_identity(&store, &doc, &rev, &[&K4, &K5]);
    let id3 = new_identity(&store, &doc, &rev, &[&K3, &K4, &K5]);
    let id4 = new_identity(&store, &doc, &rev, &[&K2, &K3, &K4, &K5]);
    let id5 = new_identity(&store, &doc, &rev, &[&K1, &K2, &K3, &K4, &K5]);

    assert!(matches!(id1.check_quorum(), Err(Error::NoCurrentQuorum)));
    assert!(matches!(id2.check_quorum(), Err(Error::NoCurrentQuorum)));
    assert!(matches!(id3.check_quorum(), Err(Error::NoCurrentQuorum)));
    assert!(matches!(id4.check_quorum(), Err(Error::NoCurrentQuorum)));
    assert!(matches!(id5.check_quorum(), Ok(_)));
}

#[test]
fn check_simple_updates() {
    let repo = repo();
    let store = IdentityStore::new(&repo);
    let mut cache = NullVerificationCache::default();

    let (doc1, rev1) = new_user_doc(&store, "T1", &[&K1, &K2]);
    let (doc2, rev2) = replace_user_doc(&store, "T2", &rev1, &rev1, &[&K1, &K2, &K3]);
    let (doc3, rev3) = replace_user_doc(&store, "T3", &rev2, &rev1, &[&K2, &K3]);

    // Desired history:
    // Id names are id{R}_{B}{r} where:
    // - R is the doc revision
    // - B is the branch
    // - r is a sort of release inside the branch
    // Each branch only adds one signature with its own key.
    // Signature sets marked with * are the verifiable ones.
    //
    // DOC    SIG      BR1       BR2       BR3
    //                  |         |         |
    // doc3  *K23       |         |       _id3_3b
    //                  |         |    __/  |
    // doc3  *K23       |        id3_2a_    |
    //                  |         |     \__ |
    // doc3   K3        |         |        id3_3a
    //                  |         |         |
    // doc2  *K123     id2_1b_    |         |
    //                  |     \__ |         |
    // doc2  *K123      |        id2_2b_    |
    //                  |         |     \__ |
    // doc2  *K123      |         |       _id2_3a
    //                  |         |    __/
    // doc2  *K12       |       _id2_2a
    //                  |    __/  |
    // doc2   K1       id2_1a     |
    //                  |         |
    // doc1  *K12      id1_1b_    |
    //                  |     \__ |
    // doc1  *K12       |       _id1_2a
    //                  |    __/
    // doc1   K1       id1_1a

    let id1_1a = new_identity(&store, &doc1, &rev1, &[&K1]);
    let id1_2a = duplicate_identity(&store, &id1_1a, &[&K2]);
    let id1_1b = duplicate_other_identity(&store, &id1_1a, &id1_2a, &[]);
    let id2_1a = with_parent_identity(&store, &doc2, &rev2, &id1_1b, &[&K1]);
    let id2_2a = duplicate_other_identity(&store, &id1_2a, &id2_1a, &[&K2]);
    let id2_3a = duplicate_identity(&store, &id2_2a, &[&K3]);
    let id2_2b = duplicate_other_identity(&store, &id2_2a, &id2_3a, &[]);
    let id2_1b = duplicate_other_identity(&store, &id2_1a, &id2_2b, &[]);
    let id3_3a = with_parent_identity(&store, &doc3, &rev3, &id2_3a, &[&K3]);
    let id3_2a = duplicate_other_identity(&store, &id2_2b, &id3_3a, &[&K2]);
    let id3_3b = duplicate_other_identity(&store, &id3_3a, &id3_2a, &[]);

    assert!(matches!(id1_1a.check_quorum(), Err(Error::NoCurrentQuorum)));
    assert!(matches!(id2_1a.check_quorum(), Err(Error::NoCurrentQuorum)));
    assert!(matches!(id3_3a.check_quorum(), Err(Error::NoCurrentQuorum)));

    let id1_2a = id1_2a
        .check_quorum()
        .unwrap()
        .check_update(None, &mut cache)
        .unwrap();
    let id1_1b = id1_1b
        .check_quorum()
        .unwrap()
        .check_update(None, &mut cache)
        .unwrap();
    let _id2_2a = id2_2a
        .check_quorum()
        .unwrap()
        .check_update(Some(&id1_2a), &mut cache)
        .unwrap();
    let id2_3a = id2_3a
        .check_quorum()
        .unwrap()
        .check_update(Some(&id1_2a), &mut cache)
        .unwrap();
    let id2_2b = id2_2b
        .check_quorum()
        .unwrap()
        .check_update(Some(&id1_2a), &mut cache)
        .unwrap();
    let id2_1b = id2_1b
        .check_quorum()
        .unwrap()
        .check_update(Some(&id1_1b), &mut cache)
        .unwrap();
    let id3_2a = id3_2a
        .check_quorum()
        .unwrap()
        .check_update(Some(&id2_2b), &mut cache)
        .unwrap();
    let id3_3b = id3_3b
        .check_quorum()
        .unwrap()
        .check_update(Some(&id2_3a), &mut cache)
        .unwrap();

    let (b1_head, b1_verified) = store
        .get_latest_identity(id2_1b.commit(), &mut cache)
        .unwrap();
    assert_eq!(b1_head.commit(), id2_1b.commit());
    assert_eq!(b1_verified.unwrap().commit(), id2_1b.commit());

    let (b2_head, b2_verified) = store
        .get_latest_identity(id3_2a.commit(), &mut cache)
        .unwrap();
    assert_eq!(b2_head.commit(), id3_2a.commit());
    assert_eq!(b2_verified.unwrap().commit(), id3_2a.commit());

    let (b3_head, b3_verified) = store
        .get_latest_identity(id3_3b.commit(), &mut cache)
        .unwrap();
    assert_eq!(b3_head.commit(), id3_3b.commit());
    assert_eq!(b3_verified.unwrap().commit(), id3_3b.commit());

    let mut cache = MemoryCache::default();

    let (b1_head, b1_verified) = store
        .get_latest_identity(id2_1b.commit(), &mut cache)
        .unwrap();
    assert_eq!(b1_head.commit(), id2_1b.commit());
    assert_eq!(b1_verified.unwrap().commit(), id2_1b.commit());

    let (b2_head, b2_verified) = store
        .get_latest_identity(id3_2a.commit(), &mut cache)
        .unwrap();
    assert_eq!(b2_head.commit(), id3_2a.commit());
    assert_eq!(b2_verified.unwrap().commit(), id3_2a.commit());

    let (b3_head, b3_verified) = store
        .get_latest_identity(id3_3b.commit(), &mut cache)
        .unwrap();
    assert_eq!(b3_head.commit(), id3_3b.commit());
    assert_eq!(b3_verified.unwrap().commit(), id3_3b.commit());
}
