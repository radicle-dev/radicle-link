// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    collections::{BTreeSet, HashSet},
    fmt::Debug,
};

use bstr::ByteSlice as _;
use link_crypto::PeerId;
use link_git::protocol::oid;

use crate::{
    error::{self, Validation},
    refs,
    sigrefs,
    LocalPeer,
    RefScan,
};

#[tracing::instrument(level = "debug", skip(cx, sigrefs), err)]
pub fn validate<'a, C, Oid>(
    cx: &'a C,
    sigrefs: &'a sigrefs::Combined<Oid>,
) -> Result<Vec<error::Validation>, <&'a C as RefScan>::Error>
where
    C: LocalPeer,
    &'a C: RefScan,
    Oid: Debug + AsRef<oid>,
{
    use refs::component::*;

    let mut fail = Vec::new();

    info!(?sigrefs, "validating");

    let local_id = LocalPeer::id(cx);

    // signed refs
    for (peer, refs) in &sigrefs.refs {
        if peer == local_id {
            continue;
        }

        let mut seen_refs = HashSet::new();
        let mut seen_rad_id = false;
        let mut seen_sigrefs = false;

        let prefix = format!("refs/remotes/{}", peer);
        info!("scanning {} for signed refs", prefix);

        for item in RefScan::scan(cx, prefix)? {
            let (name, oid) = item?;

            trace!("{}", name);

            if name.ends_with(b"rad/id") {
                seen_rad_id = true;
            } else if name.ends_with(b"rad/signed_refs") {
                seen_sigrefs = true;
            }

            let owned = refs::owned(name.as_bstr());
            // XXX: Should rad/self actually be signed?
            if owned.as_ref() != refs::RadId.as_bytes()
                && owned.starts_with(refs::Prefix::Rad.as_bytes())
            {
                continue;
            }
            match refs.refs.get(owned.as_ref()) {
                None => fail.push(Validation::Unexpected(name)),
                Some(signed_oid) => {
                    seen_refs.insert(owned.as_ref().to_owned());

                    if signed_oid.as_ref() != oid.as_ref() {
                        fail.push(Validation::MismatchedTips {
                            signed: signed_oid.as_ref().to_owned(),
                            actual: oid.into(),
                            name,
                        })
                    }
                },
            }
        }

        for missing in refs
            .refs
            .keys()
            .filter(|k| !seen_refs.contains(k.as_bstr()))
        {
            fail.push(Validation::Missing {
                refname: (*missing).to_owned(),
                remote: *peer,
            })
        }

        if !seen_rad_id {
            fail.push(Validation::MissingRadId(*peer))
        }

        if !seen_sigrefs {
            fail.push(Validation::MissingSigRefs(*peer))
        }
    }

    // unsigned remote tracking
    {
        use either::Either::*;
        use refs::parsed::{Cat, Identity, Rad, Refs};

        let mut seen_peers = BTreeSet::new();
        for peer in &sigrefs.remotes {
            if peer == local_id {
                continue;
            }

            let mut seen_rad_id = false;
            let mut seen_sigrefs = false;

            let prefix = format!("refs/remotes/{}", peer);
            info!(%prefix, "scanning for unsigned trackings");

            for item in RefScan::scan(cx, prefix)? {
                let (name, _oid) = item?;

                trace!("{}", name);

                let owned = refs::owned(name.as_bstr());
                trace!("owned {}", owned.as_ref());
                match refs::parse::<Identity>(owned.as_ref()) {
                    None => fail.push(Validation::Strange(name)),
                    Some(refs::Parsed { inner, .. }) => match inner {
                        Left(Rad::Id) => {
                            seen_rad_id = true;
                        },

                        Left(Rad::SignedRefs) => {
                            seen_sigrefs = true;
                        },

                        Right(Refs {
                            cat: Cat::Unknown(_),
                            ..
                        }) => {
                            fail.push(Validation::Strange(name));
                        },

                        _ => {},
                    },
                }

                seen_peers.insert(peer);
            }

            if !seen_rad_id {
                fail.push(Validation::MissingRadId(*peer))
            }

            if !seen_sigrefs {
                fail.push(Validation::MissingSigRefs(*peer))
            }
        }

        for missing in sigrefs
            .remotes
            .iter()
            .filter(|p| p != &local_id && !seen_peers.contains(p))
        {
            fail.push(Validation::NoData(*missing))
        }
    }

    // finally, find orphans and other strange refs
    {
        let pids = sigrefs
            .refs
            .keys()
            .chain(sigrefs.remotes.iter())
            .filter(|id| id != &local_id)
            .collect::<BTreeSet<_>>();

        info!(?pids, "scanning for orphans and strange refs");

        for item in RefScan::scan::<_, String>(cx, None)? {
            let (name, _oid) = item?;

            trace!("{}", name);

            let strange = match name.splitn(4, refs::is_separator).collect::<Vec<_>>()[..] {
                [REFS, REMOTES, id, _] => {
                    let pid = std::str::from_utf8(id)
                        .ok()
                        .and_then(|s| s.parse::<PeerId>().ok());
                    match pid {
                        None => true,
                        Some(pid) => !pids.contains(&pid),
                    }
                },

                [REFS, RAD, ..] | [REFS, HEADS, ..] | [REFS, NOTES, ..] | [REFS, TAGS, ..] => false,
                _ => true,
            };

            if strange {
                fail.push(Validation::StrangeOrPrunable(name))
            }
        }
    }

    Ok(fail)
}
