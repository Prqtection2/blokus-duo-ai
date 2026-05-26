//! Transposition table for the search.
//!
//! Single-slot direct-mapped table. Each entry stores the full key for
//! verification, the search value, the best move found, the search depth, and
//! the bound type. Replacement policy: keep the deeper entry on key collision,
//! always overwrite on key match.

use crate::board::Move;

#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum TtFlag {
    #[default]
    Empty,
    Exact,
    Lower,
    Upper,
}

#[derive(Copy, Clone, Default)]
pub struct TtEntry {
    pub key: u64,
    pub value: i32,
    pub best_move: Option<Move>,
    pub depth: u8,
    pub flag: TtFlag,
}

pub struct TranspositionTable {
    entries: Vec<TtEntry>,
    mask: usize,
}

impl TranspositionTable {
    /// Create a table with `1 << size_log2` slots (clamped to a minimum of 1).
    pub fn new(size_log2: u32) -> Self {
        let size = 1usize << size_log2.max(1);
        Self {
            entries: vec![TtEntry::default(); size],
            mask: size - 1,
        }
    }

    pub fn clear(&mut self) {
        for e in self.entries.iter_mut() {
            *e = TtEntry::default();
        }
    }

    pub fn capacity(&self) -> usize {
        self.entries.len()
    }

    pub fn probe(&self, key: u64) -> Option<TtEntry> {
        let idx = (key as usize) & self.mask;
        let e = self.entries[idx];
        if e.flag != TtFlag::Empty && e.key == key {
            Some(e)
        } else {
            None
        }
    }

    pub fn store(
        &mut self,
        key: u64,
        depth: u8,
        value: i32,
        flag: TtFlag,
        best_move: Option<Move>,
    ) {
        let idx = (key as usize) & self.mask;
        let cur = self.entries[idx];
        // Depth-preferred with always-replace on key match (we just searched a
        // node, so the new info supersedes any stale entry for the same key).
        let should_replace = cur.flag == TtFlag::Empty
            || cur.key == key
            || depth >= cur.depth;
        if should_replace {
            self.entries[idx] = TtEntry {
                key,
                value,
                best_move,
                depth,
                flag,
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_table_probe_returns_none() {
        let tt = TranspositionTable::new(8);
        assert!(tt.probe(0x1234).is_none());
    }

    #[test]
    fn round_trip_value() {
        let mut tt = TranspositionTable::new(8);
        tt.store(0xABCDEF, 5, 42, TtFlag::Exact, None);
        let e = tt.probe(0xABCDEF).expect("missing entry");
        assert_eq!(e.value, 42);
        assert_eq!(e.depth, 5);
        assert_eq!(e.flag, TtFlag::Exact);
    }

    #[test]
    fn deeper_entry_wins_on_collision() {
        let mut tt = TranspositionTable::new(4); // small for forced collisions
        let key_a: u64 = 0x100;
        let key_b: u64 = 0x100 + (1 << 4); // collides on low bits with key_a
        tt.store(key_a, 5, 100, TtFlag::Exact, None);
        // Shallower store on a colliding key should not evict the deeper one.
        tt.store(key_b, 3, 200, TtFlag::Exact, None);
        assert_eq!(tt.probe(key_a).unwrap().value, 100);
        // But a deeper store on the colliding key should evict.
        tt.store(key_b, 6, 200, TtFlag::Exact, None);
        assert!(tt.probe(key_a).is_none());
        assert_eq!(tt.probe(key_b).unwrap().value, 200);
    }
}
