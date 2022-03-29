// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    path::PathBuf,
    str::FromStr,
};

use anyhow::Result;
use clap::Parser as _;

use librad::{
    net::Network,
    profile::{LnkHome, ProfileId},
};

use linkd_lib::args::{
    self,
    Args,
    KeyArgs,
    MetricsArgs,
    MetricsProvider,
    ProtocolArgs,
    ProtocolListen,
    Signer,
    TrackingArgs,
    TrackingMode,
};
use lnk_clib::seed::Seed;

#[test]
fn defaults() -> Result<()> {
    #[rustfmt::skip]
    let iter = vec![
        "linkd",
            "--protocol-listen", "localhost",
    ];
    let parsed = Args::try_parse_from(iter)?;

    assert_matches!(
        parsed,
        Args {
            lnk_home: LnkHome::ProjectDirs,
            ..
        }
    );
    assert_eq!(
        parsed,
        Args {
            ..Default::default()
        }
    );

    Ok(())
}

#[test]
fn bootstraps() -> Result<()> {
    let bootstraps = vec![
        Seed {
            addrs: "sprout.radicle.xyz:12345".to_string(),
            peer: "hynkyndc6w3p8urucakobzna7sxwgcqny7xxtw88dtx3pkf7m3nrzc".parse()?,
            label: None,
        },
        Seed {
            addrs: "setzling.radicle.xyz:12345".to_string(),
            peer: "hybz9gfgtd9d4pd14a6r66j5hz6f77fed4jdu7pana4fxaxbt369kg".parse()?,
            label: None,
        },
    ];

    #[rustfmt::skip]
    let iter = vec![
        "linkd",
            "--protocol-listen", "localhost",
            "--bootstrap", "hynkyndc6w3p8urucakobzna7sxwgcqny7xxtw88dtx3pkf7m3nrzc@sprout.radicle.xyz:12345",
            "--bootstrap", "hybz9gfgtd9d4pd14a6r66j5hz6f77fed4jdu7pana4fxaxbt369kg@setzling.radicle.xyz:12345",
    ];
    let parsed = Args::try_parse_from(iter)?;

    assert_eq!(
        parsed,
        Args {
            bootstraps,
            ..Default::default()
        }
    );

    Ok(())
}

#[test]
fn metrics_graphite() -> Result<()> {
    #[rustfmt::skip]
    let iter = vec![
        "linkd",
            "--protocol-listen", "localhost",
            "--metrics-provider", "graphite",
            "--graphite-addr", "graphite:9108",
    ];
    let parsed = Args::try_parse_from(iter)?;

    assert_eq!(
        parsed,
        Args {
            metrics: MetricsArgs {
                provider: Some(MetricsProvider::Graphite),
                graphite_addr: "graphite:9108".to_string(),
            },
            ..Default::default()
        }
    );

    Ok(())
}

#[test]
fn profile_id() -> Result<()> {
    let id = ProfileId::new();

    #[rustfmt::skip]
    let iter = vec![
        "linkd",
            "--protocol-listen", "localhost",
            "--profile-id", id.as_str()
    ];
    let parsed = Args::try_parse_from(iter)?;

    assert_eq!(
        parsed,
        Args {
            profile_id: Some(id),
            ..Default::default()
        }
    );

    Ok(())
}

#[test]
fn protocol_listen() -> Result<()> {
    #[rustfmt::skip]
    let iter = vec![
        "linkd",
            "--protocol-listen", "127.0.0.1:12345",
    ];
    let parsed = Args::try_parse_from(iter)?;

    assert_eq!(
        parsed,
        Args {
            protocol: ProtocolArgs {
                listen: ProtocolListen::Provided {
                    addr: SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 12345))
                },
                ..Default::default()
            },
            ..Default::default()
        }
    );

    Ok(())
}

#[test]
fn protocol_network() -> Result<()> {
    #[rustfmt::skip]
    let iter = vec![
        "linkd",
            "--protocol-listen", "localhost",
            "--protocol-network", "testnet",
    ];
    let parsed = Args::try_parse_from(iter)?;

    assert_eq!(
        parsed,
        Args {
            protocol: ProtocolArgs {
                network: Network::from_str("testnet").unwrap(),
                ..Default::default()
            },
            ..Default::default()
        }
    );

    Ok(())
}

#[test]
fn lnk_home() -> Result<()> {
    #[rustfmt::skip]
    let iter = vec![
        "linkd",
            "--protocol-listen", "localhost",
            "--lnk-home", "/tmp/linkd",
    ];
    let parsed = Args::try_parse_from(iter)?;

    assert_eq!(
        parsed,
        Args {
            lnk_home: LnkHome::Root(PathBuf::from("/tmp/linkd")),
            ..Default::default()
        }
    );

    Ok(())
}

