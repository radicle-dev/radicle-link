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
}

impl<Marker: Ord, A: Eq> Replace<Marker, A> {
    pub fn new(val: A) -> Self
    where
        Marker: Default,
    {
        Replace {
            marker: Marker::default(),
            val,
        }
    }

    pub fn replace(&mut self, marker: Marker, val: A) -> Result<(), Error<Marker>> {
        if self.marker < marker {
            self.marker = marker;
            self.val = val;

            Ok(())
        } else if self.marker == marker && self.val != val {
            Err(Error::ConflictingMarker(marker))
        } else {
            // no-op
            Ok(())
        }
    }

    pub fn apply(&mut self, other: Self) -> Result<(), Error<Marker>> {
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
        left.replace(left.marker + 1, 2).expect("error occurred");
        let r1 = left.clone();

        // Replace 2 with 3
        left.replace(left.marker + 1, 2).expect("error occurred");
        let r2 = left.clone();

        let mut right = Replace { marker: 0, val: 1 };
        right.apply(r2).expect("error occurred");
        right.apply(r1).expect("error occurred");

        assert_eq!(left, right);
    }

    #[test]
    fn concurrent_replace() {
        // Left starts with 1
        let mut left: Replace<usize, u8> = Replace::new(1);

        // Replace 1 with 2
        left.replace(left.marker + 1, 2).expect("error occurred");

        // Right also starts with 1
        let mut right = Replace::new(1);

        // Replace 1 with 3
        right.replace(right.marker + 1, 3).expect("error occurred");

        let left_result = left.apply(right.clone());
        let right_result = right.apply(left.clone());

        // Concurrent replace will fail if the markers are the same.
        // The user should be expected to try apply their edit again with a
        // higher marker.
        assert_eq!(left_result, Err(Error::ConflictingMarker(left.marker)));
        assert_eq!(right_result, Err(Error::ConflictingMarker(right.marker)));
        assert!(left != right);

        // Right got there first and they are now synced
        right.replace(right.marker + 1, 3).expect("error occured");
        left.apply(right.clone()).expect("error occurred");
        assert_eq!(left, right);
    }
}
