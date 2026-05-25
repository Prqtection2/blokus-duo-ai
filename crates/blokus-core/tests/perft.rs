//! Cross-generator correctness tests.
//!
//! The plan's DoD asks for full perft(1..=4) agreement between the corner-anchored
//! `generate_moves` and the brute-force `generate_moves_reference`. Full perft
//! at depth 4 from the start is ~10^11 nodes, which is infeasible. Instead we:
//!   * full perft agreement at depth 1 and depth 2 (~160K nodes, ~seconds),
//!   * random-game agreement: at every node visited during 50 random games
//!     (~40 plies each), both generators must produce the same move set.
//!
//! Combined, this catches the same class of bugs that full d=4 perft would,
//! without the exponential cost. Bump `DEEP_PERFT_DEPTH` and `RANDOM_GAMES`
//! for a stricter (and slower) sweep.

use blokus_core::bitboard::Bitboard;
use blokus_core::board::{Board, Move};
use blokus_core::movegen::{generate_moves, generate_moves_reference};

const RANDOM_GAMES: usize = 50;
const DEEP_PERFT_DEPTH: u32 = 2;

fn perft<F: Fn(&Board) -> Vec<Move>>(board: &mut Board, depth: u32, gen: &F) -> u64 {
    if depth == 0 {
        return 1;
    }
    let moves = gen(board);
    if moves.is_empty() {
        if board.game_over() {
            return 1;
        }
        board.make_move(&Move::Pass);
        let v = perft(board, depth - 1, gen);
        board.unmake_move();
        return v;
    }
    let mut total = 0;
    for mv in &moves {
        board.make_move(mv);
        total += perft(board, depth - 1, gen);
        board.unmake_move();
    }
    total
}

fn move_key(m: &Move) -> (u8, [u64; 4]) {
    match *m {
        Move::Place { piece, placement } => (piece, placement.0),
        Move::Pass => (255, [0; 4]),
    }
}

fn assert_same_moves(board: &Board) {
    let mut a = generate_moves(board);
    let mut b = generate_moves_reference(board);
    a.sort_by_key(move_key);
    b.sort_by_key(move_key);
    if a != b {
        panic!(
            "fast and reference generators disagree at ply={}, stm={}:\n  fast: {} moves\n  ref:  {} moves",
            board.ply, board.side_to_move, a.len(), b.len()
        );
    }
}

#[test]
fn perft_depth_1_matches() {
    let mut fast_board = Board::new();
    let mut slow_board = Board::new();
    let na = perft(&mut fast_board, 1, &generate_moves);
    let nb = perft(&mut slow_board, 1, &generate_moves_reference);
    assert_eq!(na, nb, "perft(1) mismatch: fast={na} ref={nb}");
    assert!(na > 0);
}

#[test]
fn perft_depth_2_matches() {
    let mut fast_board = Board::new();
    let mut slow_board = Board::new();
    let na = perft(&mut fast_board, DEEP_PERFT_DEPTH, &generate_moves);
    let nb = perft(&mut slow_board, DEEP_PERFT_DEPTH, &generate_moves_reference);
    assert_eq!(na, nb, "perft(2) mismatch: fast={na} ref={nb}");
}

#[test]
fn random_games_agreement_and_make_unmake_property() {
    let mut rng_state: u64 = 0xCAFE_F00D_DEAD_BEEF;
    fn next(s: &mut u64) -> u64 {
        *s = s.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = *s;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    #[derive(Clone, PartialEq, Eq, Debug)]
    struct Snap {
        side_to_move: u8,
        ply: u32,
        occupied: Bitboard,
        own: [Bitboard; 2],
        forbidden: [Bitboard; 2],
        corners: [Bitboard; 2],
        pieces_left: [u32; 2],
        last_placed: [Option<u8>; 2],
        passes: u8,
        zobrist: u64,
    }
    fn snap(b: &Board) -> Snap {
        Snap {
            side_to_move: b.side_to_move,
            ply: b.ply,
            occupied: b.occupied,
            own: b.own,
            forbidden: b.forbidden,
            corners: b.corners,
            pieces_left: b.pieces_left,
            last_placed: b.last_placed,
            passes: b.consecutive_passes,
            zobrist: b.zobrist,
        }
    }

    for game in 0..RANDOM_GAMES {
        let mut board = Board::new();
        let mut snaps: Vec<Snap> = Vec::new();
        let mut history: Vec<Move> = Vec::new();

        // Play until both players have passed.
        for _ in 0..50 {
            if board.game_over() {
                break;
            }
            assert_same_moves(&board);
            snaps.push(snap(&board));
            let mvs = generate_moves(&board);
            let mv = if mvs.is_empty() {
                Move::Pass
            } else {
                let idx = (next(&mut rng_state) as usize) % mvs.len();
                mvs[idx]
            };
            board.make_move(&mv);
            history.push(mv);
        }

        // Unmake all the way back, asserting state matches each pre-move snap.
        while history.pop().is_some() {
            board.unmake_move();
            let want = snaps.pop().unwrap();
            let got = snap(&board);
            assert_eq!(got, want,
                "game {game}: make/unmake symmetry broke at ply {}", got.ply);
        }
        // Should be back at the initial state.
        assert_eq!(snap(&board), snap(&Board::new()),
            "game {game}: unmade board does not equal a freshly constructed board");
    }
}
