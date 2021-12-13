// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::collections::{hash_map, HashMap};

use bstr::{BStr, BString};

use super::{Applied, RefScan, Refdb, Update, Updated};
use crate::{ObjectId, Void};

/// A very simple in-memory [`Refdb`].
///
/// It treats symrefs as direct, and does not keep track of ancestry (hence
/// updates can't fail).
#[derive(Default, Debug)]
pub struct Mem {
    refs: HashMap<BString, ObjectId>,
}

impl From<HashMap<BString, ObjectId>> for Mem {
    fn from(refs: HashMap<BString, ObjectId>) -> Self {
        Self { refs }
    }
}

impl Refdb for Mem {
    type Oid = ObjectId;

    type FindError = Void;
    type TxError = Void;
    type ReloadError = Void;

    fn refname_to_id(
        &self,
        refname: impl AsRef<BStr>,
    ) -> Result<Option<Self::Oid>, Self::FindError> {
        Ok(self.refs.get(refname.as_ref()).map(Clone::clone))
    }

    fn update<'a, I>(&mut self, updates: I) -> Result<Applied<'a>, Self::TxError>
    where
        I: IntoIterator<Item = Update<'a>>,
    {
        let mut ap = Applied::default();
        for up in updates {
            match up {
                Update::Direct {
                    name,
                    target,
                    no_ff: _,
                } => {
                    let name = name.into_owned();
                    self.refs.insert(name.clone(), target);
                    ap.updated.push(Updated::Direct { name, target });
                },
                Update::Symbolic {
                    name,
                    target,
                    type_change: _,
                } => {
                    let name = name.into_owned();
                    self.refs.insert(name.clone(), target.target);
                    ap.updated.push(Updated::Symbolic {
                        name,
                        target: target.name(),
                    });
                },
            }
        }

        Ok(ap)
    }

    fn reload(&mut self) -> Result<(), Self::ReloadError> {
        Ok(())
    }
}

impl<'a> RefScan for &'a Mem {
    type Oid = ObjectId;
    type Scan = Scan<'a, Self::Oid>;
    type Error = Void;

    fn scan<O, P>(self, prefix: O) -> Result<Self::Scan, Self::Error>
    where
        O: Into<Option<P>>,
        P: AsRef<str>,
    {
        let prefix = prefix.into();
        Ok(Scan {
            pref: prefix.map(|p| p.as_ref().to_owned()),
            iter: self.refs.iter(),
        })
    }
}

pub struct Scan<'a, Oid> {
    pref: Option<String>,
    iter: hash_map::Iter<'a, BString, Oid>,
}

impl<'a, Oid> Iterator for Scan<'a, Oid>
where
    Oid: Clone + 'a,
{
    type Item = Result<(BString, Oid), Void>;

    fn next(&mut self) -> Option<Self::Item> {
        let next = self.iter.next().and_then(|(k, v)| match &self.pref {
            None => Some((k.to_owned(), v.clone())),
            Some(p) => {
                if k.starts_with(p.as_bytes()) {
                    Some((k.to_owned(), v.clone()))
                } else {
                    None
                }
            },
        });

        next.map(Ok)
    }
}
