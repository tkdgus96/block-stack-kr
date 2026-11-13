//! 연습 상대용 간단한 자동 플레이어.
//!
//! 모든 회전 × 열 위치에 떨어뜨려 본 뒤 높이·구멍·울퉁불퉁함을 평가해
//! 최선의 위치를 고르고, 그 위치로 가는 입력 목록을 만든다.

use crate::game::{Active, Game, Input, CELL_EMPTY, HEIGHT, WIDTH};

fn fits(board: &[u8], a: &Active) -> bool {
    a.cells().all(|(x, y)| {
        x >= 0 && (x as usize) < WIDTH && y >= 0 && (y as usize) < HEIGHT && board[y as usize * WIDTH + x as usize] == CELL_EMPTY
    })
}

/// 봇마다 조금씩 다른 평가 가중치 (같은 시드에서도 다른 판을 만든다)
#[derive(Clone, Copy, Debug)]
pub struct Style {
    pub height: i32,
    pub holes: i32,
    pub bump: i32,
    pub lines: i32,
}

impl Style {
    pub const DEFAULT: Style = Style { height: 5, holes: 35, bump: 2, lines: 40 };

    pub fn variant(n: u32) -> Style {
        const V: [Style; 5] = [
            Style::DEFAULT,
            Style { height: 4, holes: 30, bump: 3, lines: 25 },
            Style { height: 6, holes: 40, bump: 1, lines: 55 },
            Style { height: 3, holes: 28, bump: 4, lines: 20 },
            Style { height: 5, holes: 45, bump: 2, lines: 70 },
        ];
        V[n as usize % V.len()]
    }
}

fn evaluate(board: &[u8], st: &Style) -> i32 {
    let mut heights = [0i32; WIDTH];
    let mut holes = 0;
    for x in 0..WIDTH {
        let mut seen = false;
        for y in 0..HEIGHT {
            let filled = board[y * WIDTH + x] != CELL_EMPTY;
            if filled && !seen {
                seen = true;
                heights[x] = (HEIGHT - y) as i32;
            } else if !filled && seen {
                holes += 1;
            }
        }
    }
    let agg: i32 = heights.iter().sum();
    let bump: i32 = heights.windows(2).map(|w| (w[0] - w[1]).abs()).sum();
    let max_h = *heights.iter().max().unwrap();
    -(agg * st.height) - holes * st.holes - bump * st.bump - max_h * 3
}

/// 현재 블록을 둘 위치를 정하고 필요한 입력 순서를 돌려준다.
pub fn plan(game: &Game) -> Vec<Input> {
    plan_with(game, &Style::DEFAULT)
}

pub fn plan_with(game: &Game, st: &Style) -> Vec<Input> {
    let Some(a) = game.active else { return vec![] };
    let mut best: Option<(i32, u8, i8)> = None;
    let mut scratch = game.board.clone();
    for rot in 0..4u8 {
        for x in -3..WIDTH as i8 + 3 {
            let mut p = Active { rot, x, ..a };
            if !fits(&game.board, &p) {
                continue;
            }
            while fits(&game.board, &Active { y: p.y + 1, ..p }) {
                p.y += 1;
            }
            scratch.copy_from_slice(&game.board);
            for (cx, cy) in p.cells() {
                scratch[cy as usize * WIDTH + cx as usize] = 1;
            }
            // 완성 줄 보너스
            let full = (0..HEIGHT).filter(|&y| scratch[y * WIDTH..(y + 1) * WIDTH].iter().all(|&c| c != CELL_EMPTY)).count() as i32;
            let score = evaluate(&scratch, st) + full * st.lines;
            if best.map_or(true, |b| score > b.0) {
                best = Some((score, rot, x));
            }
        }
    }
    let Some((_, rot, x)) = best else { return vec![Input::HardDrop] };
    let mut out = Vec::new();
    match rot {
        1 => out.push(Input::RotateCw),
        2 => {
            out.push(Input::RotateCw);
            out.push(Input::RotateCw)
        }
        3 => out.push(Input::RotateCcw),
        _ => {}
    }
    // 회전 보정으로 x가 달라질 수 있으므로 시뮬레이션해서 이동량을 계산
    let mut sim = game.clone();
    for i in &out {
        sim.apply_input(*i);
    }
    let cur_x = sim.active.map(|s| s.x).unwrap_or(a.x);
    let dx = x - cur_x;
    let step = if dx < 0 { Input::Left } else { Input::Right };
    for _ in 0..dx.unsigned_abs() {
        out.push(step);
    }
    out.push(Input::HardDrop);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bot_survives_a_while_and_clears_lines() {
        let mut g = Game::new(77);
        for _ in 0..300 {
            if g.dead {
                break;
            }
            for i in plan(&g) {
                g.apply_input(i);
            }
        }
        assert!(g.stats.lines > 20, "lines={}", g.stats.lines);
    }

    #[test]
    fn every_style_plays_reasonably_and_styles_differ() {
        let mut boards = Vec::new();
        for n in 0..5 {
            let st = Style::variant(n);
            let mut g = Game::new(77);
            for _ in 0..200 {
                if g.dead {
                    break;
                }
                for i in plan_with(&g, &st) {
                    g.apply_input(i);
                }
            }
            assert!(!g.dead && g.stats.lines > 10, "style {n}: lines={} dead={}", g.stats.lines, g.dead);
            boards.push(g.board.clone());
        }
        boards.dedup();
        assert!(boards.len() > 1, "스타일별로 다른 판이 나와야 함");
    }
}
