// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use librad::git::refs::Remotes;

mod remotes {
    use super::*;

    use pretty_assertions::assert_eq;
    use serde_json::json;
    lazy_static! {
        static ref REMOTES: Remotes<String> = serde_json::from_value(json!({
            "lolek": {
                "bolek": {
                    "tola": {
                        "alice": {
                            "bob": {
                                "carol": {
                                    "dylan": {}
                                }
                            }
                        }
                    }
                }
            },
            "alice": {
                "bob": {
                    "carol": {
                        "dylan": {}
                    }
                }
            }
        }))
        .unwrap();
    }

    #[test]
    fn so_many_levels_of_remoteness() {
        assert!(REMOTES["lolek"]["bolek"]["tola"]["alice"]["bob"]["carol"].contains_key("dylan"));
        assert!(REMOTES["alice"]["bob"]["carol"].contains_key("dylan"))
    }

    #[test]
    fn deep_flatten() {
        assert_eq!(
            vec![
                "alice", "bob", "carol", "dylan", "lolek", "bolek", "tola", "alice", "bob",
                "carol", "dylan"
            ],
            REMOTES.flatten().collect::<Vec<_>>()
        )
    }

    #[test]
    fn cutoff() {
        assert_eq!(
            json!({
                "lolek": {
                    "bolek": {
                        "tola": {}
                    }
                },
                "alice": {
                    "bob": {
                        "carol": {}
                    }
                }
            }),
            serde_json::to_value(REMOTES.clone().cutoff(3)).unwrap()
        )
    }

    #[test]
    fn cutoff_mut() {
        let mut remotes = REMOTES.clone();
        remotes.cutoff_mut(3);

        assert_eq!(
            json!({
                "lolek": {
                    "bolek": {
                        "tola": {}
                    }
                },
                "alice": {
                    "bob": {
                        "carol": {}
                    }
                }
            }),
            serde_json::to_value(remotes).unwrap()
        )
    }
}

mod verifying_refs {
    // These tests reproduce the issue described in
    // <https://lists.sr.ht/~radicle-link/dev/%3CCAH_DpYRRkDEpQB%3DVa%3DhHVaubbk8A2XR-tFzB7RDWzyzsT5Vpew%40mail.gmail.com%3E
    //
    // To review: we added a key to the signed_refs blob that old codebases
    // didn't know to include in the canonical form when verifying signatures.
    // The fix has been to loosen the requirements on the structure of the
    // signed_refs blob. These tests now serve to ensure that we have some
    // confidence that we are not making backwards incompatible changes in
    // future

    use librad::{git::refs::Signed, PeerId};

    #[test]
    fn refs_without_cobs() {
        let peer: PeerId = "hyd5xjym8y5osfzhrnwh4fosby8788t1sqdz1mq1bkdq5de35hu9eq"
            .parse()
            .unwrap();
        let json = serde_json::json!({
            "refs": {
                "heads": {},
                "rad": {
                    "id": "e34d441552ba94507897654ce5b7094fa2ee894b"
                },
                "tags": {},
                "notes": {},
                "remotes": {}
            },
            "signature": "hyyu4ubnbsb9xftnq1gibsgu3oda6fcem6wqgomqefe43pmsa81653y55pahr4xotidh9wfuxiwgccxeig4wr884ei3pg1e5isaencoox"
        });
        Signed::from_json(&serde_json::to_vec(&json).unwrap(), &peer).unwrap();
    }

    #[test]
    fn refs_with_cobs() {
        let peer = "hyn3pfi96bfpbx5dnsbmfi15grtimfet5hpr86mmzdxunz5wy5frss"
            .parse()
            .unwrap();
        let json = serde_json::json!({
          "refs": {
            "heads": {
              "main": "dcf932a7aae2a74e7c8a6166df2aa295b4221235"
            },
            "rad": {
              "id": "cebe1d24b890074059bed32fa81ded6646eef862"
            },
            "tags": {},
            "notes": {},
            "cob": {
              "xyz.example.radicle.issue/hnrkji98yc63m8b4m133fbhbjqwhfcm1zqpmo": "9afce067b2b3874b967250f0297538562e577357"
            },
            "remotes": {}
          },
          "signature": "hyy41chktz6sin646k6p3d3863hkfer9ih76xcss7tjuk8ooxn7ju5sntwrih77xyni4pricaz86191y54sk5cxmh19e7gqzmo8f8xaaj"
        });
        Signed::from_json(&serde_json::to_vec(&json).unwrap(), &peer).unwrap();
    }

