//! 정해진 판을 푸는 퍼즐. 모든 퍼즐은 테스트에서 실제로 풀리는지 확인한다.
use crate::{Events, Game, Kind, SpinKind, HEIGHT, WIDTH};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Goal {
    /// Full T-Spin 으로 정해진 줄 수를 지운다
    TSpin(u8),
    /// 4줄을 한 번에 지운다
    Tetris,
    /// 주어진 블록을 모두 써서 판을 비운다
    PerfectClear,
}

impl Goal {
    pub fn met(self, ev: &Events) -> bool {
        match self {
            Goal::TSpin(lines) => ev.locked && ev.spin == SpinKind::Full && ev.cleared == lines,
            Goal::Tetris => ev.locked && ev.cleared == 4,
            Goal::PerfectClear => ev.locked && ev.perfect_clear,
        }
    }
}

pub struct Puzzle {
    pub title: &'static str,
    pub hint: &'static str,
    /// 보이는 판의 아래쪽 줄. 위에서 아래 순서이고 마지막 줄이 맨 아랫줄이다. `#` 은 블록, `.` 은 빈칸.
    pub rows: &'static [&'static str],
    /// 쓸 수 있는 블록. 첫 블록부터 차례로 나온다.
    pub pieces: &'static [Kind],
    pub goal: Goal,
}

pub const PUZZLES: &[Puzzle] = &[
    Puzzle {
        title: "T스핀 더블 1",
        hint: "왼쪽 홈 위로 T를 옮긴 뒤 회전해서 끼워 넣으세요.",
        rows: &["#.........", "...#######", "#.########"],
        pieces: &[Kind::T],
        goal: Goal::TSpin(2),
    },
    Puzzle {
        title: "T스핀 더블 2",
        hint: "오른쪽 홈입니다. 회전 방향을 반대로 생각해 보세요.",
        rows: &[".........#", "#######...", "########.#"],
        pieces: &[Kind::T],
        goal: Goal::TSpin(2),
    },
    Puzzle {
        title: "T스핀 더블 3",
        hint: "가운데 홈입니다. 홈 바로 위에서 회전하세요.",
        rows: &["###.......", "##...#####", "###.######"],
        pieces: &[Kind::T],
        goal: Goal::TSpin(2),
    },
    Puzzle {
        title: "T스핀 싱글",
        hint: "아랫줄 하나만 지워집니다. T를 세운 채로 회전해 넣으세요.",
        rows: &["##........", "#...#####.", "##.#######"],
        pieces: &[Kind::T],
        goal: Goal::TSpin(1),
    },
    Puzzle {
        title: "T스핀 트리플",
        hint: "T를 눕힌 채 지붕 아래로 밀어 넣은 뒤 오른쪽으로 회전하면 회전 보정으로 홈에 들어갑니다.",
        rows: &["##........", "#....#####", "#.########", "#..#######", "#.########"],
        pieces: &[Kind::T],
        goal: Goal::TSpin(3),
    },
    Puzzle {
        title: "테트리스",
        hint: "I블록을 세워서 오른쪽 끝 빈 줄에 넣으세요.",
        rows: &["#########.", "#########.", "#########.", "#########."],
        pieces: &[Kind::I],
        goal: Goal::Tetris,
    },
    Puzzle {
        title: "퍼펙트 클리어 1",
        hint: "O블록 하나로 판을 비우세요.",
        rows: &["########..", "########.."],
        pieces: &[Kind::O],
        goal: Goal::PerfectClear,
    },
    Puzzle {
        title: "퍼펙트 클리어 2",
        hint: "I블록 두 개를 눕혀 쌓고 O블록으로 마무리하세요.",
        rows: &["####......", "####......"],
        pieces: &[Kind::I, Kind::I, Kind::O],
        goal: Goal::PerfectClear,
    },
    Puzzle {
        title: "퍼펙트 클리어 3",
        hint: "L블록 두 개를 맞물려 두 줄 네 칸을 채우고 O로 끝내세요.",
        rows: &["####......", "####......"],
        pieces: &[Kind::L, Kind::L, Kind::O],
        goal: Goal::PerfectClear,
    },
    Puzzle {
        title: "퍼펙트 클리어 4",
        hint: "O블록을 먼저 오른쪽에 놓고 I블록 두 개로 마무리하세요.",
        rows: &["......####", "......####"],
        pieces: &[Kind::O, Kind::I, Kind::I],
        goal: Goal::PerfectClear,
    },
];

