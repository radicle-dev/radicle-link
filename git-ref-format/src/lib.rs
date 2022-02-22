// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

//! Everything you never knew you wanted for handling git ref names.
//!
//! # Overview
//!
//! This crate provides a number of types which allow to validate git ref names,
//! create new ones which are valid by construction, make assertions about their
//! structure, and deconstruct them into their components.
//!
//! ## Basic Types
//!
//! The basic types are:
//!
//! * [`RefStr`]
//! * [`RefString`]
//!
//! They are wrappers around [`str`] and [`String`] respectively, with the
//! additional guarantee that they are also valid ref names as per
//! [`git-check-ref-format`] (which is also exposed directly as
//! [`check_ref_format`]). Both types are referred to as "ref strings".
//!
//! Note that this implies that ref names must be valid UTF-8, which git itself
//! doesn't require.
//!
//! Ref strings can be iterated over, either yielding `&str` or [`Component`]. A
//! [`Component`] is guaranteed to not contain a '/' separator, and can thus
//! also be used to conveniently construct known-valid ref strings. The [`lit`]
//! module contains a number of types (and `const` values thereof) which can be
//! coerced into [`Component`], and thus can be used to construct known-valid
//! ref strings.
//!
//! The [`name`] module also provides a number of constant values of commonly
//! used ref strings / components, which are useful for pattern matching.
//!
//! The `"macro"` feature enables the `refstring!` and `component!` macros,
//! which can be convenient to construct compile-time validated [`RefString`]s
//! respectively [`Component`]s.
//!
//! ## Refspec Patterns
//!
//! The types
//!
//! * [`refspec::PatternStr`]
//! * [`refspec::PatternString`]
//!
//! guarantee that their values are valid ref strings but additionally _may_
//! contain at most one "*" character. It is thus possible to convert a ref
//! string to a refspec pattern, but not the other way round. Refspec patterns
//! are commonly used for mapping remote to local refs (cf. [`git-fetch`]).
//!
//! The `"macro"` feature enables the `refspec::pattern!` macro, which
//! constructs a compile-time validated [`refspec::PatternString`].
//!
//! ## Structured Ref Strings
//!
//! Ref strings may be [`Qualified`], which essentially means that they start
//! with "refs/". [`Qualified`] ref string also require at least three
//! components (eg. "refs/heads/main"), which makes it easier to deal with
//! common naming conventions.
//!
//! [`Qualified`] refs may be [`Namespaced`], or can be given a namespace
//! (namespaces can be nested). [`Namespaced`] refs are also [`Qualified`], and
//! can have their namespace(s) stripped.
//!
//! # On Git Ref Name Conventions
//!
//! Git references are essentially path names pointing to their traditional
//! storage location in a the repository (`$GIT_DIR/refs`). Unlike (UNIX) file
//! paths, they are subject to a few restrictions, as described in
//! [`git-check-ref-format`].
//!
//! On top of that, there are a number of conventions around the hierarchical
//! naming, _some_ of which are treated specially by tools such as the `git`
//! CLI. For example:
//!
//! * `refs/heads/..` are also called "branches".
//!
//!   Omitting the "refs/heads/" prefix is typically accepted. Such a branch
//!   name is also referred to as a "shorthand" ref.
//!
//! * `refs/tags/..` are assumed to contain tags.
//!
//!   `git` treats tags specially, specifically it insists that they be globally
//!   unique across all   copies of the repository.
//!
//! * `refs/remotes/../..` is where "remote tracking branches" are stored.
//!
//!   In `git`, the first element after "remotes" is considered the name of the
//!   [remote][git-remote] (as it appears in the config file), while everything
//!   after that is considered a shorthand branch. Note, however, that the
//!   remote name may itself contain '/' separators, so it is not generally
//!   possible to extract  the branch name without access to the config.
//!
//! * `refs/namespaces/..` is hidden unless [`gitnamespaces`] are in effect.
//!
//!   The structure of namespaces is recursive: they contain full refs, which
//!   can themselves be namespaces (eg.
//!   `refs/namespaces/a/refs/namespaces/b/refs/heads/branch`). Note that,
//!   unlike remote names,  namespace names can **not** contain forward slashes
//!   but there is no tooling which would enforce that.
//!
//! There are also other such ref hierachies `git` knows about, and this crate
//! doesn't attempt to cover all of them. More importantly, `git` does not
//! impose any restrictions on ref hierarchies: as long as they don't collide
//! with convential ones, applications can introduce any hierchies they want.
//!
//! This restricts the transformations between conventional refs which can be
//! made without additional information besides the ref name: for example, it is
//! not generally possible to turn a remote tracking branch into a branch (or a
//! shorthand) without knowning about all possible remote names.
//!
//! Therefore, this crate doesn't attempt to interpret all possible semantics
//! associated with refs, and instead tries to make it easy for library
//! consumers to do so.
//!
//! [`git-check-ref-format`]: https://git-scm.com/docs/git-check-ref-format
//! [`git-fetch`]: https://git-scm.com/docs/git-fetch
//! [git-remote]: https://git-scm.com/docs/git-remote
//! [`gitnamespaces`]: https://git-scm.com/docs/gitnamespaces
#[cfg(feature = "percent-encoding")]
pub use git_ref_format_core::PercentEncode;
pub use git_ref_format_core::{
    check_ref_format,
    lit,
    name::component,
    Component,
    DuplicateGlob,
    Error,
    Namespaced,
    Options,
    Qualified,
    RefStr,
    RefString,
};

pub mod name {
    pub use git_ref_format_core::name::*;

    #[cfg(any(feature = "macro", feature = "git-ref-format-macro"))]
    pub use git_ref_format_macro::component;
}

#[cfg(any(feature = "macro", feature = "git-ref-format-macro"))]
pub use git_ref_format_macro::refname;

pub mod refspec {
    pub use git_ref_format_core::refspec::*;

    #[cfg(any(feature = "macro", feature = "git-ref-format-macro"))]
    pub use git_ref_format_macro::pattern;
}
