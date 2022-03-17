// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{collections::HashSet, convert::TryFrom, fmt::Debug};

use either::Either;
use itertools::Itertools as _;
use link_crypto::PeerId;
use link_git::protocol::oid;

use crate::{error, ids, refdb, refs, sigrefs::Refs, RefScan};

pub fn validate<'a, U, S, P, O>(
    scan: S,
    id: P,
    refs: &Refs<O>,
) -> Result<Vec<error::Validation>, S::Error>
where
    U: ids::Urn + Clone + Debug,
    S: RefScan,
    P: Into<Option<&'a PeerId>>,
    O: AsRef<oid> + Debug,
{
    let id = id.into();
    let tree = SigTree { id, refs };
    match id {
        None => {
            let iter = scan
                .scan::<_, String>(None)?
                .filter_ok(|x| !x.name.starts_with("refs/remotes"));
            tree.validate::<U, _, _, _>(iter)
        },
        Some(id) => {
            let iter = scan.scan(format!("refs/remotes/{}", id))?;
            tree.validate::<U, _, _, _>(iter)
        },
    }
}

struct SigTree<'a, Oid> {
    id: Option<&'a PeerId>,
    refs: &'a Refs<Oid>,
}

impl<'a, Oid> SigTree<'a, Oid>
where
    Oid: AsRef<oid> + Debug,
{
    fn validate<U, O, I, E>(&self, iter: I) -> Result<Vec<error::Validation>, E>
    where
        U: ids::Urn + Clone + Debug,
        O: AsRef<oid>,
        I: Iterator<Item = Result<refdb::Ref<O>, E>>,
    {
        use Either::*;

        let id = error::LocalOrRemote::from(self.id.copied());

        info!("validating sigtree of {}", id);
        debug!(refs = ?self.refs);

        let mut fail = Vec::new();

        let mut count = 0;
        let mut seen = HashSet::new();
        let mut seen_rad_id = false;
        let mut seen_sigrefs = false;

        for item in iter {
            count += 1;
            let refdb::Ref {
                name, peeled: oid, ..
            } = item?;
            match refs::Parsed::<U>::try_from(name.clone()) {
                Err(e) => {
                    fail.push(error::Validation::Malformed {
                        name: name.into_refstring(),
                        source: e,
                    });
                },
                Ok(parsed) if parsed.remote.as_ref() != self.id => {
                    warn!("skipping remote {:?} not owned by {}", parsed.remote, id)
                },

                Ok(parsed) => {
                    seen.insert(parsed.to_owned().as_ref().to_owned());
                    match parsed.inner {
                        Left(refs::parsed::Rad::Id) => {
                            seen_rad_id = true;
                        },
                        Left(refs::parsed::Rad::SignedRefs) => {
                            seen_sigrefs = true;
                            if oid.as_ref() != self.refs.at.as_ref() {
                                fail.push(error::Validation::MismatchedTips {
                                    expected: self.refs.at.as_ref().to_owned(),
                                    actual: oid.as_ref().to_owned(),
                                    name: parsed.to_owned().as_ref().to_owned(),
                                })
                            }
                        },
                        Left(_) => {},

                        Right(name) => match self.refs.refs.get(name.as_ref()) {
                            Some(tip) => {
                                if tip.as_ref() != oid.as_ref() {
                                    fail.push(error::Validation::MismatchedTips {
                                        expected: tip.as_ref().to_owned(),
                                        actual: oid.as_ref().to_owned(),
                                        name: name.as_ref().to_owned(),
                                    });
                                }
                            },
                            None => {
                                fail.push(error::Validation::Unexpected(name.as_ref().to_owned()))
                            },
                        },
                    }
                },
            }
        }

        if count == 0 {
            fail.push(error::Validation::NoData(id));
        } else {
            if !seen_rad_id {
                fail.push(error::Validation::MissingRadId(id));
            }
            if !seen_sigrefs {
                fail.push(error::Validation::MissingSigRefs(id));
            }

            for missing in self
                .refs
                .refs
                .keys()
                .filter(|k| !seen.contains(k.as_refstr()))
            {
                fail.push(error::Validation::Missing {
                    refname: missing.to_owned(),
                    remote: id,
                });
            }
        }

        Ok(fail)
    }
}
