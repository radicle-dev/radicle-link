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
