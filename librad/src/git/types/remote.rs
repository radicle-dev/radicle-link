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

use std::path::{Path, PathBuf};

use super::AsRefspec;

pub struct Remote {
    /// The file path to the git monorepo.
    pub monorepo: PathBuf,
    /// Name of the remote, e.g. `"rad"`, `"origin"`.
    pub name: String,
    /// The set of fetch specs to add upon creation.
    pub fetch_spec: Box<dyn AsRefspec>,
    /// The set of push specs to add upon creation.
    pub push_specs: Vec<Box<dyn AsRefspec>>,
}

impl Remote {
    /// Create a `"rad"` remote with no specs.
    pub fn rad_remote(monorepo: impl AsRef<Path>, fetch_spec: Box<dyn AsRefspec>) -> Self {
        Self {
            monorepo: monorepo.as_ref().to_path_buf(),
            name: "rad".to_string(),
            fetch_spec,
            push_specs: vec![],
        }
    }

    /// Add a series of push specs to the remote.
    pub fn add_pushes<I>(&mut self, specs: I)
    where
        I: Iterator<Item = Box<dyn AsRefspec>>,
    {
        for spec in specs {
            self.push_specs.push(spec)
        }
    }

    /// Create the [`git2::Remote`] and add the specs.
    pub fn create<'a>(&self, repo: &'a git2::Repository) -> Result<git2::Remote<'a>, git2::Error> {
        let _ = repo.remote_with_fetch(
            &self.name,
            &format!("file://{}", self.monorepo.display()),
            &self.fetch_spec.as_refspec(),
        )?;

        for spec in self.push_specs.iter() {
            repo.remote_add_push(&self.name, &spec.as_refspec())?;
        }

        // To ensure that the push spec is persisted we need to call `find_remote` here.
        // Otherwise, `remote_add_push` doesn't affect the "loaded remotes".
        repo.find_remote("rad")
    }
}

#[cfg(test)]
mod tests {
    use std::{io, marker::PhantomData};

    use super::*;
    use crate::{git::types::*, hash::Hash};
    use librad_test::tempdir::WithTmpDir;

    #[test]
    fn can_create_remote() {
        WithTmpDir::new::<_, io::Error>(|path| {
            let repo = git2::Repository::init(path).expect("failed to init repo");

            let id = Hash::hash(b"geez");
            let heads: FlatRef<String, _> = FlatRef::heads(PhantomData, None);
            let namespaced_heads = NamespacedRef::heads(id, None);

            let fetch = heads
                .clone()
                .refspec(namespaced_heads.clone(), Force::True)
                .into_dyn();
            let push = namespaced_heads.refspec(heads, Force::False).into_dyn();


            let mut remote = Remote::rad_remote(path, fetch);
            remote.add_pushes(vec![push].into_iter());
            let git_remote = remote.create(&repo).expect("failed to create the remote");


            assert_eq!(
                git_remote
                    .fetch_refspecs().expect("failed to get fetchspecs")
                    .iter()
                    .collect::<Vec<Option<&str>>>(),
                vec![Some("+refs/namespaces/hwd1yredksthny1hht3bkhtkxakuzfnjxd8dyk364prfkjxe4xpxsww3try/refs/heads/*:refs/heads/*")],
            );

            assert_eq!(
                git_remote
                    .push_refspecs().expect("failed to get fetchspecs")
                    .iter()
                    .collect::<Vec<Option<&str>>>(),
                vec![Some("refs/heads/*:refs/namespaces/hwd1yredksthny1hht3bkhtkxakuzfnjxd8dyk364prfkjxe4xpxsww3try/refs/heads/*")],
            );

            Ok(())
        }).unwrap();
    }
}
