//! Blokus Duo engine core.

pub mod bitboard;
pub mod board;
pub mod eval;
pub mod movegen;
pub mod pieces;
pub mod search;
pub mod tt;
pub mod zobrist;

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod smoke {
    use super::*;

    #[test]
    fn version_is_non_empty() {
        assert!(!version().is_empty());
    }
}
