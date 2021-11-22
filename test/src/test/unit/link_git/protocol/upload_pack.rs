// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use link_git::protocol::upload_pack;

mod header {
    use super::*;
    use std::str::FromStr as _;

    #[test]
    fn service_must_be_upload_pack() {
        assert_eq!(
            upload_pack::Header::from_str("git-receive-pack "),
            Err("unsupported service")
        )
    }

    #[test]
    fn no_path() {
        assert_eq!(
            upload_pack::Header::from_str("git-upload-pack "),
            Err("missing path")
        )
    }

    #[test]
    fn empty_path() {
        assert_eq!(
            upload_pack::Header::from_str("git-upload-pack \0host=lolhost:123\0"),
            Err("empty path")
        )
    }

    #[test]
    fn host_and_port() {
        assert_eq!(
            upload_pack::Header::from_str("git-upload-pack /git.git\0host=lolhost:123\0").unwrap(),
            upload_pack::Header {
                path: "/git.git".to_owned(),
                host: Some(("lolhost".to_owned(), Some(123))),
                extra: vec![]
            }
        )
    }

    #[test]
    fn host_without_port() {
        assert_eq!(
            upload_pack::Header::from_str("git-upload-pack /git.git\0host=lolhost\0").unwrap(),
            upload_pack::Header {
                path: "/git.git".to_owned(),
                host: Some(("lolhost".to_owned(), None)),
                extra: vec![]
            }
        )
    }

    #[test]
    fn no_host() {
        assert_eq!(
            upload_pack::Header::from_str("git-upload-pack /git.git\0").unwrap(),
            upload_pack::Header {
                path: "/git.git".to_owned(),
                host: None,
                extra: vec![]
            }
        )
    }

    #[test]
    fn empty_host() {
        assert_eq!(
            upload_pack::Header::from_str("git-upload-pack /git.git\0\0").unwrap(),
            upload_pack::Header {
                path: "/git.git".to_owned(),
                host: None,
                extra: vec![]
            }
        )
    }

    #[test]
    fn no_host_extra() {
        assert_eq!(
            upload_pack::Header::from_str("git-upload-pack /git.git\0\0version=42\0").unwrap(),
            upload_pack::Header {
                path: "/git.git".to_owned(),
                host: None,
                extra: vec![("version".to_owned(), Some("42".to_owned()))]
            }
        )
    }

    #[test]
    fn host_port_extra() {
        assert_eq!(
            upload_pack::Header::from_str(
                "git-upload-pack /git.git\0host=lolhost:123\0\0version=42\0"
            )
            .unwrap(),
            upload_pack::Header {
                path: "/git.git".to_owned(),
                host: Some(("lolhost".to_owned(), Some(123))),
                extra: vec![("version".to_owned(), Some("42".to_owned()))]
            }
        )
    }

    #[test]
    fn host_extra_extra() {
        assert_eq!(
            upload_pack::Header::from_str(
                "git-upload-pack /git.git\0host=lolhost\0\0version=42\0foo\0n=69\0"
            )
            .unwrap(),
            upload_pack::Header {
                path: "/git.git".to_owned(),
                host: Some(("lolhost".to_owned(), None)),
                extra: vec![
                    ("version".to_owned(), Some("42".to_owned())),
                    ("foo".to_owned(), None),
                    ("n".to_owned(), Some("69".to_owned()))
                ]
            }
        )
    }
}
