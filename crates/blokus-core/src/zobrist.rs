//! Minimal Zobrist hashing for incremental, undo-friendly board hashes.
//!
//! The hash XORs four kinds of randomized markers:
//!   - cell[player][bit]      : flipped for each cell a player owns.
//!   - piece_used[player][id] : flipped when a free piece is consumed.
//!   - last_mono[player]      : flipped when a player's last-placed becomes
//!                              the monomino (matters for end-of-game scoring).
//!   - side_to_move           : flipped every ply.

use std::sync::OnceLock;

use crate::pieces::NUM_FREE_PIECES;

pub struct ZobristTable {
    pub cell: [[u64; 256]; 2],
    pub piece_used: [[u64; NUM_FREE_PIECES]; 2],
    pub last_mono: [u64; 2],
    pub side_to_move: u64,
    /// One marker per consecutive-pass count (0, 1, or 2).
    pub pass_count: [u64; 3],
}

fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

static TABLE: OnceLock<ZobristTable> = OnceLock::new();

pub fn table() -> &'static ZobristTable {
    TABLE.get_or_init(|| {
        let mut s: u64 = 0xBADC_0FFE_E0DD_F00D;
        let mut cell = [[0u64; 256]; 2];
        for p in 0..2 {
            for i in 0..256 {
                cell[p][i] = splitmix64(&mut s);
            }
        }
        let mut piece_used = [[0u64; NUM_FREE_PIECES]; 2];
        for p in 0..2 {
            for i in 0..NUM_FREE_PIECES {
                piece_used[p][i] = splitmix64(&mut s);
            }
        }
        let last_mono = [splitmix64(&mut s), splitmix64(&mut s)];
        let side_to_move = splitmix64(&mut s);
        let pass_count = [
            splitmix64(&mut s),
            splitmix64(&mut s),
            splitmix64(&mut s),
        ];
        ZobristTable {
            cell,
            piece_used,
            last_mono,
            side_to_move,
            pass_count,
        }
    })
}

/// Recompute a board's zobrist from its current state — used as an oracle
/// for the incremental hash. Two equivalent positions reached by different
/// move orders must agree on this value.
pub fn compute_from_state(
    own: [&crate::bitboard::Bitboard; 2],
    pieces_left: [u32; 2],
    last_placed: [Option<u8>; 2],
    side_to_move: u8,
    consecutive_passes: u8,
) -> u64 {
    let z = table();
    let mut h: u64 = 0;
    let all_pieces_mask: u32 = (1u32 << NUM_FREE_PIECES) - 1;
    for p in 0..2 {
        for bit in own[p].iter_bits() {
            h ^= z.cell[p][bit];
        }
        let used = all_pieces_mask & !pieces_left[p];
        for id in 0..NUM_FREE_PIECES {
            if (used >> id) & 1 != 0 {
                h ^= z.piece_used[p][id];
            }
        }
        if last_placed[p] == Some(0) {
            // monomino id = 0; matches MONOMINO_ID in pieces.rs
            h ^= z.last_mono[p];
        }
    }
    if side_to_move == 1 {
        h ^= z.side_to_move;
    }
    let cp = consecutive_passes as usize;
    if cp > 0 && cp < z.pass_count.len() {
        h ^= z.pass_count[0] ^ z.pass_count[cp];
    }
    h
}
