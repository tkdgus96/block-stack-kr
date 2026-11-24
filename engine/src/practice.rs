//! Offline challenges. Their records are deliberately separate from ranked matches.
use crate::{Active, Events, Game, Input, Kind, SpinKind, HEIGHT, WIDTH};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    TSpin,
    Combo,
    Cancel,
    Sprint,
    Blitz,
}
impl Mode {
    pub const ALL: [Self; 5] = [
        Self::TSpin,
        Self::Combo,
        Self::Cancel,
        Self::Sprint,
        Self::Blitz,
    ];
    pub fn title(self) -> &'static str {
        match self {
            Self::TSpin => "T스핀 연습",
            Self::Combo => "콤보 연습",
            Self::Cancel => "공격 상쇄 연습",
            Self::Sprint => "40줄 스프린트",
            Self::Blitz => "2분 점수 도전",
        }
    }
    pub fn hint(self) -> &'static str {
        match self {
        Self::TSpin=>"준비된 T블록을 오른쪽으로 회전(↑)한 뒤 즉시 고정(Space)하세요. 마지막 회전 후 이동하면 T스핀이 해제됩니다.",
        Self::Combo=>"O블록을 왼쪽 끝으로 옮겨 고정하세요. 다음 O블록도 같은 곳에 놓아 두 번 연속 줄을 지우세요.",
        Self::Cancel=>"I블록을 오른쪽 회전(↑)하고 왼쪽 끝의 빈 세로줄에 내려놓으세요. 테트리스 공격으로 대기 중인 4줄을 상쇄합니다.",
        Self::Sprint=>"빈 보드에서 40줄을 최대한 빨리 지우세요. 40줄을 완료한 경기만 최고 기록에 반영됩니다.",
        Self::Blitz=>"120초 동안 점수를 모으세요. 시간이 끝나기 전에 보드가 차면 도전 실패로 기록되지 않습니다.",
    }
    }
    pub fn exercise(self) -> bool {
        matches!(self, Self::TSpin | Self::Combo | Self::Cancel)
    }
    pub fn key(self) -> &'static str {
        match self {
            Self::TSpin => "spin",
            Self::Combo => "combo",
            Self::Cancel => "cancel",
            Self::Sprint => "sprint",
            Self::Blitz => "blitz",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Playing,
    Success,
    Failed,
}
#[derive(Clone)]
pub struct Session {
    pub mode: Mode,
    pub game: Game,
    pub ticks: u32,
    pub outcome: Outcome,
    pub last: Events,
}
impl Session {
    pub fn new(mode: Mode, seed: u64) -> Self {
        let mut game = Game::new(seed);
        match mode {
            Mode::TSpin => {
                // A genuine fifth-kick SRS puzzle; rotation is performed by the player.
                game.active = Some(Active {
                    kind: Kind::T,
                    rot: 0,
                    x: 3,
                    y: 14,
                });
                for (x, y) in [(4, 16), (3, 14), (3, 13), (2, 16), (2, 18), (3, 19)] {
                    game.board[y * WIDTH + x] = 8;
                }
            }
            Mode::Combo => {
                for y in HEIGHT - 4..HEIGHT {
                    for x in 2..WIDTH {
                        game.board[y * WIDTH + x] = 8;
                    }
                }
                game.active = Some(Active {
                    kind: Kind::O,
                    rot: 0,
                    x: 4,
                    y: 1,
                });
                game.next.push_front(Kind::O);
            }
            Mode::Cancel => {
                for y in HEIGHT - 4..HEIGHT {
                    for x in 1..WIDTH {
                        game.board[y * WIDTH + x] = 8;
                    }
                }
                game.active = Some(Active {
                    kind: Kind::I,
                    rot: 0,
                    x: 3,
                    y: 1,
                });
                game.receive_garbage(4);
            }
            _ => {}
        }
        Self {
            mode,
            game,
            ticks: 0,
            outcome: Outcome::Playing,
            last: Events::default(),
        }
    }
    fn accept(&mut self, event: Events) {
        if event.locked {
            self.last = event;
        }
        let success = match self.mode {
            Mode::TSpin => event.locked && event.spin == SpinKind::Full,
            Mode::Combo => event.locked && event.cleared > 0 && event.combo >= 1,
            Mode::Cancel => self.game.stats.cancelled >= 4,
            Mode::Sprint => self.game.stats.lines >= 40,
            Mode::Blitz => false,
        };
        if success {
            self.outcome = Outcome::Success;
        } else if self.game.dead
            || (self.mode == Mode::TSpin && event.locked)
            || (self.mode == Mode::Combo && event.locked && event.cleared == 0)
            || (self.mode == Mode::Cancel && event.garbage_applied > 0)
        {
            self.outcome = Outcome::Failed;
        }
    }
    pub fn input(&mut self, input: Input) {
        if self.outcome == Outcome::Playing {
            let e = self.game.apply_input(input);
            self.accept(e);
        }
    }
    /// Absolute wall-clock-derived ticks: backgrounding cannot extend the Blitz timer.
    pub fn advance_to(&mut self, ticks: u32) {
        if self.outcome != Outcome::Playing {
            return;
        }
        let target = if self.mode == Mode::Blitz {
            ticks.min(7200)
        } else {
            ticks
        };
        // Exercises have no gravity or time pressure. Their timing still measures completion.
        if self.mode.exercise() {
            self.ticks = target;
            return;
        }
        while self.ticks < target && self.outcome == Outcome::Playing {
            self.ticks += 1;
            let e = self.game.tick();
            self.accept(e);
            if self.mode == Mode::Blitz && self.ticks == 7200 && self.outcome == Outcome::Playing {
                self.outcome = Outcome::Success;
            }
        }
    }
    pub fn record_value(&self) -> Option<u32> {
        (self.outcome == Outcome::Success).then_some(if self.mode == Mode::Blitz {
            self.game.stats.score
        } else {
            self.ticks.max(1)
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn spin_is_solved_by_real_rotation() {
        let mut s = Session::new(Mode::TSpin, 1);
        s.input(Input::RotateCw);
        s.input(Input::HardDrop);
        assert_eq!(s.outcome, Outcome::Success);
        assert_eq!(s.last.spin, SpinKind::Full);
    }
    #[test]
    fn combo_requires_two_successive_clears() {
        let mut s = Session::new(Mode::Combo, 1);
        for _ in 0..4 {
            s.input(Input::Left);
        }
        s.input(Input::HardDrop);
        assert_eq!(s.outcome, Outcome::Playing);
        for _ in 0..4 {
            s.input(Input::Left);
        }
        s.input(Input::HardDrop);
        assert_eq!(s.outcome, Outcome::Success);
    }
    #[test]
    fn tetris_cancels_four_pending_rows() {
        let mut s = Session::new(Mode::Cancel, 1);
        s.input(Input::RotateCw);
        for _ in 0..6 {
            s.input(Input::Left);
        }
        s.input(Input::HardDrop);
        assert_eq!(s.outcome, Outcome::Success);
        assert_eq!(s.game.stats.cancelled, 4);
        assert_eq!(s.game.pending_total(), 0);
    }
    #[test]
    fn unfinished_and_dead_games_do_not_record() {
        let mut s = Session::new(Mode::Sprint, 1);
        assert_eq!(s.record_value(), None);
        for _ in 0..100 {
            s.input(Input::HardDrop);
        }
        assert_eq!(s.outcome, Outcome::Failed);
        assert_eq!(s.record_value(), None);
    }
    #[test]
    fn blitz_deadline_is_exact_and_cannot_be_extended() {
        let mut s = Session::new(Mode::Blitz, 1);
        s.game.active = None;
        s.advance_to(7199);
        assert_eq!(s.record_value(), None);
        s.advance_to(8000);
        assert_eq!(s.ticks, 7200);
        assert_eq!(s.outcome, Outcome::Success);
        assert_eq!(s.record_value(), Some(0));
        s.advance_to(9000);
        assert_eq!(s.ticks, 7200);
    }
    #[test]
    fn sprint_finishes_only_at_forty_lines() {
        let mut s = Session::new(Mode::Sprint, 1);
        s.game.stats.lines = 39;
        assert_eq!(s.record_value(), None);
        for x in 1..WIDTH {
            s.game.board[(HEIGHT - 1) * WIDTH + x] = 8;
        }
        s.game.active = Some(Active {
            kind: Kind::I,
            rot: 1,
            x: -2,
            y: 1,
        });
        s.input(Input::HardDrop);
        assert_eq!(s.outcome, Outcome::Success);
        assert!(s.record_value().is_some());
    }
    #[test]
    fn failed_spin_can_be_retried_from_same_board() {
        let mut s = Session::new(Mode::TSpin, 4);
        s.input(Input::HardDrop);
        assert_eq!(s.outcome, Outcome::Failed);
        let a = Session::new(Mode::TSpin, 4);
        let b = Session::new(Mode::TSpin, 4);
        assert_eq!(a.game, b.game);
    }
}
