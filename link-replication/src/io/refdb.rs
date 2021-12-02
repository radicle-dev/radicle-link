// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    borrow::Cow,
    collections::HashMap,
    convert::TryFrom,
    io,
    path::{Path, PathBuf},
    time::{SystemTime, SystemTimeError, UNIX_EPOCH},
};

use bstr::{BStr, BString, ByteSlice as _, ByteVec as _};
use either::Either;
use itertools::Itertools as _;
use link_crypto::PeerId;
use link_git::{
    actor,
    lock,
    protocol::{oid, ObjectId},
    refs::{
        self,
        transaction::{Change, LogChange, PreviousValue, RefEdit, RefLog},
        FullName,
        Reference,
        Target,
    },
};

use crate::{
    odb::Odb,
    refdb::{self, Applied, Policy, SymrefTarget, Update, Updated},
    Error,
};

pub mod error {
    use std::{io, time::SystemTimeError};

    use bstr::BString;
    use link_git::{protocol::ObjectId, refs};
    use thiserror::Error;

    #[derive(Debug, Error)]
    pub enum Find {
        #[error(transparent)]
        Refname(#[from] refs::name::Error),

        #[error(transparent)]
        Follow(#[from] refs::db::error::Follow),

        #[error(transparent)]
        Find(#[from] refs::file::find::Error),
    }

    #[derive(Debug, Error)]
    pub enum Scan {
        #[error(transparent)]
        Iter(#[from] refs::file::iter::loose_then_packed::Error),

        #[error(transparent)]
        Follow(#[from] refs::db::error::Follow),

        #[error(transparent)]
        Io(#[from] io::Error),
    }

    #[derive(Debug, Error)]
    pub enum Tx {
        #[error("non-fast-forward update of {name} (current: {cur}, new: {new})")]
        NonFF {
            name: BString,
            new: ObjectId,
            cur: ObjectId,
        },

        #[error("missing target {target} for symbolic ref {name}")]
        MissingSymrefTarget { name: BString, target: BString },

        #[error("symref target {0} is itself a symref")]
        TargetSymbolic(BString),

        #[error("expected symref {name} to point to {expected}, but got {actual}")]
        UnexpectedSymrefTarget {
            name: BString,
            expected: ObjectId,
            actual: ObjectId,
        },

        #[error("rejected type change of {0}")]
        TypeChange(BString),

        #[error("error determining if {old} is an ancestor of {new} in within {name}")]
        Ancestry {
            name: BString,
            new: ObjectId,
            old: ObjectId,
            #[source]
            source: Box<dyn std::error::Error + Send + Sync + 'static>,
        },

        #[error(transparent)]
        Reload(#[from] Reload),

        #[error(transparent)]
        Prepare(#[from] refs::file::transaction::prepare::Error),

        #[error(transparent)]
        Commit(#[from] refs::file::transaction::commit::Error),

        #[error(transparent)]
        Refname(#[from] refs::name::Error),

        #[error(transparent)]
        Find(#[from] Find),

        #[error("broken system clock")]
        Clock(#[from] SystemTimeError),
    }

    #[derive(Debug, Error)]
    pub enum Reload {
        #[error("failed to reload packed refs")]
        Snapshot(#[from] refs::db::error::Snapshot),
    }
}

#[derive(Clone)]
pub struct UserInfo {
    pub name: String,
    pub peer_id: PeerId,
}

impl UserInfo {
    fn signature(&self) -> Result<actor::Signature, SystemTimeError> {
        let time = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        Ok(actor::Signature {
            name: BString::from(self.name.as_str()),
            email: format!("{}@{}", self.name, self.peer_id).into(),
            time: actor::Time {
                time: time as u32,
                offset: 0,
                sign: actor::Sign::Plus,
            },
        })
    }
}

#[derive(Clone)]
pub struct Refdb<D> {
    info: UserInfo,
    namespace: refs::Namespace,
    odb: D,
    refdb: refs::db::Refdb,
    snap: refs::db::Snapshot,
}

impl<D> Refdb<D> {
    pub fn new(
        info: UserInfo,
        odb: D,
        refdb: refs::db::Refdb,
        namespace: impl Into<refs::Namespace>,
    ) -> Result<Self, Error> {
        let snap = refdb.snapshot()?;
        let namespace = namespace.into();

        Ok(Self {
            info,
            namespace,
            odb,
            refdb,
            snap,
        })
    }

    fn reload(&mut self) -> Result<(), error::Reload> {
        self.snap = self.refdb.snapshot()?;
        Ok(())
    }

    fn namespaced(&self, name: &mut Cow<BStr>) -> Result<FullName, refs::name::Error> {
        name.to_mut().insert_str(0, self.namespace.as_bstr());
        FullName::try_from(name.as_ref())
    }
}

impl<D: Odb> Refdb<D> {
    fn find_namespaced(&self, name: &FullName) -> Result<Option<ObjectId>, error::Find> {
        match self.snap.find(name.to_partial())? {
            None => Ok(None),
            Some(tip) => Ok(Some(self.snap.follow(&tip)?.target.into_id())),
        }
    }

    fn as_edits<'a>(
        &self,
        mut update: Update<'a>,
    ) -> Result<Either<Update<'a>, Vec<RefEdit>>, error::Tx> {
        use Either::*;

        match update {
            Update::Direct {
                ref mut name,
                target,
                no_ff,
            } => {
                let force_create_reflog = force_reflog(name);
                let name = self.namespaced(name)?;
                let tip = self.find_namespaced(&name)?;
                match tip {
                    None => Ok(Right(vec![RefEdit {
                        change: Change::Update {
                            log: LogChange {
                                mode: RefLog::AndReference,
                                force_create_reflog,
                                message: "replicate: create".into(),
                            },
                            expected: PreviousValue::MustNotExist,
                            new: Target::Peeled(target),
                        },
                        name,
                        deref: false,
                    }])),

                    Some(prev) => {
                        let is_ff = self.odb.is_in_ancestry_path(target, prev).map_err(|e| {
                            error::Tx::Ancestry {
                                name: name.as_bstr().to_owned(),
                                new: target,
                                old: prev,
                                source: e.into(),
                            }
                        })?;
                        if !is_ff {
                            match no_ff {
                                Policy::Abort => Err(error::Tx::NonFF {
                                    name: name.into_inner(),
                                    new: target,
                                    cur: prev,
                                }),
                                Policy::Reject => Ok(Left(update)),
                                Policy::Allow => Ok(Right(vec![RefEdit {
                                    change: Change::Update {
                                        log: LogChange {
                                            mode: RefLog::AndReference,
                                            force_create_reflog,
                                            message: "replicate: forced update".into(),
                                        },
                                        expected: PreviousValue::MustExistAndMatch(Target::Peeled(
                                            prev,
                                        )),
                                        new: Target::Peeled(target),
                                    },
                                    name,
                                    deref: false,
                                }])),
                            }
                        } else {
                            Ok(Right(vec![RefEdit {
                                change: Change::Update {
                                    log: LogChange {
                                        mode: RefLog::AndReference,
                                        force_create_reflog,
                                        message: "replicate: fast-forward".into(),
                                    },
                                    expected: PreviousValue::MustExistAndMatch(Target::Peeled(
                                        prev,
                                    )),
                                    new: Target::Peeled(target),
                                },
                                name,
                                deref: false,
                            }]))
                        }
                    },
                }
            },

            Update::Symbolic {
                ref mut name,
                ref target,
                type_change,
            } => {
                let name = self.namespaced(name)?;
                let src = self
                    .snap
                    .find(name.as_bstr())
                    .map_err(error::Find::from)?
                    .map(|r| r.target);

                match src {
                    // Type change
                    Some(Target::Peeled(_prev)) if matches!(type_change, Policy::Abort) => {
                        Err(error::Tx::TypeChange(name.into_inner()))
                    },
                    Some(Target::Peeled(_prev)) if matches!(type_change, Policy::Reject) => {
                        Ok(Left(update))
                    },

                    _ => {
                        let src_name = name;
                        let dst = self
                            .snap
                            .find(target.name().as_bstr())
                            .map_err(error::Find::from)?
                            .map(|r| r.target);
                        let force_create_reflog = force_reflog(src_name.as_bstr());

                        let SymrefTarget {
                            name: dst_name,
                            target,
                        } = target;
                        let edits = match dst {
                            // Target is a symref -- reject this for now
                            Some(Target::Symbolic(dst)) => {
                                return Err(error::Tx::TargetSymbolic(dst.into_inner()))
                            },

                            // Target does not exist
                            None => {
                                let dst_name = FullName::try_from(dst_name.qualified())?;
                                vec![
                                    // Create target
                                    RefEdit {
                                        change: Change::Update {
                                            log: LogChange {
                                                mode: RefLog::AndReference,
                                                force_create_reflog,
                                                message: "replicate: implicit symref target".into(),
                                            },
                                            expected: PreviousValue::MustNotExist,
                                            new: Target::Peeled(*target),
                                        },
                                        name: dst_name.clone(),
                                        deref: false,
                                    },
                                    // Create source
                                    RefEdit {
                                        change: Change::Update {
                                            log: LogChange {
                                                mode: RefLog::AndReference,
                                                force_create_reflog,
                                                message: "replicate: symbolic ref".into(),
                                            },
                                            expected: PreviousValue::MustNotExist,
                                            new: Target::Symbolic(dst_name),
                                        },
                                        name: src_name,
                                        deref: false,
                                    },
                                ]
                            },

                            // Target is a direct ref
                            Some(Target::Peeled(dst)) => {
                                let mut edits = Vec::with_capacity(2);

                                // Fast-forward target if possible
                                let is_ff = target != &dst
                                    && self.is_in_ancestry_path(*target, dst).map_err(|e| {
                                        error::Tx::Ancestry {
                                            name: dst_name.qualified().as_bstr().to_owned(),
                                            new: *target,
                                            old: dst,
                                            source: e.into(),
                                        }
                                    })?;
                                if is_ff {
                                    let dst_name = FullName::try_from(dst_name.qualified())?;
                                    edits.push(RefEdit {
                                        change: Change::Update {
                                            log: LogChange {
                                                mode: RefLog::AndReference,
                                                force_create_reflog: force_reflog(
                                                    dst_name.as_bstr(),
                                                ),
                                                message: "replicate: fast-forward symref target"
                                                    .into(),
                                            },
                                            expected: PreviousValue::MustExistAndMatch(
                                                Target::Peeled(dst),
                                            ),
                                            new: Target::Peeled(*target),
                                        },
                                        name: dst_name,
                                        deref: false,
                                    })
                                }

                                let dst_name = FullName::try_from(dst_name.qualified())?;
                                edits.push(RefEdit {
                                    change: Change::Update {
                                        log: LogChange {
                                            mode: RefLog::AndReference,
                                            force_create_reflog,
                                            message: "replicate: symbolic ref".into(),
                                        },
                                        expected: PreviousValue::MustNotExist,
                                        new: Target::Symbolic(dst_name),
                                    },
                                    name: src_name,
                                    deref: false,
                                });
                                edits
                            },
                        };

                        Ok(Right(edits))
                    },
                }
            },
        }
    }
}

impl<D: Odb> refdb::Refdb for Refdb<D> {
    type Oid = ObjectId;

    type Scan<'a> = Scan<'a>;

    type FindError = error::Find;
    type ScanError = error::Scan;
    type TxError = error::Tx;
    type ReloadError = error::Reload;

    fn refname_to_id(
        &self,
        refname: impl AsRef<BStr>,
    ) -> Result<Option<Self::Oid>, Self::FindError> {
        self.find_namespaced(&self.namespaced(&mut Cow::from(refname.as_ref()))?)
    }

    fn scan<O, P>(&self, prefix: O) -> Result<Self::Scan<'_>, Self::ScanError>
    where
        O: Into<Option<P>>,
        P: AsRef<str>,
    {
        let prefix = {
            let ns = self.namespace.to_path();
            match prefix.into() {
                None => ns,
                Some(p) => ns.join(PathBuf::from(p.as_ref())).into(),
            }
        };
        let inner = self.snap.iter(Some(prefix))?;
        Ok(Scan {
            snap: &self.snap,
            namespace: &self.namespace,
            inner,
        })
    }

    fn update<'a, I>(&mut self, updates: I) -> Result<Applied<'a>, Self::TxError>
    where
        I: IntoIterator<Item = Update<'a>>,
    {
        use Either::*;

        #[derive(Default)]
        struct Edits<'a> {
            rejected: Vec<Update<'a>>,
            // XXX: annoyingly, gitoxide refuses multiple edits of the same ref
            // in a transaction
            edits: HashMap<FullName, RefEdit>,
        }

        let Edits { rejected, edits } = updates.into_iter().map(|up| self.as_edits(up)).fold_ok(
            Edits::default(),
            |mut es, e| {
                match e {
                    Left(rej) => es.rejected.push(rej),
                    Right(ed) => es.edits.extend(ed.into_iter().map(|e| (e.name.clone(), e))),
                }
                es
            },
        )?;
        let tx = self
            .snap
            .transaction()
            .prepare(edits.into_values(), lock::acquire::Fail::Immediately)?;
        let sig = self.info.signature()?;
        let applied = tx
            .commit(&sig)?
            .into_iter()
            .map(|RefEdit { change, name, .. }| match change {
                Change::Update { new, .. } => match new {
                    Target::Peeled(oid) => Updated::Direct {
                        name: name.into_inner(),
                        target: oid,
                    },
                    Target::Symbolic(sym) => Updated::Symbolic {
                        name: name.into_inner(),
                        target: sym.into_inner(),
                    },
                },
                Change::Delete { .. } => unreachable!("unexpected delete"),
            })
            .collect::<Vec<_>>();

        if !applied.is_empty() {
            self.reload()?;
        }

        Ok(Applied {
            rejected,
            updated: applied,
        })
    }

    fn reload(&mut self) -> Result<(), Self::ReloadError> {
        self.reload()
    }
}

impl<D: Odb> Odb for Refdb<D> {
    type LookupError = D::LookupError;
    type RevwalkError = D::RevwalkError;
    type AddPackError = D::AddPackError;

    fn contains(&self, oid: impl AsRef<oid>) -> bool {
        self.odb.contains(oid)
    }

    fn lookup<'a>(
        &self,
        oid: impl AsRef<oid>,
        buf: &'a mut Vec<u8>,
    ) -> Result<Option<crate::odb::Object<'a>>, Self::LookupError> {
        self.odb.lookup(oid, buf)
    }

    fn is_in_ancestry_path(
        &self,
        new: impl Into<ObjectId>,
        old: impl Into<ObjectId>,
    ) -> Result<bool, Self::RevwalkError> {
        self.odb.is_in_ancestry_path(new, old)
    }

    fn add_pack(&self, path: impl AsRef<Path>) -> Result<(), Self::AddPackError> {
        self.odb.add_pack(path)
    }
}

impl<D> AsRef<D> for Refdb<D> {
    fn as_ref(&self) -> &D {
        &self.odb
    }
}

pub struct Scan<'a> {
    snap: &'a refs::db::Snapshot,
    namespace: &'a refs::Namespace,
    inner: refs::file::iter::LooseThenPacked<'a, 'a>,
}

impl<'a> Iterator for Scan<'a> {
    type Item = Result<(BString, ObjectId), error::Scan>;

    fn next(&mut self) -> Option<Self::Item> {
        use refs::file::iter::loose_then_packed::Error;

        let item = self.inner.next()?;
        match item {
            // XXX: https://github.com/Byron/gitoxide/issues/202
            Err(Error::Traversal(e)) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => Some(Err(e.into())),
            Ok(r) => match self.snap.follow(&r) {
                Err(e) => Some(Err(e.into())),
                Ok(Reference {
                    mut name, target, ..
                }) => {
                    name.strip_namespace(self.namespace);
                    let oid = target.into_id();

                    Some(Ok((name.into_inner(), oid)))
                },
            },
        }
    }
}

fn force_reflog(refname: &BStr) -> bool {
    use crate::refs::{component::*, is_separator};

    matches!(
        refname.splitn(8, is_separator).collect::<Vec<_>>()[..],
        [REFS, RAD, ..]
            | [REFS, REMOTES, _, RAD, ..]
            | [REFS, NAMESPACES, _, REFS, RAD, ..]
            | [REFS, NAMESPACES, _, REFS, REMOTES, _, RAD, ..]
    )
}
