//! Offline challenges. Their records are deliberately separate from ranked matches.
use crate::puzzles::{self, PUZZLES};
use crate::{Active, Events, Game, Input, Kind, LevelRule, SpinKind, HEIGHT, HIDDEN, WIDTH};

/// 퍼즐 좌표는 보이는 판 기준으로 적고 이 값을 더해 실제 행으로 바꾼다.
const TOP: i8 = HIDDEN as i8;
/// 새 블록이 나오는 행 (보이는 판 바로 위)
const SPAWN_Y: i8 = HIDDEN as i8 - 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    TSpin,
    Combo,
    Cancel,
    Sprint,
    Blitz,
    /// 10줄마다 레벨이 오르고 150줄을 지우면 끝난다
    Marathon,
    /// 레벨이 오르지 않고 판이 차면 비우고 계속한다
    Zen,
    /// `PUZZLES` 의 번호
    Puzzle(u8),
}

/// 마라톤 완료 줄 수
pub const MARATHON_LINES: u32 = 150;

impl Mode {
    pub const ALL: [Self; 8] = [
        Self::TSpin,
        Self::Combo,
        Self::Cancel,
        Self::Sprint,
        Self::Blitz,
        Self::Marathon,
        Self::Zen,
        Self::Puzzle(0),
    ];
    pub fn title(self) -> &'static str {
        match self {
            Self::Marathon => "마라톤",
            Self::Zen => "Zen",
            Self::Puzzle(i) => PUZZLES.get(i as usize).map_or("퍼즐", |p| p.title),
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
        Self::Marathon=>"10줄마다 레벨이 올라 블록이 빨라집니다. 150줄을 지우거나 블록이 넘칠 때까지의 점수를 기록합니다.",
        Self::Zen=>"레벨이 오르지 않고, 판이 넘치면 비우고 이어서 합니다. 기록 없이 편하게 연습하세요.",
        Self::Puzzle(i)=>PUZZLES.get(i as usize).map_or("", |p| p.hint),
    }
    }
    pub fn exercise(self) -> bool {
        matches!(self, Self::TSpin | Self::Combo | Self::Cancel | Self::Puzzle(_))
    }
    /// 모드 선택 버튼 이름. 퍼즐은 여러 개를 하나의 버튼으로 묶는다.
    pub fn menu_title(self) -> &'static str {
        match self {
            Self::Puzzle(_) => "퍼즐",
            _ => self.title(),
        }
    }
    /// 최고 기록 저장 키
    pub fn key(self) -> String {
        match self {
            Self::TSpin => "spin".into(),
            Self::Combo => "combo".into(),
            Self::Cancel => "cancel".into(),
            Self::Sprint => "sprint".into(),
            Self::Blitz => "blitz".into(),
            Self::Marathon => "marathon".into(),
            Self::Zen => "zen".into(),
            Self::Puzzle(i) => format!("puzzle{i}"),
        }
    }
    /// 기록이 점수(클수록 좋음)인지, 시간(작을수록 좋음)인지
    pub fn higher_is_better(self) -> bool {
        matches!(self, Self::Blitz | Self::Marathon)
    }
    pub fn same_kind(self, other: Mode) -> bool {
        std::mem::discriminant(&self) == std::mem::discriminant(&other)
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
    /// 실제로 누른 키 수 (누르고 있어 반복된 이동은 세지 않는다)
    pub presses: u32,
    /// finesse 판정을 한 블록 수와 그중 최소 입력보다 많이 누른 블록 수
    pub judged: u32,
    pub faults: u32,
    piece_presses: u32,
    /// 보관이나 소프트 드롭을 쓴 블록은 판정하지 않는다
    piece_skip: bool,
    /// 지금 블록이 생성된 순간의 게임
    spawn: Option<Game>,
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
                    y: TOP + 12,
                });
                // 보이는 판의 아래쪽 8줄 안에 만든 퍼즐
                for (x, y) in [(4, 14), (3, 12), (3, 11), (2, 14), (2, 16), (3, 17)] {
                    game.board[(TOP as usize + y) * WIDTH + x] = 8;
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
                    y: SPAWN_Y,
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
                    y: SPAWN_Y,
                });
                game.receive_garbage(4);
            }
            Mode::Marathon => game.level_rule = LevelRule::Lines,
            Mode::Zen => game.level_rule = LevelRule::Fixed,
            Mode::Puzzle(i) => {
                let puzzle = &PUZZLES[(i as usize).min(PUZZLES.len() - 1)];
                game = puzzles::setup(puzzle, seed);
            }
            _ => {}
        }
        Self {
            mode,
            spawn: Some(game.clone()),
            game,
            ticks: 0,
            outcome: Outcome::Playing,
            last: Events::default(),
            presses: 0,
            judged: 0,
            faults: 0,
            piece_presses: 0,
            piece_skip: false,
        }
    }
    /// 블록당 키 입력 수
    pub fn kpp(&self) -> f64 {
        if self.game.stats.pieces == 0 { 0.0 } else { self.presses as f64 / self.game.stats.pieces as f64 }
    }
    /// 키를 새로 눌렀을 때. 누르고 있어 반복되는 이동은 `input` 을 쓴다.
    pub fn press(&mut self, input: Input) {
        if self.outcome != Outcome::Playing {
            return;
        }
        self.presses += 1;
        self.piece_presses += 1;
        if matches!(input, Input::Hold | Input::SoftDrop) {
            self.piece_skip = true;
        }
        self.input(input);
    }
    fn accept(&mut self, event: Events) {
        if event.locked {
            self.last = event;
        }
        if self.mode == Mode::Zen && self.game.dead {
            self.game.clear_board_and_continue();
        }
        if event.locked || self.mode == Mode::Zen && event.died {
            self.piece_presses = 0;
            self.piece_skip = false;
            self.spawn = Some(self.game.clone());
        }
        let puzzle = match self.mode {
            Mode::Puzzle(i) => PUZZLES.get(i as usize),
            _ => None,
        };
        let success = match self.mode {
            Mode::Marathon => self.game.stats.lines >= MARATHON_LINES,
            Mode::Zen => false,
            Mode::Puzzle(_) => puzzle.is_some_and(|p| p.goal.met(&event)),
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
            || (self.mode == Mode::Cancel && event.locked)
            || puzzle.is_some_and(|p| event.locked && self.game.stats.pieces >= p.pieces.len() as u32)
        {
            self.outcome = Outcome::Failed;
        }
    }
    pub fn input(&mut self, input: Input) {
        if self.outcome != Outcome::Playing {
            return;
        }
        let landing: Option<Vec<(i8, i8)>> = (input == Input::HardDrop)
            .then(|| self.game.ghost().map(|g| g.cells().collect()))
            .flatten();
        let e = self.game.apply_input(input);
        if e.locked && !self.piece_skip {
            if let (Some(cells), Some(start)) = (landing, self.spawn.as_ref()) {
                if let Some(min) = crate::finesse::min_presses(start, &cells) {
                    self.judged += 1;
                    if self.piece_presses > min {
                        self.faults += 1;
                    }
                }
            }
        }
        self.accept(e);
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
        match self.mode {
            Mode::Zen => None,
            // 마라톤은 블록이 넘쳐 끝나도 그때까지의 점수를 기록한다.
            Mode::Marathon => (self.outcome != Outcome::Playing).then_some(self.game.stats.score),
            Mode::Blitz => (self.outcome == Outcome::Success).then_some(self.game.stats.score),
            _ => (self.outcome == Outcome::Success).then_some(self.ticks.max(1)),
        }
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
    fn marathon_levels_by_lines_and_records_score_on_top_out() {
        let mut s = Session::new(Mode::Marathon, 1);
        s.advance_to(60 * 40); // 대전이라면 30초에 레벨 1이지만, 줄을 안 지웠으므로 0
        assert_eq!(s.outcome, Outcome::Playing);
        assert_eq!(s.game.level, 0);
        s.game.stats.lines = 25;
        s.advance_to(60 * 40 + 1);
        assert_eq!(s.game.level, 2);
        while s.outcome == Outcome::Playing {
            s.input(Input::HardDrop);
        }
        assert_eq!(s.outcome, Outcome::Failed);
        assert_eq!(s.record_value(), Some(s.game.stats.score));
        assert!(Mode::Marathon.higher_is_better());
    }
    #[test]
    fn zen_clears_the_board_instead_of_ending() {
        let mut s = Session::new(Mode::Zen, 1);
        for _ in 0..200 {
            s.input(Input::HardDrop);
        }
        assert_eq!(s.outcome, Outcome::Playing);
        assert!(!s.game.dead);
        assert_eq!(s.game.stats.pieces, 200);
        assert_eq!(s.record_value(), None);
    }
    #[test]
    fn puzzle_succeeds_with_solution_and_fails_on_wrong_drop() {
        let mut s = Session::new(Mode::Puzzle(6), 1); // 퍼펙트 클리어 1: O 하나
        for _ in 0..4 {
            s.press(Input::Right);
        }
        s.press(Input::HardDrop);
        assert_eq!(s.outcome, Outcome::Success);
        assert!(s.record_value().is_some());
        let mut s = Session::new(Mode::Puzzle(6), 1);
        s.press(Input::HardDrop);
        assert_eq!(s.outcome, Outcome::Failed);
        assert_eq!(Mode::Puzzle(3).key(), "puzzle3");
        assert!(Mode::Puzzle(3).same_kind(Mode::Puzzle(0)));
    }
    #[test]
    fn finesse_counts_extra_presses_but_not_held_repeats() {
        let mut s = Session::new(Mode::Zen, 1);
        // 벽까지: 한 번 누르고 누른 채 반복 -> 최소 입력과 같다
        s.press(Input::Left);
        for _ in 0..5 {
            s.input(Input::Left);
        }
        s.press(Input::HardDrop);
        assert_eq!((s.judged, s.faults), (1, 0));
        // 왼쪽 한 칸을 오른쪽 두 번, 왼쪽 세 번으로 가면 실수
        s.press(Input::Right);
        s.press(Input::Right);
        s.press(Input::Left);
        s.press(Input::Left);
        s.press(Input::Left);
        s.press(Input::HardDrop);
        assert_eq!((s.judged, s.faults), (2, 1));
        // 보관을 쓴 블록은 판정하지 않는다
        s.press(Input::Hold);
        s.press(Input::HardDrop);
        assert_eq!(s.judged, 2);
        assert_eq!(s.presses, 10);
        assert!((s.kpp() - 10.0 / 3.0).abs() < 1e-9);
    }
    #[test]
    fn exercise_pieces_start_where_the_player_can_see_them() {
        for mode in [Mode::TSpin, Mode::Combo, Mode::Cancel] {
            let s = Session::new(mode, 1);
            let a = s.game.active.unwrap();
            assert!(a.cells().any(|(_, y)| y as usize >= HIDDEN), "{mode:?} piece is hidden at {a:?}");
            assert!(s.game.board[..HIDDEN * WIDTH].iter().all(|&c| c == 0), "{mode:?} puzzle cells are hidden");
        }
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
