// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    collections::HashMap,
    convert::TryFrom,
    io,
    path::Path,
    time::{SystemTime, SystemTimeError, UNIX_EPOCH},
};

use bstr::{BString, ByteVec as _};
use either::Either;
use git_ref_format::{Component, Qualified, RefString};
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
    #[allow(clippy::enum_variant_names)]
    pub enum Find {
        #[error(transparent)]
        Follow(#[from] refs::db::error::Follow),

        #[error(transparent)]
        Find(#[from] refs::file::find::Error),
    }

    #[derive(Debug, Error)]
    pub enum Scan {
        #[error("`gitoxide` yielded an invalid refname")]
        WtfGitoxide(#[from] git_ref_format::Error),

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

        #[error("`gitoxide` yielded an invalid refname")]
        WtfGitoxide(#[from] git_ref_format::Error),

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
    namespace: Component<'static>,
    odb: D,
    refdb: refs::db::Refdb,
    snap: refs::db::Snapshot,
}

impl<D> Refdb<D> {
    pub fn new(
        info: UserInfo,
        odb: D,
        refdb: refs::db::Refdb,
        namespace: impl Into<Component<'static>>,
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

    fn namespaced(&self, name: &Qualified) -> FullName {
        qualified_to_fullname(name.add_namespace(self.namespace.clone()).into_qualified())
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
        update: Update<'a>,
    ) -> Result<Either<Update<'a>, Vec<RefEdit>>, error::Tx> {
        match update {
            Update::Direct {
                name,
                target,
                no_ff,
            } => self.direct_edit(name, target, no_ff),

            Update::Symbolic {
                name,
                target,
                type_change,
            } => self.symbolic_edit(name, target, type_change),
        }
    }

    fn direct_edit<'a>(
        &self,
        name: Qualified<'a>,
        target: ObjectId,
        no_ff: Policy,
    ) -> Result<Either<Update<'a>, Vec<RefEdit>>, error::Tx> {
        use Either::*;

        let force_create_reflog = force_reflog(&name);
        let name_ns = self.namespaced(&name);
        let tip = self.find_namespaced(&name_ns)?;
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
                name: name_ns,
                deref: false,
            }])),

            Some(prev) => {
                let is_ff = self.odb.is_in_ancestry_path(target, prev).map_err(|e| {
                    error::Tx::Ancestry {
                        name: name_ns.clone().into_inner(),
                        new: target,
                        old: prev,
                        source: e.into(),
                    }
                })?;
                if !is_ff {
                    match no_ff {
                        Policy::Abort => Err(error::Tx::NonFF {
                            name: name_ns.into_inner(),
                            new: target,
                            cur: prev,
                        }),
                        Policy::Reject => Ok(Left(Update::Direct {
                            name,
                            target,
                            no_ff,
                        })),
                        Policy::Allow => Ok(Right(vec![RefEdit {
                            change: Change::Update {
                                log: LogChange {
                                    mode: RefLog::AndReference,
                                    force_create_reflog,
                                    message: "replicate: forced update".into(),
                                },
                                expected: PreviousValue::MustExistAndMatch(Target::Peeled(prev)),
                                new: Target::Peeled(target),
                            },
                            name: name_ns,
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
                            expected: PreviousValue::MustExistAndMatch(Target::Peeled(prev)),
                            new: Target::Peeled(target),
                        },
                        name: name_ns,
                        deref: false,
                    }]))
                }
            },
        }
    }

    fn symbolic_edit<'a>(
        &self,
        name: Qualified<'a>,
        target: SymrefTarget<'a>,
        type_change: Policy,
    ) -> Result<Either<Update<'a>, Vec<RefEdit>>, error::Tx> {
        use Either::*;

        let name_ns = self.namespaced(&name);
        let src = self
            .snap
            .find(name_ns.as_bstr())
            .map_err(error::Find::from)?
            .map(|r| r.target);

        match src {
            // Type change
            Some(Target::Peeled(_prev)) if matches!(type_change, Policy::Abort) => {
                Err(error::Tx::TypeChange(name_ns.into_inner()))
            },
            Some(Target::Peeled(_prev)) if matches!(type_change, Policy::Reject) => {
                Ok(Left(Update::Symbolic {
                    name,
                    target,
                    type_change,
                }))
            },

            _ => {
                let src_name = name_ns;
                let dst = self
                    .snap
                    .find(target.name().as_bstr())
                    .map_err(error::Find::from)?
                    .map(|r| r.target);
                let force_create_reflog = force_reflog(&name);

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
                        let dst_name = qualified_to_fullname(dst_name.clone().into_qualified());
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
                                    new: Target::Peeled(target),
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
                        let is_ff = target != dst
                            && self.is_in_ancestry_path(target, dst).map_err(|e| {
                                error::Tx::Ancestry {
                                    name: dst_name
                                        .clone()
                                        .into_qualified()
                                        .into_refstring()
                                        .into_bstring(),
                                    new: target,
                                    old: dst,
                                    source: e.into(),
                                }
                            })?;
                        if is_ff {
                            let dst_name_qualified = dst_name.to_owned().into_qualified();
                            let force_create_reflog = force_reflog(&dst_name_qualified);
                            let dst_name = qualified_to_fullname(dst_name_qualified);
                            edits.push(RefEdit {
                                change: Change::Update {
                                    log: LogChange {
                                        mode: RefLog::AndReference,
                                        force_create_reflog,
                                        message: "replicate: fast-forward symref target".into(),
                                    },
                                    expected: PreviousValue::MustExistAndMatch(Target::Peeled(dst)),
                                    new: Target::Peeled(target),
                                },
                                name: dst_name,
                                deref: false,
                            })
                        }

                        let dst_name = qualified_to_fullname(dst_name.clone().into_qualified());
                        edits.push(RefEdit {
                            change: Change::Update {
                                log: LogChange {
                                    mode: RefLog::AndReference,
                                    force_create_reflog,
                                    message: "replicate: symbolic ref".into(),
                                },
                                expected: src
                                    .map(PreviousValue::MustExistAndMatch)
                                    .unwrap_or(PreviousValue::MustNotExist),
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
    }
}

impl<'a, D> refdb::RefScan for &'a Refdb<D> {
    type Oid = ObjectId;
    type Scan = Scan<'a>;
    type Error = error::Scan;

    fn scan<O, P>(self, prefix: O) -> Result<Self::Scan, Self::Error>
    where
        O: Into<Option<P>>,
        P: AsRef<str>,
    {
        let prefix = {
            let ns = Path::new("refs/namespaces").join(self.namespace.as_str());
            match prefix.into() {
                None => ns,
                Some(p) => ns.join(Path::new(p.as_ref())),
            }
        };
        let inner = self.snap.iter(Some(prefix))?;
        Ok(Scan {
            snap: &self.snap,
            inner,
        })
    }
}

impl<D: Odb> refdb::Refdb for Refdb<D> {
    type Oid = ObjectId;

    type FindError = error::Find;
    type TxError = error::Tx;
    type ReloadError = error::Reload;

    fn refname_to_id<'a, Q>(&self, refname: Q) -> Result<Option<Self::Oid>, Self::FindError>
    where
        Q: AsRef<Qualified<'a>>,
    {
        self.find_namespaced(&self.namespaced(refname.as_ref()))
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
            .map(|RefEdit { change, name, .. }| {
                let name = fullname_to_refstring(name)?;
                match change {
                    Change::Update { new, .. } => match new {
                        Target::Peeled(oid) => Ok(Updated::Direct { name, target: oid }),
                        Target::Symbolic(sym) => Ok(Updated::Symbolic {
                            name,
                            target: fullname_to_refstring(sym)?,
                        }),
                    },
                    Change::Delete { .. } => unreachable!("unexpected delete"),
                }
            })
            .collect::<Result<Vec<_>, Self::TxError>>()?;

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
    inner: refs::file::iter::LooseThenPacked<'a, 'a>,
}

impl Scan<'_> {
    fn next_ref(&self, r: Reference) -> Result<refdb::Ref<ObjectId>, error::Scan> {
        use Either::*;

        let peeled = self
            .snap
            .follow(&r)
            .map(|Reference { target, .. }| target.into_id())?;
        let name = fullname_to_qualified(r.name)?
            .namespaced()
            .expect("BUG: revwalk should return namespaced refs")
            .strip_namespace();
        let target = match r.target {
            Target::Peeled(oid) => Left(oid),
            Target::Symbolic(sym) => Right(fullname_to_qualified(sym)?),
        };

        Ok(refdb::Ref {
            name,
            target,
            peeled,
        })
    }
}

impl<'a> Iterator for Scan<'a> {
    type Item = Result<refdb::Ref<ObjectId>, error::Scan>;

    fn next(&mut self) -> Option<Self::Item> {
        use refs::file::iter::loose_then_packed::Error;

        match self.inner.next()? {
            // XXX: https://github.com/Byron/gitoxide/issues/202
            Err(Error::Traversal(e)) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => Some(Err(e.into())),
            Ok(r) => Some(self.next_ref(r)),
        }
    }
}

fn force_reflog(refname: &Qualified) -> bool {
    use git_ref_format::lit::{KnownLit::*, SomeLit};

    matches!(
        refname
            .components()
            .take(8)
            .map(SomeLit::from)
            .filter_map(SomeLit::known)
            .collect::<Vec<_>>()[..],
        [Refs, Rad, ..]
            | [Refs, Remotes, _, Rad, ..]
            | [Refs, Namespaces, _, Refs, Rad, ..]
            | [Refs, Namespaces, _, Refs, Remotes, _, Rad, ..]
    )
}

fn fullname_to_qualified(name: FullName) -> Result<Qualified<'static>, git_ref_format::Error> {
    fullname_to_refstring(name).map(|name| {
        name.into_qualified()
            .expect("BUG: revwalk should always return qualified refs")
    })
}

fn qualified_to_fullname(q: Qualified) -> FullName {
    FullName::try_from(q.into_refstring().into_bstring())
        .expect("`Qualified` is a valid `FullName`")
}

fn fullname_to_refstring(name: FullName) -> Result<RefString, git_ref_format::Error> {
    RefString::try_from(Vec::from(name.into_inner()).into_string_lossy())
}
