//! 키 입력 효율(finesse) 계산.
//!
//! 블록이 생성된 상태에서 원하는 자리에 바로 내려놓기까지 필요한 최소 키 입력 수를 구한다.
//! 좌우 끝까지 한 번에 미는 입력(DAS)은 한 번으로 센다. 소프트 드롭이 필요한 자리(밑으로
//! 밀어 넣기)는 판정하지 않는다.
use crate::{Active, Game, Input};
use std::collections::{HashSet, VecDeque};

#[derive(Clone, Copy)]
enum Step {
    Tap(Input),
    /// 벽까지 계속 이동 (키를 누르고 있기)
    Das(Input),
}

const STEPS: [Step; 7] = [
    Step::Tap(Input::Left),
    Step::Tap(Input::Right),
    Step::Das(Input::Left),
    Step::Das(Input::Right),
    Step::Tap(Input::RotateCw),
    Step::Tap(Input::RotateCcw),
    Step::Tap(Input::Rotate180),
];

fn sorted_cells(a: &Active) -> [(i8, i8); 4] {
    let mut c = [(0, 0); 4];
    for (slot, cell) in c.iter_mut().zip(a.cells()) {
        *slot = cell;
    }
    c.sort();
    c
}

/// 바로 내렸을 때 놓이는 칸
fn landing(game: &Game) -> Option<[(i8, i8); 4]> {
    game.ghost().map(|g| sorted_cells(&g))
}

/// `start` 의 현재 블록을 `target` 칸에 바로 내려놓는 최소 입력 수 (바로 내리기 포함).
/// 소프트 드롭 없이 갈 수 없으면 `None`.
pub fn min_presses(start: &Game, target: &[(i8, i8)]) -> Option<u32> {
    let mut want = [(0, 0); 4];
    if target.len() != 4 {
        return None;
    }
    want.copy_from_slice(target);
    want.sort();
    let first = start.active?;
    let mut seen = HashSet::new();
    seen.insert((first.x, first.y, first.rot));
    let mut queue = VecDeque::from([(start.clone(), 0u32)]);
    while let Some((game, cost)) = queue.pop_front() {
        if landing(&game) == Some(want) {
            return Some(cost + 1);
        }
        if cost >= 8 {
            continue;
        }
        for step in STEPS {
            let mut next = game.clone();
            match step {
                Step::Tap(input) => {
                    next.apply_input(input);
                }
                Step::Das(input) => {
                    for _ in 0..crate::WIDTH {
                        let before = next.active;
                        next.apply_input(input);
                        if next.active == before {
                            break;
                        }
                    }
                }
            }
            let Some(a) = next.active else { continue };
            if seen.insert((a.x, a.y, a.rot)) {
                queue.push_back((next, cost + 1));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Kind, HIDDEN};

    fn spawn(kind: Kind) -> Game {
        let mut g = Game::new(1);
        g.active = Some(Active { kind, rot: 0, x: kind.spawn_x(), y: HIDDEN as i8 - 1 });
        g
    }

    fn drop_cells(mut g: Game, inputs: &[Input]) -> Vec<(i8, i8)> {
        for &i in inputs {
            g.apply_input(i);
        }
        g.ghost().unwrap().cells().collect()
    }

    #[test]
    fn straight_drop_is_one_press() {
        let g = spawn(Kind::T);
        let target = drop_cells(g.clone(), &[]);
        assert_eq!(min_presses(&g, &target), Some(1));
    }

    #[test]
    fn wall_counts_das_as_one_press() {
        let g = spawn(Kind::T);
        let target = drop_cells(g.clone(), &[Input::Left, Input::Left, Input::Left]);
        assert_eq!(min_presses(&g, &target), Some(2), "DAS left + hard drop");
        let target = drop_cells(g.clone(), &[Input::Left]);
        assert_eq!(min_presses(&g, &target), Some(2), "one tap + hard drop");
    }

    #[test]
    fn symmetric_pieces_accept_either_rotation() {
        let g = spawn(Kind::I);
        let vertical = drop_cells(g.clone(), &[Input::RotateCcw]);
        // 같은 칸을 차지하는 회전이 여럿이면 그중 가장 짧은 입력을 쓴다.
        assert_eq!(min_presses(&g, &vertical), Some(2));
        // 180도 돌린 I는 바로 내린 것과 같은 칸에 놓이므로 한 번이면 된다.
        let flipped = drop_cells(g.clone(), &[Input::Rotate180]);
        assert_eq!(min_presses(&g, &flipped), Some(1));
    }

    #[test]
    fn unreachable_without_soft_drop_is_none() {
        let g = spawn(Kind::O);
        assert_eq!(min_presses(&g, &[(0, 0), (0, 1), (1, 0), (1, 1)]), None);
    }
}
