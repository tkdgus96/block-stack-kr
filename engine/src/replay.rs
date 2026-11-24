//! 전체 경기 리플레이: 시드와 tick 별 입력·받은 공격만 저장하고, 같은 엔진으로 다시 실행한다.
//!
//! 서버 `Room::simulate` 의 순서를 그대로 따른다.
//! 한 tick 안에서 플레이어 순서대로 (그 tick 의 입력 적용 → `Game::tick`) 을 하고,
//! 모든 플레이어가 끝난 뒤 그 tick 에 받은 공격을 기록된 순서대로 넣는다.
use crate::{Game, Input};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReplayPlayer {
    pub id: u8,
    pub name: String,
    pub team: u8,
    pub bot: bool,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum ReplayAction {
    Input(Input),
    /// 공격을 받아 대기열에 넣었다
    Garbage(u8),
    /// 경기 중 나갔다. 이후로는 진행하지 않는다.
    Leave,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReplayEvent {
    /// 경기 시작(카운트다운 끝) 뒤 몇 번째 tick 인지. 1부터 시작한다.
    pub tick: u32,
    pub player: u8,
    pub action: ReplayAction,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MatchReplay {
    pub id: String,
    pub seed: u64,
    pub players: Vec<ReplayPlayer>,
    pub events: Vec<ReplayEvent>,
    pub end_tick: u32,
}

/// 리플레이를 원하는 tick 까지 진행하며 볼 수 있게 하는 실행기
pub struct Player {
    pub games: Vec<(u8, Game, bool)>,
    pub tick: u32,
    cursor: usize,
}

impl Player {
    pub fn new(replay: &MatchReplay) -> Self {
        Player {
            games: replay.players.iter().map(|p| (p.id, Game::new(replay.seed), true)).collect(),
            tick: 0,
            cursor: 0,
        }
    }

    /// `target` tick 까지 진행한다. 뒤로 가려면 새로 만들어 다시 진행한다.
    pub fn advance_to(&mut self, replay: &MatchReplay, target: u32) {
        let target = target.min(replay.end_tick);
        while self.tick < target {
            self.tick += 1;
            let t = self.tick;
            let events = &replay.events;
            // 이 tick 의 이벤트 범위
            let start = self.cursor;
            let mut end = start;
            while end < events.len() && events[end].tick <= t {
                end += 1;
            }
            for (id, game, active) in self.games.iter_mut() {
                if !*active {
                    continue;
                }
                for e in &events[start..end] {
                    if e.player != *id {
                        continue;
                    }
                    match e.action {
                        ReplayAction::Leave => *active = false,
                        ReplayAction::Input(input) if !game.dead => {
                            game.apply_input(input);
                        }
                        _ => {}
                    }
                }
                if *active && !game.dead {
                    game.tick();
                }
            }
            for e in &events[start..end] {
                if let ReplayAction::Garbage(lines) = e.action {
                    if let Some((_, game, true)) = self.games.iter_mut().find(|(id, _, _)| *id == e.player) {
                        game.receive_garbage(lines);
                    }
                }
            }
            self.cursor = end;
        }
    }

    pub fn game(&self, id: u8) -> Option<&Game> {
        self.games.iter().find(|(p, _, _)| *p == id).map(|(_, g, _)| g)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replays_inputs_and_garbage_in_server_order() {
        let replay = MatchReplay {
            id: "T".into(),
            seed: 5,
            players: vec![
                ReplayPlayer { id: 1, name: "a".into(), team: 0, bot: false },
                ReplayPlayer { id: 2, name: "b".into(), team: 1, bot: false },
            ],
            events: vec![
                ReplayEvent { tick: 1, player: 1, action: ReplayAction::Input(Input::HardDrop) },
                ReplayEvent { tick: 1, player: 2, action: ReplayAction::Garbage(3) },
                ReplayEvent { tick: 2, player: 2, action: ReplayAction::Leave },
            ],
            end_tick: 100,
        };
        // 직접 같은 순서로 진행한 결과와 같아야 한다.
        let (mut a, mut b) = (Game::new(5), Game::new(5));
        a.apply_input(Input::HardDrop);
        a.tick();
        b.tick();
        b.receive_garbage(3);
        for _ in 2..=100 {
            a.tick();
        }
        let mut p = Player::new(&replay);
        p.advance_to(&replay, 1);
        assert_eq!(p.game(2).unwrap().pending_total(), 3);
        p.advance_to(&replay, 500);
        assert_eq!(p.tick, 100, "stops at the end of the match");
        assert_eq!(p.game(1).unwrap(), &a);
        assert_eq!(p.game(2).unwrap().board, b.board, "left player stops after leaving");
    }
}
