// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{io, marker::PhantomData, path::PathBuf};

use bstr::BString;
use futures_lite::io::{AsyncRead, AsyncWrite};
use link_git::{
    protocol as git,
    protocol::{ObjectId, Ref},
};
use radicle_data::NonEmptyVec;

use crate::{transmit::LsRefs, Net, Odb, Refdb, Urn};

#[async_trait]
pub trait Connection {
    type Read: AsyncRead + Unpin;
    type Write: AsyncWrite + Unpin;
    type Error: std::error::Error + Send + Sync + 'static;

    async fn open_stream(&self) -> Result<(Self::Read, Self::Write), Self::Error>;
}

pub struct Network<U, D, B, C> {
    git_dir: PathBuf,
    urn: U,
    db: D,
    conn: C,
    _marker: PhantomData<B>,
}

impl<U, D, B, C> Network<U, D, B, C> {
    pub fn new(db: D, conn: C, git_dir: impl Into<PathBuf>, urn: U) -> Self {
        Self {
            git_dir: git_dir.into(),
            db,
            conn,
            urn,
            _marker: PhantomData,
        }
    }
}

#[async_trait(?Send)]
impl<U, D, B, C> Net for Network<U, D, B, C>
where
    U: Urn,

    D: Refdb + Odb + AsRef<B>,
    D::FindError: Send + Sync,

    B: ToOwned,
    <B as ToOwned>::Owned: git::packwriter::BuildThickener + Send + 'static,

    C: Connection,
    C::Read: Send + 'static,
    C::Write: Send + 'static,
    C::Error: Send + Sync,
{
    type Error = io::Error;

    #[tracing::instrument(level = "debug", skip(self), err)]
    async fn run_ls_refs(&self, ls: LsRefs) -> Result<Vec<Ref>, Self::Error> {
        let ref_prefixes = match ls {
            LsRefs::Full => Vec::default(),
            LsRefs::Prefix { prefixes } => {
                let mut ps = prefixes
                    .into_iter()
                    .map(Into::into)
                    .collect::<Vec<BString>>();
                ps.sort();
                ps.dedup();

                ps
            },
        };
        let (recv, send) = self.conn.open_stream().await.map_err(io_other)?;
        git::ls_refs(
            git::ls::Options {
                repo: BString::from(self.urn.encode_id()),
                extra_params: Vec::default(),
                ref_prefixes,
            },
            recv,
            send,
        )
        .await
    }

    #[tracing::instrument(level = "debug", skip(self), err)]
    async fn run_fetch(
        &self,
        max_pack_bytes: u64,
        wants: NonEmptyVec<ObjectId>,
        haves: Vec<ObjectId>,
    ) -> Result<(), Self::Error> {
        let wants = {
            let NonEmptyVec { head, mut tail } = wants;
            tail.push(head);
            tail
        };
        let out = {
            // FIXME: make options work with slice
            let wants = wants.clone();
            let thick: B::Owned = self.db.as_ref().to_owned();
            let (recv, send) = self.conn.open_stream().await.map_err(io_other)?;
            git::fetch(
                git::fetch::Options {
                    repo: BString::from(self.urn.encode_id()),
                    extra_params: vec![],
                    wants,
                    haves,
                    want_refs: vec![],
                },
                move |stop| {
                    git::packwriter::Standard::new(
                        &self.git_dir,
                        git::packwriter::Options {
                            max_pack_bytes,
                            ..Default::default()
                        },
                        thick,
                        stop,
                    )
                },
                recv,
                send,
            )
            .await?
        };
        let pack_path = out
            .pack
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "empty or no packfile received",
                )
            })?
            .index_path
            .expect("written packfile must have a path");

        // Validate we got all requested tips in the pack
        {
            use link_git::odb::index::IndexFile;

            let idx = IndexFile::at(&pack_path).map_err(io_other)?;
            for oid in wants {
                if idx.lookup(oid).is_none() {
                    return Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        format!("wanted {} not found in pack", oid),
                    ));
                }
            }
        }
        // abstraction leak: we could add the `Index` directly if we knew the
        // type of our odb.
        self.db.add_pack(&pack_path).map_err(io_other)?;

        Ok(())
    }
}

fn io_other<E>(e: E) -> io::Error
where
    E: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    io::Error::new(io::ErrorKind::Other, e)
}