/// 퍼즐 판을 만든다. 주어진 블록이 앞에서부터 나오고, 그 뒤는 일반 순서다.
pub fn setup(puzzle: &Puzzle, seed: u64) -> Game {
    let mut game = Game::new(seed);
    game.board.fill(0);
    let top = HEIGHT - puzzle.rows.len();
    for (r, row) in puzzle.rows.iter().enumerate() {
        for (x, ch) in row.bytes().take(WIDTH).enumerate() {
            if ch == b'#' {
                game.board[(top + r) * WIDTH + x] = crate::CELL_GARBAGE;
            }
        }
    }
    // 현재 블록과 다음 블록을 퍼즐 순서로 바꾼다.
    for &kind in puzzle.pieces[1..].iter().rev() {
        game.next.push_front(kind);
    }
    game.set_active_kind(puzzle.pieces[0]);
    game
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Input;
    use std::collections::HashSet;

    const MOVES: [Input; 6] = [Input::Left, Input::Right, Input::RotateCw, Input::RotateCcw, Input::Rotate180, Input::SoftDrop];

    /// 현재 블록으로 갈 수 있는 모든 자리에서 바로 내려 본다. (고정 결과 게임, 이벤트)
    fn placements(game: &Game) -> Vec<(Game, Events)> {
        let mut out = Vec::new();
        let mut boards = HashSet::new();
        let mut seen = HashSet::new();
        let mut stack = vec![game.clone()];
        while let Some(g) = stack.pop() {
            let Some(a) = g.active else { continue };
            // 회전 이력이 T-Spin 판정에 쓰이므로 상태에 넣는다.
            if !seen.insert((a.x, a.y, a.rot, g.combat.last_rotation.map(|r| r.kick_index))) { continue; }
            let mut dropped = g.clone();
            let ev = dropped.apply_input(Input::HardDrop);
            // 같은 판이 되는 자리는 한 번만 본다. T-Spin 여부가 다르면 다른 결과로 친다.
            if boards.insert((dropped.board.clone(), ev.spin == SpinKind::Full, ev.spin == SpinKind::Mini)) {
                out.push((dropped, ev));
            }
            for m in MOVES {
                let mut n = g.clone();
                // 소프트 드롭은 바닥까지 한 번에 내린다. 한 칸씩 보면 상태가 너무 많아진다.
                if m == Input::SoftDrop {
                    loop {
                        let before = n.active;
                        n.apply_input(m);
                        if n.active == before { break; }
                    }
                } else {
                    n.apply_input(m);
                }
                stack.push(n);
            }
        }
        out
    }

    /// `max_height`: 퍼즐 줄 수. 이보다 높이 쌓으면 그 판으로는 풀 수 없으므로 더 보지 않는다.
    fn solvable(game: &Game, goal: Goal, pieces_left: usize, max_height: u8) -> bool {
        if pieces_left == 0 { return false; }
        placements(game).into_iter().any(|(next, ev)| {
            goal.met(&ev) || (!next.dead && (ev.cleared == 0 || goal == Goal::PerfectClear) && next.stack_height() <= max_height
                && solvable(&next, goal, pieces_left - 1, max_height))
        })
    }

    #[test]
    fn every_puzzle_is_solvable_with_its_pieces() {
        for p in PUZZLES {
            assert!(p.rows.iter().all(|r| r.len() == WIDTH), "{}: row width", p.title);
            let game = setup(p, 1);
            assert_eq!(game.active.unwrap().kind, p.pieces[0], "{}", p.title);
            assert!(solvable(&game, p.goal, p.pieces.len(), p.rows.len() as u8), "{} cannot be solved", p.title);
        }
    }

    #[test]
    fn a_plain_hard_drop_does_not_solve_spin_puzzles() {
        for p in PUZZLES.iter().filter(|p| matches!(p.goal, Goal::TSpin(_))) {
            let mut game = setup(p, 1);
            let ev = game.apply_input(Input::HardDrop);
            assert!(!p.goal.met(&ev), "{} is solved without trying", p.title);
        }
    }
}
