// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{hash::Hash, marker::PhantomData};

use bloom_filter_simple::{BloomFilter as _, KMBloomFilter};
use siphasher::sip::SipHasher24;
use xxhash_rust::xxh3::Xxh3;

pub struct BloomFilter<T> {
    inner: KMBloomFilter<Xxh3, SipHasher24>,
    _marker: PhantomData<T>,
}

impl<T: Hash> BloomFilter<T> {
    pub fn new(capacity: usize, fp_rate: f64) -> Option<Self> {
        if capacity == 0 {
            None
        } else {
            Some(Self {
                inner: KMBloomFilter::new(capacity, fp_rate),
                _marker: PhantomData,
            })
        }
    }

    pub fn load(hashers: usize, filter: Vec<u8>) -> Option<Self> {
        KMBloomFilter::load(
            hashers,
            (filter.len() as f64 / hashers as f64).ceil() as usize,
            filter,
        )
        .map(|inner| Self {
            inner,
            _marker: PhantomData,
        })
    }

    pub fn hashers(&self) -> usize {
        self.inner.number_of_hashers()
    }

    pub fn approx_elements(&self) -> usize {
        self.inner.approximate_element_count().ceil() as usize
    }

    pub fn filter(&self) -> &[u8] {
        self.inner.bitset()
    }

    pub fn insert(&mut self, value: &T) {
        self.inner.insert(value)
    }

    pub fn contains(&self, value: &T) -> bool {
        self.inner.contains(value)
    }

    pub fn intersection(&self, other: &Self) -> Option<Self> {
        if self.inner.eq_configuration(&other.inner) {
            Some(Self {
                inner: self.inner.intersect(&other.inner),
                _marker: PhantomData,
            })
        } else {
            None
        }
    }

    pub fn union(&self, other: &Self) -> Option<Self> {
        if self.inner.eq_configuration(&other.inner) {
            Some(Self {
                inner: self.inner.union(&other.inner),
                _marker: PhantomData,
            })
        } else {
            None
        }
    }
}
