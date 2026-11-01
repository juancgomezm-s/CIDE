// SPDX-License-Identifier: BSD-2-Clause
// Copyright 2013 Fredrik Ehnbom and contributors.
// Rust adaptation for JC-IDE, 2026.
// Ported from limetext/text region.go and selected region_test.go cases at
// a501ee418129fedf3fdca3b0d116231dd14af26c.
// Full upstream notice: ../../../third_party/limetext-text-LICENSE.

use std::fmt;
use std::ops::Range;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Region {
    pub a: usize,
    pub b: usize,
}

impl Region {
    pub const fn new(a: usize, b: usize) -> Self {
        Self { a, b }
    }
    pub fn begin(self) -> usize {
        self.a.min(self.b)
    }
    pub fn end(self) -> usize {
        self.a.max(self.b)
    }
    pub fn len(self) -> usize {
        self.end() - self.begin()
    }
    pub fn is_empty(self) -> bool {
        self.a == self.b
    }

    pub fn contains(self, position: usize) -> bool {
        position >= self.begin() && position <= self.end()
    }

    pub fn covers(self, other: Self) -> bool {
        self.contains(other.begin()) && other.end() <= self.end()
    }

    pub fn cover(self, other: Self) -> Self {
        if self.a <= self.b {
            Self::new(self.a.min(other.begin()), self.b.max(other.end()))
        } else {
            Self::new(self.a.max(other.end()), self.b.min(other.begin()))
        }
    }

    pub fn intersection(self, other: Self) -> Self {
        if !self.contains(other.begin()) && !other.contains(self.begin()) {
            return Self::default();
        }
        let overlap = Self::new(self.begin().max(other.begin()), self.end().min(other.end()));
        if overlap.is_empty() {
            Self::default()
        } else {
            overlap
        }
    }

    pub fn intersects(self, other: Self) -> bool {
        self == other || !self.intersection(other).is_empty()
    }

    pub fn byte_range(self) -> Range<usize> {
        self.begin()..self.end()
    }
}

impl fmt::Display for Region {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({}, {})", self.a, self.b)
    }
}

#[cfg(test)]
mod tests {
    use super::Region;

    #[test]
    fn upstream_intersection_cases() {
        let cases = [
            ((10, 20), (25, 35), (0, 0), false),
            ((25, 35), (10, 20), (0, 0), false),
            ((10, 25), (20, 35), (20, 25), true),
            ((20, 35), (10, 25), (20, 25), true),
            ((10, 25), (15, 20), (15, 20), true),
            ((15, 20), (10, 25), (15, 20), true),
            ((5, 10), (10, 23), (0, 0), false),
            ((5, 10), (5, 10), (5, 10), true),
            ((0, 0), (0, 0), (0, 0), true),
            ((1, 1), (1, 1), (0, 0), true),
            ((23, 24), (10, 23), (0, 0), false),
        ];
        for (a, b, expected, intersects) in cases {
            let a = Region::new(a.0, a.1);
            let b = Region::new(b.0, b.1);
            assert_eq!(a.intersection(b), Region::new(expected.0, expected.1));
            assert_eq!(a.intersects(b), intersects);
        }
    }

    #[test]
    fn upstream_cover_preserves_orientation() {
        let cases = [
            ((0, 1), (1, 0), (0, 1)),
            ((1, 0), (0, 1), (1, 0)),
            ((1, 0), (5, 10), (10, 0)),
            ((5, 10), (1, 0), (0, 10)),
        ];
        for (a, b, expected) in cases {
            assert_eq!(
                Region::new(a.0, a.1).cover(Region::new(b.0, b.1)),
                Region::new(expected.0, expected.1)
            );
        }
    }

    #[test]
    fn cursor_boundaries_are_inclusive_but_byte_range_is_half_open() {
        let backwards = Region::new(11, 10);
        assert_eq!(backwards.byte_range(), 10..11);
        assert!(backwards.contains(10));
        assert!(backwards.contains(11));
        assert!(!backwards.contains(9));
        assert!(!backwards.contains(12));
        assert!(backwards.covers(Region::new(10, 10)));
        assert_eq!(backwards.len(), 1);
        assert!(Region::new(10, 10).contains(10));
        assert!(!Region::new(5, 5).intersects(Region::new(0, 10)));
    }

    #[test]
    fn every_small_oriented_intersection_matches_a_set_of_selected_bytes() {
        for a in 0..=6 {
            for b in 0..=6 {
                let left_bytes: Vec<_> = (0..6)
                    .filter(|&p| (a <= p && p < b) || (b <= p && p < a))
                    .collect();
                for c in 0..=6 {
                    for d in 0..=6 {
                        let right_bytes: Vec<_> = (0..6)
                            .filter(|&p| (c <= p && p < d) || (d <= p && p < c))
                            .collect();
                        let common: Vec<_> = left_bytes
                            .iter()
                            .filter(|p| right_bytes.contains(p))
                            .copied()
                            .collect();
                        let expected = match (common.first(), common.last()) {
                            (Some(&first), Some(&last)) => Region::new(first, last + 1),
                            _ => Region::default(),
                        };
                        let left = Region::new(a, b);
                        let right = Region::new(c, d);
                        assert_eq!(left.intersection(right), expected);
                        assert_eq!(left.intersects(right), left == right || !common.is_empty());
                    }
                }
            }
        }
    }
}
