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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error<Marker> {
    ConflictingMarker(Marker),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replace<Marker: Ord, A> {
    pub marker: Marker,
    pub val: A,
    conflicts: Vec<A>,
}

impl<Marker: Ord, A: Eq> Replace<Marker, A> {
    pub fn new(val: A) -> Self
    where
        Marker: Default,
    {
        Replace {
            marker: Marker::default(),
            val,
            conflicts: vec![],
        }
    }

    pub fn replace(&mut self, marker: Marker, val: A) {
        if self.marker < marker {
            self.marker = marker;
            self.val = val;
            self.conflicts = vec![];
        } else if self.marker == marker && self.val != val {
            self.conflicts.push(val);
        }
    }

    pub fn apply(&mut self, other: Self) {
        self.replace(other.marker, other.val)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commutative_ints() {
        // Start with 1
        let mut left: Replace<usize, u8> = Replace::new(1);
        // Replace 1 with 2
        left.replace(left.marker + 1, 2);
        let r1 = left.clone();

        // Replace 2 with 3
        left.replace(left.marker + 1, 2);
        let r2 = left.clone();

        let mut right = Replace::new(1);
        right.apply(r2);
        right.apply(r1);

        assert_eq!(left, right);
    }

    #[test]
    fn concurrent_replace() {
        // Left starts with 1
        let mut left: Replace<usize, u8> = Replace::new(1);

        // Replace 1 with 2
        left.replace(left.marker + 1, 2);

        // Right also starts with 1
        let mut right = Replace::new(1);

        // Replace 1 with 3
        right.replace(right.marker + 1, 3);

        left.apply(right.clone());
        right.apply(left.clone());

        // Concurrent replace will store conflicts locally.
        // The user should be expected try resolve the conflicts.
        assert!(left != right);
        assert_eq!(left.conflicts, vec![3]);
        assert_eq!(right.conflicts, vec![2]);

        // One way is to apply a higher marker.
        right.replace(right.marker + 1, 3);
        left.apply(right.clone());
        assert_eq!(left, right);
    }
}
