// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{convert::Infallible, path::Path, sync::Arc};

use link_git::{
    hash::{oid, ObjectId},
    odb::{self, index, window, Object},
    protocol::packwriter::{BuildThickener, Thickener},
    traverse::commit::{ancestors, Ancestors},
};

use crate::Error;

#[derive(Clone)]
pub struct Odb(Arc<odb::Odb<index::Shared<index::Stats>, window::Small<window::Stats>>>);

impl Odb {
    pub fn open(git_dir: impl AsRef<Path>) -> Result<Self, Error> {
        let git_dir = git_dir.as_ref();
        let loose = odb::backend::Loose::at(git_dir.join("objects"));
        let packed = {
            let index = odb::index::Shared::open(git_dir)?.with_stats();
            let data = odb::window::Fixed::default().with_stats();
            odb::backend::Packed { index, data }
        };

        Ok(Self(Arc::new(odb::Odb { loose, packed })))
    }
}

impl Thickener for Odb {
    fn find_object<'a>(&self, id: ObjectId, buf: &'a mut Vec<u8>) -> Option<Object<'a>> {
        self.0.find(id, buf, &mut odb::cache::Never).ok().flatten()
    }
}

impl BuildThickener for Odb {
    type Error = Infallible;
    type Thick = Self;

    fn build_thickener(&self) -> Result<Self::Thick, Self::Error> {
        Ok(self.clone())
    }
}

impl crate::odb::Odb for Odb {
    type LookupError = odb::Error;
    type RevwalkError = ancestors::Error;
    type AddPackError = odb::pack::error::Index;

    fn contains(&self, oid: impl AsRef<oid>) -> bool {
        self.0.contains(oid)
    }

    fn lookup<'a>(
        &self,
        oid: impl AsRef<oid>,
        buf: &'a mut Vec<u8>,
    ) -> Result<Option<crate::odb::Object<'a>>, Self::LookupError> {
        self.0
            .find(oid, buf, &mut odb::cache::Never)
            .map(|obj| obj.map(Into::into))
    }

    fn is_in_ancestry_path(
        &self,
        new: impl Into<ObjectId>,
        old: impl Into<ObjectId>,
    ) -> Result<bool, Self::RevwalkError> {
        let new = new.into();
        let old = old.into();

        // No need to take the lock
        if new == old {
            return Ok(true);
        }

        let odb = &self.0;
        // Annoyingly, gitoxide returns an error if the tip is not known. While
        // we're at it, we can also fast-path the revwalk if the ancestor is
        // unknown.
        if !odb.contains(&new) || !odb.contains(&old) {
            return Ok(false);
        }
        let mut cache = odb::cache::lru::StaticLinkedList::<64>::default();
        let walk = Ancestors::new(Some(new), ancestors::State::default(), move |oid, buf| {
            let obj = odb.find(oid, buf, &mut cache).ok().flatten()?;
            obj.try_into_commit_iter()
        });
        for parent in walk {
            if parent? == old {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn add_pack(&self, path: impl AsRef<Path>) -> Result<(), Self::AddPackError> {
        let index = odb::pack::Index::open(path)?;
        self.0.packed.index.push(index);

        Ok(())
    }
}