    #[test]
    fn refs_with_unknown_category() {
        let peer = "hyndhteicad3puwto5s5re1bjjjx487dbjfxwng9hpcjspcbyw9cjn"
            .parse()
            .unwrap();
        let json = serde_json::json!({
            "refs": {
                "heads": {
                  "main": "dcf932a7aae2a74e7c8a6166df2aa295b4221235"
                },
                "rad": {
                  "id": "cebe1d24b890074059bed32fa81ded6646eef862"
                },
                "tags": {},
                "notes": {},
                "cob": {},
                "somecategory": {
                    "someref":  "9afce067b2b3874b967250f0297538562e577357"
                },
                "remotes": {}
            },
            "signature": "hynbjhkupc4hcatkhmrw57bmqwakkf7tmd87z39amq8uw8qjy95fg6irnkga6sqfa9bbz75njqz8r7ag6dfrc94wcq1krd8kikpxcguan"
        });
        Signed::from_json(&serde_json::to_vec(&json).unwrap(), &peer).unwrap();
    }
}

mod computing_refs {
    use librad::{
        git::{refs::Refs, types::Namespace, Storage},
        paths::Paths,
        reflike,
        PeerId,
        SecretKey,
    };
    use radicle_git_ext::name as reference;

    use crate::{make_refs, rad::identities::TestProject};

    macro_rules! assert_refs {
        ($iter: expr, [$($ref_str:literal => $expected_oid:expr)*]) => {
            assert_eq!(
                $iter.collect::<Vec<(reference::OneLevel, radicle_git_ext::Oid)>>(),
                vec![$((
                    reference::OneLevel::from(reflike!($ref_str)),
                    $expected_oid.into()
                )),+],
            )
        }
    }

    #[test]
    fn test_compute_valid_refs() {
        // Create a project and some storage
        let project_dir = tempfile::TempDir::new().unwrap();
        let paths = Paths::from_root(project_dir.path()).unwrap();
        let key = SecretKey::new();
        let storage = Storage::open(&paths, key).unwrap();
        let project = TestProject::create(&storage).unwrap();

        // Get raw access to the underlying repo so we can forcibly create some
        // references
        let raw_repo = git2::Repository::open(paths.git_dir()).unwrap();

        // Create a bunch of blobs to create references to
        let some_head = raw_repo.blob("some head".as_bytes()).unwrap();
        let some_note = raw_repo.blob("some note".as_bytes()).unwrap();
        let some_tag = raw_repo.blob("some tag".as_bytes()).unwrap();
        let some_cob = raw_repo.blob("some cob".as_bytes()).unwrap();
        let some_radid = raw_repo.blob("radid".as_bytes()).unwrap();
        let some_otherblob = raw_repo.blob("some other blob".as_bytes()).unwrap();
        let signed_refs = raw_repo.blob("signedrefs".as_bytes()).unwrap();
        let unused_remote = raw_repo.blob("remote".as_bytes()).unwrap();

        let remote_peer: PeerId = SecretKey::new().into();
        let remote_name = format!("remotes/{}/rad/id", remote_peer);

        // Create references in the places the Refs expects to look when computing
        // itself
        let namespace: Namespace<radicle_git_ext::Oid> = project.project.urn().into();
        let namespace_prefix = format!("refs/namespaces/{}/refs/", namespace);
        let refs_to_create = vec![
            ("heads/somehead", some_head),
            ("notes/somenote", some_note),
            ("tags/sometag", some_tag),
            ("cobs/somecob", some_cob),
            ("rad/id", some_radid),
            ("rad/signed_refs", signed_refs),
            ("somecategory/something", some_otherblob),
            (remote_name.as_str(), unused_remote),
        ];

        for (category, target) in refs_to_create {
            raw_repo
                .reference(
                    format!("{}{}", namespace_prefix, category).as_str(),
                    target,
                    true,
                    "",
                )
                .unwrap();
        }

        // Now assert that the computed refs are the ones we created
        let refs = Refs::compute(&storage, &project.project.urn()).unwrap();
        assert_refs!(refs.heads(), ["somehead" => some_head]);
        assert_refs!(refs.rad(), ["id" => some_radid]);
        assert_refs!(refs.tags(), ["sometag" => some_tag]);
        assert_refs!(refs.notes(), ["somenote" => some_note]);
        assert_refs!(refs.cobs(), ["somecob" => some_cob]);
        let uncategorised_refs: Vec<(&str, &str, radicle_git_ext::Oid)> =
            refs.other_refs().collect();
        assert_eq!(
            uncategorised_refs,
            vec![("somecategory", "something", some_otherblob.into())]
        );

        let expected_refs = make_refs! {
            "heads" => {"somehead" => some_head,},
            "rad" => {"id" => some_radid,},
            "tags" => {"sometag" => some_tag,},
            "notes" => {"somenote" => some_note,},
            "cobs" => {"somecob" => some_cob,},
            "somecategory" => {"something" => some_otherblob,},
        };
        assert_eq!(refs.categorised_refs, expected_refs);
    }
}
