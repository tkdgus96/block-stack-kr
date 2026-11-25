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

/// 실제 엔진 입력으로 갈 수 있는 자리 하나: 입력 순서, 고정 뒤 게임, 고정 이벤트
struct Placement {
    inputs: Vec<Input>,
    after: Game,
    ev: crate::game::Events,
}

/// 현재 블록으로 갈 수 있는 모든 자리를 엔진으로 직접 움직여 찾는다(회전 보정 포함).
/// `tucks` 이면 바닥까지 내린 뒤 옆으로 밀어 넣는 자리와 T-Spin 도 찾는다.
fn placements(game: &Game, tucks: bool) -> Vec<Placement> {
    use std::collections::{HashSet, VecDeque};
    let moves: &[Input] = if tucks {
        &[Input::Left, Input::Right, Input::RotateCw, Input::RotateCcw, Input::Rotate180, Input::SoftDrop]
    } else {
        &[Input::Left, Input::Right, Input::RotateCw, Input::RotateCcw]
    };
    let mut out: Vec<Placement> = Vec::new();
    let mut boards = HashSet::new();
    let mut seen = HashSet::new();
    // (게임, 입력 순서, 움직인 횟수). 바닥까지 내리기는 입력이 여러 개여도 한 번으로 센다.
    let mut queue = VecDeque::from([(game.clone(), Vec::new(), 0u8)]);
    while let Some((g, path, depth)) = queue.pop_front() {
        let Some(a) = g.active else { continue };
        if !seen.insert((a.x, a.y, a.rot, g.combat.last_rotation.is_some())) {
            continue;
        }
        let mut dropped = g.clone();
        let ev = dropped.apply_input(Input::HardDrop);
        if boards.insert((dropped.board.clone(), ev.spin)) {
            let mut inputs = path.clone();
            inputs.push(Input::HardDrop);
            out.push(Placement { inputs, after: dropped, ev });
        }
        // 바닥에서 옆으로 미는 길은 짧게만 본다.
        if depth >= 8 {
            continue;
        }
        for &m in moves {
            let mut n = g.clone();
            let mut p = path.clone();
            if m == Input::SoftDrop {
                // 바닥까지 한 번에 내린다. 실제 입력은 내려간 칸 수만큼 넣는다.
                loop {
                    let before = n.active;
                    n.apply_input(Input::SoftDrop);
                    if n.active == before { break; }
                    p.push(Input::SoftDrop);
                }
            } else {
                n.apply_input(m);
                p.push(m);
            }
            if n.active != g.active || n.combat.last_rotation != g.combat.last_rotation {
                queue.push_back((n, p, depth + 1));
            }
        }
    }
    out
}

/// 자리 하나의 점수: 판 모양 + 이번 고정으로 만든 공격
fn score(p: &Placement, st: &Style, attack_weight: i32) -> i32 {
    if p.after.dead {
        return i32::MIN / 2;
    }
    evaluate(&p.after.board, st) + p.ev.cleared as i32 * st.lines / 2 + p.ev.generated as i32 * attack_weight
        + if p.ev.spin == crate::game::SpinKind::Full { attack_weight * 2 } else { 0 }
}

/// 난이도별 봇 계획.
/// - 0 (쉬움): 현재 블록만, 회전×열 평가 (기존 방식)
/// - 1 (보통): 보관한 블록도 비교하고, 다음 블록까지 한 수 앞을 본다
/// - 2 (어려움): 보통 + 바닥에서 밀어 넣기·T-Spin 탐색, 공격량을 더 크게 친다
pub fn plan_level(game: &Game, st: &Style, level: u8) -> Vec<Input> {
    if level == 0 {
        return plan_with(game, st);
    }
    let tucks = level >= 2;
    let attack_weight = if level >= 2 { 30 } else { 8 };
    // (보관 여부, 시작 게임)
    let mut starts = vec![(false, game.clone())];
    if !game.hold_used {
        let mut held = game.clone();
        held.apply_input(Input::Hold);
        if held.active.is_some() && !held.dead {
            starts.push((true, held));
        }
    }
    let mut best: Option<(i32, Vec<Input>)> = None;
    for (hold, start) in starts {
        let mut first = placements(&start, tucks);
        first.sort_by_key(|p| std::cmp::Reverse(score(p, st, attack_weight)));
        // 한 수 앞은 앞쪽 후보 몇 개만 본다.
        for p in first.into_iter().take(6) {
            let now = score(&p, st, attack_weight);
            let next = placements(&p.after, false)
                .iter()
                .map(|q| score(q, st, attack_weight))
                .max()
                .unwrap_or(now);
            let total = now / 2 + next;
            if best.as_ref().is_none_or(|b| total > b.0) {
                let mut inputs = if hold { vec![Input::Hold] } else { Vec::new() };
                inputs.extend(p.inputs);
                best = Some((total, inputs));
            }
        }
    }
    best.map(|b| b.1).unwrap_or_else(|| vec![Input::HardDrop])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn play(level: u8, seed: u64, pieces: usize, garbage_every: usize) -> Game {
        let mut g = Game::new(seed);
        for n in 0..pieces {
            if g.dead { break; }
            if garbage_every > 0 && n % garbage_every == 0 { g.receive_garbage(2); g.elapsed += crate::game::GARBAGE_DELAY_TICKS; }
            for i in plan_level(&g, &Style::DEFAULT, level) {
                g.apply_input(i);
            }
        }
        g
    }

    #[test]
    fn harder_bots_attack_more_and_survive_pressure() {
        let easy = play(0, 5, 150, 6);
        let hard = play(2, 5, 150, 6);
        assert!(!hard.dead, "hard bot died under 2 lines every 6 pieces");
        assert!(hard.stats.sent > easy.stats.sent, "hard sent {} vs easy {}", hard.stats.sent, easy.stats.sent);
        let normal = play(1, 5, 150, 6);
        assert!(!normal.dead);
    }

    #[test]
    fn smart_plans_use_hold_and_only_engine_inputs() {
        let mut used_hold = false;
        let mut g = Game::new(9);
        for _ in 0..60 {
            let plan = plan_level(&g, &Style::DEFAULT, 2);
            assert_eq!(plan.last(), Some(&Input::HardDrop));
            used_hold |= plan.contains(&Input::Hold);
            let before = g.stats.pieces;
            for i in plan { g.apply_input(i); }
            assert_eq!(g.stats.pieces, before + 1, "each plan places exactly one piece");
        }
        assert!(used_hold);
    }

    #[test]
    fn hard_bot_finds_a_t_spin_double_slot() {
        // 퍼즐 "T스핀 더블 1" 판에서 T를 받으면 T스핀 더블을 고른다.
        let g = crate::puzzles::setup(&crate::puzzles::PUZZLES[0], 1);
        let mut h = g.clone();
        let plan = plan_level(&h, &Style::DEFAULT, 2);
        let mut last = Default::default();
        for i in plan { last = h.apply_input(i); }
        assert_eq!((last.spin, last.cleared), (crate::game::SpinKind::Full, 2));
    }

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
#[cfg(test)]
mod timing {
    #[test]
    #[ignore]
    fn time_hard_plan() {
        let mut g = crate::Game::new(3);
        let start = std::time::Instant::now();
        let mut worst = std::time::Duration::ZERO;
        for _ in 0..200 {
            let t = std::time::Instant::now();
            let plan = super::plan_level(&g, &super::Style::DEFAULT, 2);
            worst = worst.max(t.elapsed());
            for i in plan { g.apply_input(i); }
        }
        println!("avg {:?} worst {:?}", start.elapsed() / 200, worst);
    }
}