#[test]
fn signer_key_file() -> Result<()> {
    #[rustfmt::skip]
    let iter = vec![
        "linkd",
            "--protocol-listen", "localhost",
            "--signer", "key",
            "--key-source", "file",
            "--key-file-path", "~/.config/radicle/secret.key",
    ];
    let parsed = Args::try_parse_from(iter)?;
    assert_eq!(
        parsed,
        Args {
            signer: args::Signer::Key,
            key: KeyArgs {
                source: args::KeySource::File,
                file_path: Some(PathBuf::from("~/.config/radicle/secret.key")),
                ..Default::default()
            },
            ..Default::default()
        }
    );

    #[rustfmt::skip]
    let iter = vec![
        "linkd",
            "--protocol-listen", "localhost",
            "--signer", "key",
            "--key-format", "base64",
            "--key-source", "file",
            "--key-file-path", "~/.config/radicle/secret.seed",
    ];
    let parsed = Args::try_parse_from(iter)?;
    assert_eq!(
        parsed,
        Args {
            signer: args::Signer::Key,
            key: KeyArgs {
                format: args::KeyFormat::Base64,
                source: args::KeySource::File,
                file_path: Some(PathBuf::from("~/.config/radicle/secret.seed")),
            },
            ..Default::default()
        }
    );

    Ok(())
}

#[test]
fn signer_key_ephemeral() -> Result<()> {
    #[rustfmt::skip]
    let iter = vec![
        "linkd",
            "--protocol-listen", "localhost",
            "--signer", "key",
            "--key-source", "ephemeral",
    ];
    let parsed = Args::try_parse_from(iter)?;
    assert_eq!(
        parsed,
        Args {
            signer: args::Signer::Key,
            key: KeyArgs {
                source: args::KeySource::Ephemeral,
                ..Default::default()
            },
            ..Default::default()
        }
    );

    #[rustfmt::skip]
    let iter = vec![
        "linkd",
            "--protocol-listen", "localhost",
            "--signer", "key",
            "--key-format", "base64",
            "--key-source", "file",
            "--key-file-path", "~/.config/radicle/secret.seed",
    ];
    let parsed = Args::try_parse_from(iter)?;
    assert_eq!(
        parsed,
        Args {
            signer: args::Signer::Key,
            key: KeyArgs {
                format: args::KeyFormat::Base64,
                source: args::KeySource::File,
                file_path: Some(PathBuf::from("~/.config/radicle/secret.seed")),
            },
            ..Default::default()
        }
    );

    Ok(())
}

#[test]
fn signer_key_stdin() -> Result<()> {
    #[rustfmt::skip]
    let iter = vec![
        "linkd",
            "--protocol-listen", "localhost",
            "--signer", "key",
            "--key-source", "stdin",
    ];
    let parsed = Args::try_parse_from(iter)?;

    assert_eq!(
        parsed,
        Args {
            signer: args::Signer::Key,
            key: KeyArgs {
                source: args::KeySource::Stdin,
                ..Default::default()
            },
            ..Default::default()
        }
    );

    Ok(())
}

#[test]
fn signer_ssh_agent() -> Result<()> {
    #[rustfmt::skip]
    let iter = vec![
        "linkd",
            "--protocol-listen", "localhost",
            "--signer", "ssh-agent",
    ];
    let parsed = Args::try_parse_from(iter)?;

    assert_eq!(
        parsed,
        Args {
            signer: Signer::SshAgent,
            ..Default::default()
        }
    );

    Ok(())
}

#[test]
fn tmp_root() -> Result<()> {
    #[rustfmt::skip]
    let iter = vec![
        "linkd",
            "--protocol-listen", "localhost",
            "--tmp-root",
    ];
    let parsed = Args::try_parse_from(iter)?;

    assert_eq!(
        parsed,
        Args {
            tmp_root: true,
            ..Default::default()
        }
    );

    Ok(())
}

#[test]
fn tracking() -> Result<()> {
    #[rustfmt::skip]
    let parsed = Args::try_parse_from(vec![
        "linkd",
            "--protocol-listen", "localhost",
            "--track", "everything",
    ])?;
    assert_eq!(
        parsed,
        Args {
            tracking: TrackingArgs {
                mode: Some(TrackingMode::Everything),
                ..Default::default()
            },
            ..Default::default()
        }
    );

    #[rustfmt::skip]
    let parsed = Args::try_parse_from(vec![
        "linkd",
            "--protocol-listen", "localhost",
            "--track", "selected",
            "--track-peer-id", "hynkyndc6w3p8urucakobzna7sxwgcqny7xxtw88dtx3pkf7m3nrzc",
            "--track-urn", "rad:git:hnrkb39fr6f4jj59nfiq7tfd9aznirdu7b59o",
            "--track-pair", "hyy9umf1p11g8o8b6qcs1risghw8yoau79oi88hg7fneotchry6r5w,rad:git:hnrkqdpm9ub19oc8dccx44echy76hzfsezyio",
    ])?;
    assert_eq!(
        parsed,
        Args {
            tracking: TrackingArgs {
                mode: Some(TrackingMode::Selected),
                peer_ids: vec!["hynkyndc6w3p8urucakobzna7sxwgcqny7xxtw88dtx3pkf7m3nrzc".parse()?,],
                urns: vec!["rad:git:hnrkb39fr6f4jj59nfiq7tfd9aznirdu7b59o".parse()?],
                pairs: vec!["hyy9umf1p11g8o8b6qcs1risghw8yoau79oi88hg7fneotchry6r5w,rad:git:hnrkqdpm9ub19oc8dccx44echy76hzfsezyio".parse()?],
            },
            ..Default::default()
        }
    );

    Ok(())
}
