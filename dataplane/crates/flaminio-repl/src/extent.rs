//! Extents (runs of blocks within a chunk) and sets of them.

use std::collections::BTreeMap;

use crate::types::Block;

/// Blocks `start .. start + len` of a chunk. Never empty.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Extent {
    pub start: Block,
    pub len: Block,
}

impl Extent {
    pub fn new(start: Block, len: Block) -> Self {
        debug_assert!(len > 0, "empty extent");
        Self { start, len }
    }

    /// One past the last block.
    pub fn end(&self) -> Block {
        self.start + self.len
    }

    pub fn overlaps(&self, o: &Extent) -> bool {
        self.start < o.end() && o.start < self.end()
    }

    pub fn contains(&self, b: Block) -> bool {
        self.start <= b && b < self.end()
    }

    pub fn blocks(&self) -> std::ops::Range<Block> {
        self.start..self.end()
    }
}

/// A set of blocks, kept as disjoint, non-adjacent extents in order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExtentSet {
    /// start → end (exclusive).
    runs: BTreeMap<Block, Block>,
}

impl ExtentSet {
    pub fn new() -> Self {
        Self::default()
    }

    /// The set of every block in `e`.
    pub fn of(e: Extent) -> Self {
        let mut s = Self::new();
        s.insert(e);
        s
    }

    pub fn clear(&mut self) {
        self.runs.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.runs.is_empty()
    }

    /// Number of blocks in the set.
    pub fn blocks(&self) -> u64 {
        self.runs.iter().map(|(s, e)| u64::from(e - s)).sum()
    }

    pub fn iter(&self) -> impl Iterator<Item = Extent> + '_ {
        self.runs.iter().map(|(&s, &e)| Extent::new(s, e - s))
    }

    pub fn contains_block(&self, b: Block) -> bool {
        self.runs
            .range(..=b)
            .next_back()
            .is_some_and(|(_, &e)| b < e)
    }

    /// Whether any block of `x` is in the set.
    pub fn overlaps(&self, x: &Extent) -> bool {
        self.runs
            .range(..x.end())
            .next_back()
            .is_some_and(|(_, &e)| e > x.start)
    }

    /// Adds every block of `x`, merging with neighbours.
    pub fn insert(&mut self, x: Extent) {
        let (mut start, mut end) = (x.start, x.end());
        // Runs that overlap or touch [start, end) are absorbed.
        let touching: Vec<(Block, Block)> = self
            .runs
            .range(..=end)
            .rev()
            .take_while(|(_, e)| **e >= start)
            .map(|(&s, &e)| (s, e))
            .collect();
        for (s, e) in touching {
            self.runs.remove(&s);
            start = start.min(s);
            end = end.max(e);
        }
        self.runs.insert(start, end);
    }

    /// Removes every block of `x`.
    pub fn remove(&mut self, x: &Extent) {
        let (start, end) = (x.start, x.end());
        let hit: Vec<(Block, Block)> = self
            .runs
            .range(..end)
            .rev()
            .take_while(|(_, e)| **e > start)
            .map(|(&s, &e)| (s, e))
            .collect();
        for (s, e) in hit {
            self.runs.remove(&s);
            if s < start {
                self.runs.insert(s, start);
            }
            if e > end {
                self.runs.insert(end, e);
            }
        }
    }

    pub fn extend(&mut self, other: &ExtentSet) {
        for x in other.iter() {
            self.insert(x);
        }
    }

    /// The parts of `x` that are not in the set, in order.
    pub fn missing_from(&self, x: &Extent) -> Vec<Extent> {
        let mut out = Vec::new();
        let mut at = x.start;
        let first = self
            .runs
            .range(..=x.start)
            .next_back()
            .map(|(&s, _)| s)
            .unwrap_or(x.start);
        for (&s, &e) in self.runs.range(first..x.end()) {
            if e <= at {
                continue;
            }
            if s > at {
                out.push(Extent::new(at, s - at));
            }
            at = at.max(e);
        }
        if at < x.end() {
            out.push(Extent::new(at, x.end() - at));
        }
        out
    }

    /// The set as extents of at most `max` blocks each.
    pub fn pieces(&self, max: Block) -> Vec<Extent> {
        let mut out = Vec::new();
        for x in self.iter() {
            let mut s = x.start;
            while s < x.end() {
                let len = max.min(x.end() - s);
                out.push(Extent::new(s, len));
                s += len;
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::Rng;
    use std::collections::BTreeSet;

    fn model(s: &ExtentSet) -> BTreeSet<Block> {
        s.iter().flat_map(|x| x.blocks()).collect()
    }

    /// Random operations against a plain set of blocks.
    #[test]
    fn matches_a_set_of_blocks() {
        let mut r = Rng::new(9);
        for _ in 0..300 {
            let mut s = ExtentSet::new();
            let mut m = BTreeSet::new();
            for _ in 0..40 {
                let start = r.range(0, 60) as Block;
                let x = Extent::new(start, r.range(1, 12) as Block);
                match r.range(0, 4) {
                    0 | 1 => {
                        s.insert(x);
                        m.extend(x.blocks());
                    }
                    2 => {
                        s.remove(&x);
                        for b in x.blocks() {
                            m.remove(&b);
                        }
                    }
                    3 => {
                        let want: BTreeSet<Block> = x.blocks().filter(|b| !m.contains(b)).collect();
                        let got: BTreeSet<Block> =
                            s.missing_from(&x).iter().flat_map(|p| p.blocks()).collect();
                        assert_eq!(got, want);
                        assert_eq!(s.overlaps(&x), x.blocks().any(|b| m.contains(&b)));
                    }
                    _ => {
                        let b = r.range(0, 75) as Block;
                        assert_eq!(s.contains_block(b), m.contains(&b));
                    }
                }
                assert_eq!(model(&s), m);
                // Canonical: runs are disjoint and not adjacent.
                let runs: Vec<Extent> = s.iter().collect();
                for w in runs.windows(2) {
                    assert!(w[0].end() < w[1].start, "{runs:?}");
                }
                assert_eq!(s.blocks(), m.len() as u64);
            }
            let pieces = s.pieces(5);
            assert!(pieces.iter().all(|p| p.len <= 5));
            let from_pieces: BTreeSet<Block> = pieces.iter().flat_map(|p| p.blocks()).collect();
            assert_eq!(from_pieces, m);
        }
    }
}
