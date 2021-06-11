// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::fmt::{self, Debug};

use data::BoundedVec;
use sized_vec::Vec as SVec;
use thiserror::Error;
use typenum::{IsLessOrEqual, Unsigned, U1000, U100000, U23, U30};
use xorf::{Filter as _, Xor16};

use super::{SomeUrn, Urn};

/// Maximum number of elements permitted in a single [`Xor`] filter.
///
/// Currently: 100,000
pub type MaxElements = U100000;

/// Maximum number of fingerprints permitted in a serialised [`Xor`] filter.
///
/// Approx. `MaxElements * 1.23`, but not exactly for all choices of
/// `MaxElements`
// 123_030
// https://github.com/paholg/typenum/pull/136
pub type MaxFingerprints = typenum::op!(U100000 + (U23 * U1000) + U30);

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum BuildError<E: std::error::Error + Send + Sync + 'static> {
    #[error("too many elements")]
    TooManyElements,

    #[error(transparent)]
    Inner(#[from] E),
}

/// Compact representation of a potentially large number of local [`Urn`]s, with
/// approximate membership tests.
///
/// We use Lemire et.al.'s [Xor filter][xor] with 16-bit fingerprints, which
/// gives a false positive rate of < 0.02. The number of elements in the filter
/// is currently limited to **100,000**, which makes for a total size of about
/// **315KiB** on the wire when fully loaded.
///
/// The choice of Xor filters is a tradeoff: their size is proportional to the
/// number of elements (ie. no "unused" bits are transmitted), and generally
/// smaller than both Bloom and Cuckoo filters. We also don't need to be careful
/// about the load factor and resulting false positive rate. Membership tests
/// appear to be on-par with all but the most query-optimised Bloom filters, and
/// one order of magnitude faster than for Golomb-coded sets (which _may_ be
/// even more space-efficient, trading false positive rate). On the downside,
/// set intersection (which is ultimately what we're after) has to be computed
/// element-wise. There is also a significant construction complexity (space +
/// time), yet we can amortise this by caching, assuming the set of locally
/// stored URNs will be relatively stable in most cases.
///
/// [xor]: https://arxiv.org/abs/1912.08258
pub struct Xor {
    inner: Xor16,
}

impl Xor {
    pub fn contains(&self, urn: &SomeUrn) -> bool {
        self.inner.contains(&xor_hash(urn))
    }

    pub fn try_from_iter<T, E>(iter: T) -> Result<(Self, usize), BuildError<E>>
    where
        T: IntoIterator<Item = Result<SomeUrn, E>>,
        E: std::error::Error + Send + Sync + 'static,
    {
        let mut xs = Vec::with_capacity(MaxElements::USIZE);
        for x in iter {
            let urn = x?;
            xs.push(xor_hash(&urn));
            if xs.len() > MaxElements::USIZE {
                return Err(BuildError::TooManyElements);
            }
        }

        let elements = xs.len();
        let inner = Xor16::from(xs);

        Ok((Self { inner }, elements))
    }

    /// The number of **fingerprints** in the filter.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.len() == 0
    }
}

impl Clone for Xor {
    fn clone(&self) -> Self {
        Self {
            inner: Xor16 {
                seed: self.inner.seed,
                block_length: self.inner.block_length,
                fingerprints: self.inner.fingerprints.clone(),
            },
        }
    }
}

impl PartialEq for Xor {
    fn eq(&self, other: &Self) -> bool {
        let this = &self.inner;
        let that = &other.inner;

        this.seed == that.seed
            && this.block_length == that.block_length
            && this.fingerprints == that.fingerprints
    }
}

impl Debug for Xor {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Xor")
            .field("seed", &self.inner.seed)
            .field("block_length", &self.inner.block_length)
            .field("fingerprints", &self.inner.fingerprints)
            .finish()
    }
}

impl<N> From<&SVec<N, SomeUrn>> for Xor
where
    N: Unsigned + IsLessOrEqual<MaxElements>,
{
    fn from(svec: &SVec<N, SomeUrn>) -> Self {
        let inner = Xor16::from(svec.iter().map(xor_hash).collect::<Vec<_>>());
        Self { inner }
    }
}

impl minicbor::Encode for Xor {
    fn encode<W: minicbor::encode::Write>(
        &self,
        e: &mut minicbor::Encoder<W>,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        Encode {
            seed: self.inner.seed,
            block_length: self.inner.block_length,
            fingerprints: &self.inner.fingerprints,
        }
        .encode(e)
    }
}

impl<'b> minicbor::Decode<'b> for Xor {
    fn decode(d: &mut minicbor::Decoder) -> Result<Self, minicbor::decode::Error> {
        let Decode {
            seed,
            block_length,
            fingerprints,
        } = minicbor::Decode::decode(d)?;
        Ok(Self {
            inner: Xor16 {
                seed,
                block_length,
                fingerprints: fingerprints.into_inner().into_boxed_slice(),
            },
        })
    }
}

#[derive(minicbor::Encode)]
#[cbor(array)]
struct Encode<'a> {
    #[n(0)]
    seed: u64,
    #[n(1)]
    block_length: usize,
    #[n(2)]
    fingerprints: &'a [u16],
}

#[derive(minicbor::Decode)]
#[cbor(array)]
struct Decode {
    #[n(0)]
    seed: u64,
    #[n(1)]
    block_length: usize,
    #[n(2)]
    fingerprints: BoundedVec<MaxFingerprints, u16>,
}

fn xor_hash(urn: &SomeUrn) -> u64 {
    let SomeUrn::Git(Urn { id, path: _ }) = urn;
    let mut buf = [0u8; 8];
    buf.copy_from_slice(&id.as_bytes()[0..8]);
    u64::from_be_bytes(buf)
}
