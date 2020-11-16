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

use std::{
    convert::TryFrom,
    fmt::{self, Display},
    str::FromStr,
};

use git_ext as ext;

use super::Force;

#[derive(Debug)]
pub struct Refspec<S, D> {
    /// The source spec (LHS of the `:`).
    ///
    /// When used as a fetch spec, it refers to the remote side, while as a push
    /// spec it refers to the local side.
    pub src: S,

    /// The destination spec (RHS of the `:`).
    ///
    /// When used as a fetch spec, it refers to the local side, while as a push
    /// spec it refers to the remote side.
    pub dst: D,

    /// Whether to allow history rewrites.
    pub force: Force,
}

impl<S, D> Refspec<S, D> {
    pub fn into_fetchspec(self) -> Fetchspec
    where
        S: Into<ext::RefspecPattern>,
        D: Into<ext::RefspecPattern>,
    {
        self.into()
    }

    pub fn into_pushspec(self) -> Pushspec
    where
        S: Into<ext::RefLike>,
        D: Into<ext::RefLike>,
    {
        self.into()
    }
}

impl<S, D> Display for Refspec<S, D>
where
    for<'a> &'a S: Into<ext::RefspecPattern>,
    for<'a> &'a D: Into<ext::RefspecPattern>,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.force.as_bool() {
            f.write_str("+")?;
        }

        let src = Into::<ext::RefspecPattern>::into(&self.src);
        let dst = Into::<ext::RefspecPattern>::into(&self.dst);

        write!(f, "{}:{}", src, dst)
    }
}

impl TryFrom<&str> for Refspec<ext::RefspecPattern, ext::RefspecPattern> {
    type Error = ext::reference::name::Error;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        let force = s.starts_with('+').into();
        let specs = s.trim_start_matches('+');
        let mut iter = specs.split(':');
        let src = iter
            .next()
            .ok_or(ext::reference::name::Error::RefFormat)
            .and_then(ext::RefspecPattern::try_from)?;
        let dst = iter
            .next()
            .ok_or(ext::reference::name::Error::RefFormat)
            .and_then(ext::RefspecPattern::try_from)?;

        Ok(Self { src, dst, force })
    }
}

impl FromStr for Refspec<ext::RefspecPattern, ext::RefspecPattern> {
    type Err = ext::reference::name::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::try_from(s)
    }
}

impl TryFrom<&str> for Refspec<ext::RefLike, ext::RefLike> {
    type Error = ext::reference::name::Error;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        let force = s.starts_with('+').into();
        let specs = s.trim_start_matches('+');
        let mut iter = specs.split(':');
        let src = iter
            .next()
            .ok_or(ext::reference::name::Error::RefFormat)
            .and_then(ext::RefLike::try_from)?;
        let dst = iter
            .next()
            .ok_or(ext::reference::name::Error::RefFormat)
            .and_then(ext::RefLike::try_from)?;

        Ok(Self { src, dst, force })
    }
}

impl FromStr for Refspec<ext::RefLike, ext::RefLike> {
    type Err = ext::reference::name::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::try_from(s)
    }
}

#[derive(Debug)]
pub struct Fetchspec(Refspec<ext::RefspecPattern, ext::RefspecPattern>);

impl<S, D> From<Refspec<S, D>> for Fetchspec
where
    S: Into<ext::RefspecPattern>,
    D: Into<ext::RefspecPattern>,
{
    fn from(spec: Refspec<S, D>) -> Self {
        Self(Refspec {
            src: spec.src.into(),
            dst: spec.dst.into(),
            force: spec.force,
        })
    }
}

impl TryFrom<&str> for Fetchspec {
    type Error = ext::reference::name::Error;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        Refspec::try_from(s).map(Self)
    }
}

impl FromStr for Fetchspec {
    type Err = ext::reference::name::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::try_from(s)
    }
}

impl Display for Fetchspec {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug)]
pub struct Pushspec(Refspec<ext::RefLike, ext::RefLike>);

impl<S, D> From<Refspec<S, D>> for Pushspec
where
    S: Into<ext::RefLike>,
    D: Into<ext::RefLike>,
{
    fn from(spec: Refspec<S, D>) -> Self {
        Self(Refspec {
            src: spec.src.into(),
            dst: spec.dst.into(),
            force: spec.force,
        })
    }
}

impl TryFrom<&str> for Pushspec {
    type Error = ext::reference::name::Error;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        Refspec::try_from(s).map(Self)
    }
}

impl FromStr for Pushspec {
    type Err = ext::reference::name::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::try_from(s)
    }
}

impl Display for Pushspec {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(f)
    }
}
