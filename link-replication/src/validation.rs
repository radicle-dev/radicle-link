// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    collections::{BTreeSet, HashSet},
    convert::TryFrom,
    fmt::Debug,
};

use git_ref_format::{name, Qualified};
use link_crypto::PeerId;
use link_git::protocol::oid;

use crate::{
    error::{self, Validation},
    refdb,
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
            let refdb::Ref {
                name, peeled: oid, ..
            } = item?;

            trace!("{}", name);

            if name.ends_with("rad/id") {
                seen_rad_id = true;
            } else if name.ends_with("rad/signed_refs") {
                seen_sigrefs = true;
            }

            let owned = match refs::owned(name.clone()) {
                Some(owned) => owned,
                None => continue,
            };
            // XXX: Should rad/self actually be signed?
            if owned.as_ref() != name::REFS_RAD_ID && owned.starts_with(refs::Prefix::Rad.as_str())
            {
                continue;
            }
            match refs.refs.get(owned.as_ref()) {
                None => fail.push(Validation::Unexpected(name.into())),
                Some(signed_oid) => {
                    seen_refs.insert(owned.as_ref().to_owned());

                    if signed_oid.as_ref() != oid.as_ref() {
                        fail.push(Validation::MismatchedTips {
                            signed: signed_oid.as_ref().to_owned(),
                            actual: oid.into(),
                            name: name.into(),
                        })
                    }
                },
            }
        }

        for missing in refs
            .refs
            .keys()
            .filter(|k| !seen_refs.contains(k.as_refstr()))
        {
            fail.push(Validation::Missing {
                refname: missing.to_owned(),
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
        use refs::parsed::{Identity, Rad};

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
                let refdb::Ref { name, .. } = item?;

                trace!("{}", name);

                let owned = match refs::owned(name.clone()) {
                    Some(owned) => owned,
                    None => {
                        fail.push(Validation::Strange(name.into_refstring()));
                        continue;
                    },
                };
                match refs::Parsed::<Identity>::try_from(Qualified::from(owned)) {
                    Err(_) => fail.push(Validation::Strange(name.into_refstring())),
                    Ok(refs::Parsed { inner, .. }) => match inner {
                        Left(Rad::Id) => {
                            seen_rad_id = true;
                        },

                        Left(Rad::SignedRefs) => {
                            seen_sigrefs = true;
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
            let refdb::Ref { name, .. } = item?;

            trace!("{}", name);

            let strange = match name.iter().take(4).collect::<Vec<_>>()[..] {
                [name::str::REFS, name::str::REMOTES, id, _] => {
                    let pid = id.parse::<PeerId>().ok();
                    match pid {
                        None => true,
                        Some(pid) => !pids.contains(&pid),
                    }
                },

                [name::str::REFS, name::str::RAD, ..]
                | [name::str::REFS, name::str::HEADS, ..]
                | [name::str::REFS, name::str::NOTES, ..]
                | [name::str::REFS, name::str::TAGS, ..] => false,
                _ => true,
            };

            if strange {
                fail.push(Validation::StrangeOrPrunable(name.into_refstring()))
            }
        }
    }

    Ok(fail)
}
