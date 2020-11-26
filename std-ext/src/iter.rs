// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

pub trait IteratorExt {
    /// Converts an interator of triples into a triple of containers, analogous
    /// to [`Iterator::unzip`].
    ///
    /// # Examples
    ///
    /// ```
    /// use radicle_std_ext::iter::IteratorExt as _;
    ///
    /// let a = [(1, 2, 3), (4, 5, 6)];
    ///
    /// let (left, middle, right): (Vec<_>, Vec<_>, Vec<_>) = a.iter().copied().unzip3();
    ///
    /// assert_eq!(left, [1, 4]);
    /// assert_eq!(middle, [2, 5]);
    /// assert_eq!(right, [3, 6]);
    /// ```
    fn unzip3<A, B, C, FromA, FromB, FromC>(self) -> (FromA, FromB, FromC)
    where
        FromA: Default + Extend<A>,
        FromB: Default + Extend<B>,
        FromC: Default + Extend<C>,
        Self: Sized + Iterator<Item = (A, B, C)>,
    {
        fn extend<'a, A, B, C>(
            ts: &'a mut impl Extend<A>,
            us: &'a mut impl Extend<B>,
            vs: &'a mut impl Extend<C>,
        ) -> impl FnMut((), (A, B, C)) + 'a {
            move |(), (t, u, v)| {
                ts.extend_one(t);
                us.extend_one(u);
                vs.extend_one(v);
            }
        }

        let mut ts: FromA = Default::default();
        let mut us: FromB = Default::default();
        let mut vs: FromC = Default::default();

        let (lower_bound, _) = self.size_hint();
        if lower_bound > 0 {
            ts.extend_reserve(lower_bound);
            us.extend_reserve(lower_bound);
            vs.extend_reserve(lower_bound);
        }

        self.fold((), extend(&mut ts, &mut us, &mut vs));

        (ts, us, vs)
    }
}

impl<T> IteratorExt for T where T: Sized + Iterator {}
