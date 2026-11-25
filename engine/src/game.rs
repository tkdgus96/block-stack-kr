//! 단일 플레이어 게임 상태. 렌더링·네트워크와 완전히 분리된 순수 로직.
//!
//! 시간은 "tick" 단위(기본 60 tick = 1초)로만 진행되며, 실제 시계에 의존하지 않는다.
//! 같은 시드와 같은 입력 순서를 주면 어떤 플랫폼에서든 같은 결과가 나온다.

use crate::piece::{kicks, Kind, ALL_KINDS};
use crate::rng::Rng;
use std::collections::VecDeque;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

pub const WIDTH: usize = 10;
/// 화면에 보이지 않는 영역. 보이는 판 위로 10줄을 두어, 방해줄에 밀려 올라간
/// 블록이 바로 판 밖으로 사라져 패배하지 않게 한다. 가이드라인 게임은 20줄이지만
/// 내 상태 메시지 크기(400바이트 미만)를 지키려고 10줄로 정했다.
/// 블록은 이 영역의 맨 아래(보이는 판 바로 위)에서 생성된다.
pub const HIDDEN: usize = 10;
pub const VISIBLE: usize = 20;
pub const HEIGHT: usize = HIDDEN + VISIBLE;
pub const VISIBLE_CELLS: usize = WIDTH * VISIBLE;

pub const TICKS_PER_SECOND: u32 = 60;
pub const LOCK_DELAY: u16 = 30;
pub const MAX_LOCK_RESETS: u8 = 15;
pub const PREVIEW: usize = 5;
/// 받은 공격이 대기열에 들어온 뒤 게임판에 올라올 수 있을 때까지의 tick (0.5초).
/// 그 사이에 줄을 지우면 상쇄할 수 있다.
pub const GARBAGE_DELAY_TICKS: u32 = 30;
/// 한 번에 올라오는 최대 방해줄
pub const MAX_GARBAGE_PER_LOCK: u8 = 8;
/// 이 tick 수마다 레벨 상승 (대전 시 속도 증가)
pub const LEVEL_UP_TICKS: u32 = 30 * TICKS_PER_SECOND;
pub const MAX_LEVEL: u8 = 14;

/// 레벨별 한 칸 낙하에 필요한 tick 수
const GRAVITY: [u16; MAX_LEVEL as usize + 1] = [60, 48, 38, 30, 24, 19, 15, 12, 9, 7, 5, 4, 3, 2, 1];

/// 셀 코드: 0 빈칸, 1..=7 블록, 8 방해줄, 9 고스트(표시 전용)
pub const CELL_EMPTY: u8 = 0;
pub const CELL_GARBAGE: u8 = 8;
pub const CELL_GHOST: u8 = 9;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum Input {
    Left,
    Right,
    RotateCw,
    RotateCcw,
    SoftDrop,
    HardDrop,
    Hold,
    /// 반드시 맨 뒤에 둔다. postcard 는 enum 순서 번호로 직렬화하므로
    /// 앞 값의 번호가 바뀌면 이전 클라이언트와 입력이 어긋난다.
    Rotate180,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Active {
    pub kind: Kind,
    pub rot: u8,
    pub x: i8,
    pub y: i8,
}

impl Active {
    pub fn cells(&self) -> impl Iterator<Item = (i8, i8)> + '_ {
        self.kind.cells(self.rot).into_iter().map(move |(dx, dy)| (self.x + dx, self.y + dy))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum SpinKind { #[default] None, Mini, Full }

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct RotationInfo { pub kick_index: u8 }

#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CombatState {
    pub last_rotation: Option<RotationInfo>,
    pub back_to_back: u16,
    /// `pending_garbage` 뒤쪽 원소와 순서대로 짝을 이루는 도착 tick(`elapsed` 기준).
    /// 레거시 Game 직렬화에는 들어가지 않으므로, 개수가 모자라면 앞쪽 원소는 이미 도착한 것으로 본다.
    pub garbage_ready_at: VecDeque<u32>,
}

/// 한 번의 입력/틱에서 발생한 결과
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Events {
    pub locked: bool,
    pub cleared: u8,
    /// 상쇄 후 상대에게 보낼 공격 줄 수
    pub attack: u8,
    /// 받은 공격을 상쇄한 줄 수
    pub cancelled: u8,
    pub garbage_applied: u8,
    pub died: bool,
    pub spin: SpinKind,
    pub combo: i8,
    pub b2b: u16,
    pub back_to_back: bool,
    pub perfect_clear: bool,
    /// Generated attack before pending-garbage cancellation.
    pub generated: u8,
}

impl Events {
    fn merge(&mut self, o: Events) {
        self.locked |= o.locked;
        if o.locked {
            self.cleared = o.cleared;
            self.spin = o.spin;
            self.combo = o.combo;
            self.b2b = o.b2b;
            self.back_to_back = o.back_to_back;
            self.perfect_clear = o.perfect_clear;
        }
        self.generated = self.generated.saturating_add(o.generated);
        self.attack += o.attack;
        self.cancelled += o.cancelled;
        self.garbage_applied += o.garbage_applied;
        self.died |= o.died;
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Stats {
    pub score: u32,
    pub lines: u32,
    pub pieces: u32,
    pub sent: u32,
    pub received: u32,
    pub cancelled: u32,
    pub max_combo: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Game {
    /// HEIGHT × WIDTH, 행 우선. 0행이 맨 위(숨김 영역).
    pub board: Vec<u8>,
    pub active: Option<Active>,
    pub hold: Option<Kind>,
    pub hold_used: bool,
    pub next: VecDeque<Kind>,
    bag: Vec<Kind>,
    bag_rng: Rng,
    garbage_rng: Rng,
    /// 대기 중인 방해줄 묶음 (도착 순)
    pub pending_garbage: VecDeque<u8>,
    gravity_counter: u16,
    lock_counter: u16,
    lock_resets: u8,
    pub elapsed: u32,
    pub level: u8,
    pub combo: i8,
    pub dead: bool,
    pub stats: Stats,
    /// 상태가 바뀔 때마다 증가 (변경 감지용)
    pub rev: u32,
    /// Sidecar metadata: never change the legacy Game postcard wire layout.
    #[cfg_attr(feature = "serde", serde(skip))]
    pub combat: CombatState,
    /// 레벨이 오르는 방식. 혼자 하는 모드에서만 바꾸므로 전송하지 않는다.
    #[cfg_attr(feature = "serde", serde(skip))]
    pub level_rule: LevelRule,
}

/// 중력 레벨이 오르는 방식
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LevelRule {
    /// 대전 기본값: 30초마다 또는 10줄마다 중 더 높은 쪽
    #[default]
    TimeOrLines,
    /// 마라톤: 10줄마다 한 단계
    Lines,
    /// Zen: 레벨이 오르지 않는다
    Fixed,
}

impl Game {
    pub fn new(seed: u64) -> Self {
        let mut g = Game {
            board: vec![CELL_EMPTY; WIDTH * HEIGHT],
            active: None,
            hold: None,
            hold_used: false,
            next: VecDeque::with_capacity(PREVIEW + 7),
            bag: Vec::with_capacity(7),
            bag_rng: Rng::new(seed),
            garbage_rng: Rng::new(seed.rotate_left(17) ^ 0xA5A5),
            pending_garbage: VecDeque::new(),
            gravity_counter: 0,
            lock_counter: 0,
            lock_resets: 0,
            elapsed: 0,
            level: 0,
            combo: -1,
            dead: false,
            stats: Stats::default(),
            rev: 0,
            combat: CombatState::default(),
            level_rule: LevelRule::default(),
        };
        g.fill_next();
        g.spawn_next();
        g
    }

    #[inline]
    pub fn cell(&self, x: usize, y: usize) -> u8 {
        self.board[y * WIDTH + x]
    }

    fn fill_next(&mut self) {
        while self.next.len() < PREVIEW + 1 {
            if self.bag.is_empty() {
                self.bag.extend_from_slice(&ALL_KINDS);
                // Fisher–Yates
                for i in (1..self.bag.len()).rev() {
                    let j = self.bag_rng.below(i as u32 + 1) as usize;
                    self.bag.swap(i, j);
                }
            }
            let k = self.bag.pop().unwrap();
            self.next.push_back(k);
        }
    }

    fn fits(&self, a: &Active) -> bool {
        a.cells().all(|(x, y)| {
            x >= 0 && (x as usize) < WIDTH && y >= 0 && (y as usize) < HEIGHT && self.cell(x as usize, y as usize) == CELL_EMPTY
        })
    }

    fn spawn(&mut self, kind: Kind) {
        self.combat.last_rotation = None;
        let a = Active { kind, rot: 0, x: kind.spawn_x(), y: (HIDDEN as i8) - 1 };
        self.gravity_counter = 0;
        self.lock_counter = 0;
        self.lock_resets = 0;
        if self.fits(&a) {
            self.active = Some(a);
        } else {
            self.active = None;
            self.dead = true;
        }
    }

    fn spawn_next(&mut self) {
        let k = self.next.pop_front().unwrap();
        self.fill_next();
        self.spawn(k);
    }

    fn grounded(&self) -> bool {
        match self.active {
            Some(a) => !self.fits(&Active { y: a.y + 1, ..a }),
            None => false,
        }
    }

    fn try_move(&mut self, dx: i8, dy: i8) -> bool {
        let Some(a) = self.active else { return false };
        let n = Active { x: a.x + dx, y: a.y + dy, ..a };
        if self.fits(&n) {
            self.active = Some(n);
            if dx != 0 || dy != 0 { self.combat.last_rotation = None; }
            true
        } else {
            false
        }
    }

    fn try_rotate(&mut self, dir: i8) -> bool {
        let Some(a) = self.active else { return false };
        if a.kind == Kind::O { return false; }
        let rot = ((a.rot as i8 + dir).rem_euclid(4)) as u8;
        for (kick_index, &(dx, dy)) in kicks(a.kind, a.rot, rot).iter().enumerate() {
            let n = Active { rot, x: a.x + dx, y: a.y + dy, ..a };
            if self.fits(&n) {
                self.active = Some(n);
                self.combat.last_rotation = Some(RotationInfo { kick_index: kick_index as u8 });
                return true;
            }
        }
        false
    }

    /// 바닥에 닿은 상태에서 조작이 성공하면 고정 대기 시간을 초기화한다
    fn on_moved(&mut self) {
        if self.lock_counter > 0 && self.lock_resets < MAX_LOCK_RESETS {
            self.lock_counter = 0;
            self.lock_resets += 1;
        }
    }

    pub fn ghost(&self) -> Option<Active> {
        let mut a = self.active?;
        while self.fits(&Active { y: a.y + 1, ..a }) {
            a.y += 1;
        }
        Some(a)
    }

    pub fn apply_input(&mut self, input: Input) -> Events {
        if self.dead || self.active.is_none() {
            return Events::default();
        }
        let mut ev = Events::default();
        let changed = match input {
            Input::Left => {
                let ok = self.try_move(-1, 0);
                if ok {
                    self.on_moved();
                }
                ok
            }
            Input::Right => {
                let ok = self.try_move(1, 0);
                if ok {
                    self.on_moved();
                }
                ok
            }
            Input::RotateCw | Input::RotateCcw | Input::Rotate180 => {
                let dir = match input { Input::RotateCw => 1, Input::RotateCcw => -1, _ => 2 };
                let ok = self.try_rotate(dir);
                if ok {
                    self.on_moved();
                }
                ok
            }
            Input::SoftDrop => {
                let ok = self.try_move(0, 1);
                if ok {
                    self.stats.score += 1;
                    self.gravity_counter = 0;
                }
                ok
            }
            Input::HardDrop => {
                let mut d = 0;
                while self.try_move(0, 1) {
                    d += 1;
                }
                self.stats.score += 2 * d;
                ev = self.lock();
                true
            }
            Input::Hold => {
                if self.hold_used {
                    false
                } else {
                    let cur = self.active.unwrap().kind;
                    self.hold_used = true;
                    match self.hold.replace(cur) {
                        Some(h) => self.spawn(h),
                        None => self.spawn_next(),
                    }
                    ev.died = self.dead;
                    true
                }
            }
        };
        if changed {
            self.rev = self.rev.wrapping_add(1);
        }
        ev
    }

    /// 고정 tick 1회 진행: 시간, 중력, 고정 처리
    pub fn tick(&mut self) -> Events {
        if self.dead {
            return Events::default();
        }
        self.elapsed += 1;
        let by_lines = (self.stats.lines / 10).min(MAX_LEVEL as u32) as u8;
        let lvl = match self.level_rule {
            LevelRule::TimeOrLines => ((self.elapsed / LEVEL_UP_TICKS).min(MAX_LEVEL as u32) as u8).max(by_lines),
            LevelRule::Lines => by_lines,
            LevelRule::Fixed => self.level,
        }.min(MAX_LEVEL);
        if lvl != self.level {
            self.level = lvl;
            self.rev = self.rev.wrapping_add(1);
        }
        let mut ev = Events::default();
        if self.active.is_none() {
            return ev;
        }
        if self.grounded() {
            self.lock_counter += 1;
            if self.lock_counter >= LOCK_DELAY {
                ev.merge(self.lock());
                self.rev = self.rev.wrapping_add(1);
            }
        } else {
            self.lock_counter = 0;
            self.gravity_counter += 1;
            if self.gravity_counter >= GRAVITY[self.level as usize] {
                self.gravity_counter = 0;
                self.try_move(0, 1);
                self.rev = self.rev.wrapping_add(1);
            }
        }
        ev
    }

    fn lock(&mut self) -> Events {
        let mut ev = Events { locked: true, ..Default::default() };
        let Some(a) = self.active.take() else { return ev };
        let mut spin = self.detect_spin(a);
        let mut above_visible = true;
        for (x, y) in a.cells() {
            self.board[y as usize * WIDTH + x as usize] = a.kind.code();
            if y as usize >= HIDDEN {
                above_visible = false;
            }
        }
        self.stats.pieces += 1;
        self.hold_used = false;

        let cleared = self.clear_lines();
        if spin == SpinKind::Mini && cleared >= 2 { spin = SpinKind::Full; }
        ev.cleared = cleared;
        ev.spin = spin;
        let difficult = cleared > 0 && (cleared == 4 || spin != SpinKind::None);
        ev.back_to_back = difficult && self.combat.back_to_back > 0;
        if difficult { self.combat.back_to_back = self.combat.back_to_back.saturating_add(1); }
        else if cleared > 0 { self.combat.back_to_back = 0; }
        if cleared > 0 {
            self.combo = self.combo.saturating_add(1);
            self.stats.max_combo = self.stats.max_combo.max(self.combo as u8);
            self.stats.lines += cleared as u32;
        } else { self.combo = -1; }
        ev.combo = self.combo;
        ev.b2b = self.combat.back_to_back;
        let mut base_score = match spin {
            SpinKind::None => [0u32,100,300,500,800][cleared.min(4) as usize],
            SpinKind::Mini => [100u32,200,400][cleared.min(2) as usize],
            SpinKind::Full => [400u32,800,1200,1600][cleared.min(3) as usize],
        };
        if ev.back_to_back { base_score = base_score * 3 / 2; }
        self.stats.score += base_score * (self.level as u32 + 1) + if cleared > 0 { 50 * self.combo.max(0) as u32 } else { 0 };
        if cleared > 0 {
            let base = match spin {
                SpinKind::None => [0u8,0,1,2,4][cleared.min(4) as usize],
                SpinKind::Mini => [0u8,0,1][cleared.min(2) as usize],
                SpinKind::Full => [0u8,2,4,6][cleared.min(3) as usize],
            };
            ev.perfect_clear = self.board.iter().all(|&c| c == CELL_EMPTY);
            let mut attack = base + combo_attack(self.combo) + u8::from(ev.back_to_back) + if ev.perfect_clear { 4 } else { 0 };
            ev.generated = attack;
            while attack > 0 {
                let Some(front) = self.pending_garbage.front_mut() else { break };
                let n = attack.min(*front);
                *front -= n;
                attack -= n;
                ev.cancelled += n;
                if *front == 0 { self.pop_pending_front(); }
            }
            self.stats.cancelled += ev.cancelled as u32;
            self.stats.sent += attack as u32;
            ev.attack = attack;
        } else {
            ev.garbage_applied = self.apply_garbage();
            if self.dead { ev.died = true; return ev; }
        }

        // 블록 전체가 숨김 영역에 고정되면 패배
        if above_visible && cleared == 0 {
            self.dead = true;
            ev.died = true;
            return ev;
        }
        self.spawn_next();
        ev.died = self.dead;
        ev
    }

    fn detect_spin(&self, a: Active) -> SpinKind {
        let Some(rotation) = self.combat.last_rotation else { return SpinKind::None };
        if a.kind != Kind::T { return SpinKind::None; }
        let blocked = |dx, dy| {
            let (x, y) = (a.x + dx, a.y + dy);
            x < 0 || x >= WIDTH as i8 || y < 0 || y >= HEIGHT as i8 || self.cell(x as usize, y as usize) != CELL_EMPTY
        };
        let corners = [blocked(0,0), blocked(2,0), blocked(2,2), blocked(0,2)];
        if corners.iter().filter(|&&b| b).count() < 3 { return SpinKind::None; }
        let front = match a.rot & 3 { 0 => (0,1), 1 => (1,2), 2 => (2,3), _ => (3,0) };
        if (corners[front.0] && corners[front.1]) || rotation.kick_index == 4 { SpinKind::Full } else { SpinKind::Mini }
    }

    fn clear_lines(&mut self) -> u8 {
        let mut cleared = 0u8;
        let mut write = HEIGHT;
        for read in (0..HEIGHT).rev() {
            let row = &self.board[read * WIDTH..(read + 1) * WIDTH];
            if row.iter().all(|&c| c != CELL_EMPTY) {
                cleared += 1;
                continue;
            }
            write -= 1;
            if write != read {
                self.board.copy_within(read * WIDTH..(read + 1) * WIDTH, write * WIDTH);
            }
        }
        for c in &mut self.board[..write * WIDTH] {
            *c = CELL_EMPTY;
        }
        cleared
    }

    /// 대기열 맨 앞 묶음에 도착 시각이 없거나(레거시 상태) 도착 시각이 지났는지
    fn front_garbage_ready(&self) -> bool {
        let untimed = self.pending_garbage.len().saturating_sub(self.combat.garbage_ready_at.len());
        untimed > 0 || self.combat.garbage_ready_at.front().is_some_and(|&t| t <= self.elapsed)
    }

    fn pop_pending_front(&mut self) {
        if self.pending_garbage.pop_front().is_some()
            && self.pending_garbage.len() < self.combat.garbage_ready_at.len()
        {
            self.combat.garbage_ready_at.pop_front();
        }
    }

    fn apply_garbage(&mut self) -> u8 {
        let mut budget = MAX_GARBAGE_PER_LOCK;
        let mut applied = 0u8;
        while budget > 0 && self.front_garbage_ready() {
            let Some(front) = self.pending_garbage.front_mut() else { break };
            let n = (*front).min(budget);
            *front -= n;
            budget -= n;
            if *front == 0 {
                self.pop_pending_front();
            }
            let hole = self.garbage_rng.below(WIDTH as u32) as usize;
            let n = n as usize;
            // 맨 위 n줄에 블록이 있으면 밀려나 패배
            if self.board[..n * WIDTH].iter().any(|&c| c != CELL_EMPTY) {
                self.dead = true;
            }
            self.board.copy_within(n * WIDTH.., 0);
            let start = (HEIGHT - n) * WIDTH;
            for r in 0..n {
                for x in 0..WIDTH {
                    self.board[start + r * WIDTH + x] = if x == hole { CELL_EMPTY } else { CELL_GARBAGE };
                }
            }
            applied += n as u8;
        }
        self.stats.received += applied as u32;
        applied
    }

    /// 퍼즐 준비용: 현재 블록을 `kind` 로 바꿔 생성 위치에 다시 놓는다.
    pub fn set_active_kind(&mut self, kind: Kind) {
        self.spawn(kind);
        self.rev = self.rev.wrapping_add(1);
    }

    /// Zen 모드용: 쌓인 블록과 대기 공격을 비우고 다음 블록으로 이어서 한다.
    /// 점수·줄 수 같은 기록은 유지한다.
    pub fn clear_board_and_continue(&mut self) {
        self.board.fill(CELL_EMPTY);
        self.pending_garbage.clear();
        self.combat.garbage_ready_at.clear();
        self.combo = -1;
        self.dead = false;
        self.hold_used = false;
        self.spawn_next();
        self.rev = self.rev.wrapping_add(1);
    }

    /// 상대가 보낸 공격을 대기열에 추가한다 (다음 고정 시 반영)
    pub fn receive_garbage(&mut self, lines: u8) {
        if lines == 0 || self.dead {
            return;
        }
        // 레거시 상태에서 넘어와 도착 시각이 빠진 앞쪽 원소가 있어도 짝이 어긋나지 않게 뒤에 붙인다.
        self.pending_garbage.push_back(lines);
        self.combat.garbage_ready_at.push_back(self.elapsed + GARBAGE_DELAY_TICKS);
        self.rev = self.rev.wrapping_add(1);
    }

    /// 대기 중인 공격을 (줄 수, 도착 tick) 으로 돌려준다. 도착 시각이 없는 원소는 0 이다.
    pub fn pending_schedule(&self) -> Vec<(u8, u32)> {
        let untimed = self.pending_garbage.len().saturating_sub(self.combat.garbage_ready_at.len());
        self.pending_garbage.iter().enumerate().map(|(i, &n)| {
            (n, if i < untimed { 0 } else { self.combat.garbage_ready_at[i - untimed] })
        }).collect()
    }

    pub fn pending_total(&self) -> u32 {
        self.pending_garbage.iter().map(|&n| n as u32).sum()
    }

    /// 화면용 보이는 영역 셀 (블록/고스트 포함). `out` 길이는 VISIBLE_CELLS.
    pub fn render_visible(&self, ghost: bool, out: &mut [u8]) {
        out.copy_from_slice(&self.board[HIDDEN * WIDTH..]);
        if let Some(a) = self.active {
            if ghost {
                if let Some(g) = self.ghost() {
                    for (x, y) in g.cells() {
                        if y as usize >= HIDDEN {
                            out[(y as usize - HIDDEN) * WIDTH + x as usize] = CELL_GHOST;
                        }
                    }
                }
            }
            for (x, y) in a.cells() {
                if y as usize >= HIDDEN {
                    out[(y as usize - HIDDEN) * WIDTH + x as usize] = a.kind.code();
                }
            }
        }
    }

    /// 현재 블록의 가장 왼쪽 위 셀 위치 (보이는 좌표계, 열/행 0-기반)
    pub fn active_anchor(&self) -> Option<(u8, u8)> {
        let a = self.active?;
        let mut best: Option<(i8, i8)> = None;
        for (x, y) in a.cells() {
            best = Some(match best {
                None => (x, y),
                Some((bx, by)) => (bx.min(x), by.min(y)),
            });
        }
        best.map(|(x, y)| (x as u8, (y - HIDDEN as i8).max(0) as u8))
    }

    /// 쌓인 높이 (보이는 영역 기준)
    pub fn stack_height(&self) -> u8 {
        for y in 0..HEIGHT {
            if self.board[y * WIDTH..(y + 1) * WIDTH].iter().any(|&c| c != CELL_EMPTY) {
                return (HEIGHT - y) as u8;
            }
        }
        0
    }
}

/// 줄 삭제 수와 연속 삭제(콤보)에 따른 공격량
pub fn attack_for(cleared: u8, combo: i8) -> u8 {
    let base = [0u8, 0, 1, 2, 4][cleared.min(4) as usize];
    base + combo_attack(combo)
}

fn combo_attack(combo: i8) -> u8 {
    const COMBO: [u8; 12] = [0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 4, 5];
    COMBO[(combo.max(0) as usize).min(COMBO.len() - 1)]
}

/// 보이는 셀 200개를 4비트씩 묶어 100바이트로 압축 (상대 게임판 전송용)
pub fn pack_cells(cells: &[u8]) -> Vec<u8> {
    cells.chunks(2).map(|p| (p[0] & 0x0F) | (p.get(1).copied().unwrap_or(0) << 4)).collect()
}

pub fn unpack_cells(packed: &[u8], out: &mut [u8]) {
    for (i, b) in packed.iter().enumerate() {
        if 2 * i < out.len() {
            out[2 * i] = b & 0x0F;
        }
        if 2 * i + 1 < out.len() {
            out[2 * i + 1] = b >> 4;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fill_row(g: &mut Game, y: usize, hole: Option<usize>) {
        for x in 0..WIDTH {
            g.board[y * WIDTH + x] = if Some(x) == hole { CELL_EMPTY } else { CELL_GARBAGE };
        }
    }

    fn spin_fixture(rot: u8, full: bool) -> Game {
        let mut g = Game::new(1);
        g.board.fill(0);
        let a = Active { kind: Kind::T, rot, x: 3, y: 10 };
        let front = match rot { 0 => [0,1], 1 => [1,2], 2 => [2,3], _ => [3,0] };
        // Full fills both front corners and one back; Mini both back and one front.
        let missing = if full { (front[1] + 1) % 4 } else { front[1] };
        for (index, (dx,dy)) in [(0,0),(2,0),(2,2),(0,2)].into_iter().enumerate() {
            if index != missing { g.board[(a.y + dy) as usize * WIDTH + (a.x + dx) as usize] = CELL_GARBAGE; }
        }
        g.active = Some(a);
        g.combat.last_rotation = Some(RotationInfo { kick_index: 0 });
        assert!(g.fits(&a));
        g
    }

    fn prepare_clear(g: &mut Game, lines: usize, perfect: bool) {
        g.board.fill(0);
        for y in HEIGHT-lines..HEIGHT { fill_row(g, y, Some(0)); }
        if !perfect { g.board[(HEIGHT-lines-1)*WIDTH+8] = CELL_GARBAGE; }
        g.active = Some(Active { kind: Kind::I, rot: 1, x: -2, y: 0 });
        g.combat.last_rotation = None;
    }

    #[test]
    fn spin_front_corners_classify_all_orientations_and_require_rotation() {
        for rot in 0..4 {
            let mut full = spin_fixture(rot, true);
            assert_eq!(full.lock().spin, SpinKind::Full, "rotation {rot}");
            let mut mini = spin_fixture(rot, false);
            let ev = mini.lock();
            assert_eq!(ev.spin, SpinKind::Mini, "rotation {rot}");
            assert_eq!((ev.generated, ev.combo, ev.b2b), (0,-1,0));
            assert_eq!(mini.stats.score, 100);
            let mut unrotated = spin_fixture(rot, true);
            unrotated.combat.last_rotation = None;
            assert_eq!(unrotated.lock().spin, SpinKind::None);
            let mut two_corners = spin_fixture(rot, true);
            let index = two_corners.board.iter().position(|&c| c != 0).unwrap();
            two_corners.board[index] = 0;
            assert_eq!(two_corners.lock().spin, SpinKind::None);
            let mut fifth = spin_fixture(rot, false);
            fifth.combat.last_rotation = Some(RotationInfo { kick_index: 4 });
            assert_eq!(fifth.lock().spin, SpinKind::Full);
        }
    }

    #[test]
    fn rotation_directions_and_successful_movement_metadata() {
        for dir in [-1,1] {
            for from in 0..4 {
                let mut g = Game::new(1);
                g.active = Some(Active { kind: Kind::T, rot: from, x: 3, y: 5 });
                assert!(g.try_rotate(dir));
                assert_eq!(g.active.unwrap().rot, (from as i8+dir).rem_euclid(4) as u8);
                assert_eq!(g.combat.last_rotation, Some(RotationInfo { kick_index: 0 }));
                assert!(g.try_move(1,0));
                assert_eq!(g.combat.last_rotation, None);
                g.try_rotate(-dir);
                assert!(g.try_move(0,1));
                assert_eq!(g.combat.last_rotation, None);
            }
        }
        let mut g = Game::new(1);
        g.active = Some(Active { kind: Kind::O, rot: 0, x: 3, y: 5 });
        assert!(!g.try_rotate(1));
        assert_eq!(g.active.unwrap().rot, 0);
    }

    #[test]
    fn rotate_180_matches_two_clockwise_turns_in_open_space() {
        for kind in crate::piece::ALL_KINDS {
            for from in 0..4 {
                let mut a = Game::new(1);
                a.active = Some(Active { kind, rot: from, x: 3, y: 8 });
                let mut b = a.clone();
                let ev = a.apply_input(Input::Rotate180);
                assert!(!ev.locked);
                b.try_rotate(1);
                b.try_rotate(1);
                let (mut ca, mut cb): (Vec<_>, Vec<_>) = (a.active.unwrap().cells().collect(), b.active.unwrap().cells().collect());
                ca.sort();
                cb.sort();
                assert_eq!(ca, cb, "{kind:?} from {from}");
                if kind != Kind::O {
                    assert_eq!(a.combat.last_rotation, Some(RotationInfo { kick_index: 0 }));
                }
            }
        }
    }

    #[test]
    fn rotate_180_kicks_up_from_floor_and_resets_lock_delay() {
        let mut g = Game::new(1);
        // T 가 평평한 면을 아래로 하고 바닥에 붙어 있다. 180도 돌리면 뾰족한 칸이 바닥 밖으로 나가므로 한 칸 위로 보정한다.
        g.active = Some(Active { kind: Kind::T, rot: 0, x: 3, y: (HEIGHT - 2) as i8 });
        g.lock_counter = 10;
        let rev = g.rev;
        g.apply_input(Input::Rotate180);
        let a = g.active.unwrap();
        assert_eq!((a.rot, a.y), (2, (HEIGHT - 3) as i8));
        assert_eq!(g.combat.last_rotation, Some(RotationInfo { kick_index: 1 }));
        assert_eq!(g.lock_counter, 0, "successful 180 rotation on the ground resets lock delay");
        assert_ne!(g.rev, rev);
    }

    #[test]
    fn failed_inputs_preserve_rotation_zero_drop_scores_spin_positive_drop_does_not() {
        let mut g = Game::new(1);
        g.active = Some(Active { kind: Kind::T, rot: 0, x: 3, y: HEIGHT as i8-2 });
        g.board[(HEIGHT-2)*WIDTH+3] = CELL_GARBAGE;
        g.combat.last_rotation = Some(RotationInfo { kick_index: 0 });
        assert!(g.grounded());
        assert_eq!(g.apply_input(Input::SoftDrop), Events::default());
        assert!(g.combat.last_rotation.is_some());
        let ev = g.apply_input(Input::HardDrop);
        assert_eq!(ev.spin, SpinKind::Mini, "floor corners count as occupied");
        assert_eq!(g.combat.last_rotation, None, "spawn resets rotation");
        let mut positive = Game::new(1);
        positive.active = Some(Active { kind: Kind::T, rot: 0, x: 3, y: 5 });
        positive.combat.last_rotation = Some(RotationInfo { kick_index: 4 });
        assert_eq!(positive.apply_input(Input::HardDrop).spin, SpinKind::None);
        let mut wall = Game::new(1);
        wall.active = Some(Active { kind: Kind::T, rot: 0, x: 0, y: 5 });
        wall.combat.last_rotation = Some(RotationInfo { kick_index: 0 });
        wall.apply_input(Input::Left);
        assert!(wall.combat.last_rotation.is_some());
        wall.apply_input(Input::Hold);
        assert!(wall.combat.last_rotation.is_none());
        wall.active = Some(Active { kind:Kind::T,rot:0,x:3,y:5 });
        assert!(wall.try_rotate(1));
        assert!(wall.combat.last_rotation.is_some());
        for _ in 0..GRAVITY[0] { wall.tick(); }
        assert!(wall.combat.last_rotation.is_none(), "successful gravity move clears rotation");
    }

    #[test]
    fn b2b_preserves_no_clear_breaks_normal_clear_and_bonuses_cancel_before_sending() {
        let mut g = Game::new(1);
        prepare_clear(&mut g,4,false);
        let first = g.apply_input(Input::HardDrop);
        assert_eq!((first.generated, first.b2b, first.back_to_back, first.combo), (4,1,false,0));
        g.board.fill(0);
        let no_clear = g.apply_input(Input::HardDrop);
        assert_eq!((no_clear.b2b,no_clear.combo), (1,-1));
        prepare_clear(&mut g,4,false);
        g.receive_garbage(3);
        let second = g.apply_input(Input::HardDrop);
        assert_eq!((second.generated,second.cancelled,second.attack,second.b2b), (5,3,2,2));
        assert!(second.back_to_back);
        prepare_clear(&mut g,1,false);
        let normal = g.apply_input(Input::HardDrop);
        assert_eq!((normal.b2b,normal.back_to_back), (0,false));
        prepare_clear(&mut g,4,false);
        let restarted = g.apply_input(Input::HardDrop);
        assert_eq!((restarted.b2b,restarted.back_to_back), (1,false));
        assert_eq!(Game::new(1).combat, CombatState::default());
    }

    #[test]
    fn perfect_clear_combo_and_b2b_score_stack_without_changing_combo_table() {
        let mut g = Game::new(1);
        prepare_clear(&mut g,4,true);
        assert_eq!(g.apply_input(Input::HardDrop).generated,8);
        prepare_clear(&mut g,4,true);
        let before = g.stats.score;
        // Start already grounded so drop score is zero.
        g.active.as_mut().unwrap().y = HEIGHT as i8-4;
        let ev = g.apply_input(Input::HardDrop);
        assert_eq!((ev.generated,ev.combo,ev.b2b), (9,1,2));
        assert!(ev.perfect_clear && ev.back_to_back);
        assert_eq!(g.stats.score-before,1250);
        assert_eq!(attack_for(4,2),5);
        let mut combined = Events { locked:true,cleared:2,combo:9,b2b:5,..Default::default() };
        combined.merge(Events::default());
        assert_eq!((combined.cleared,combined.combo,combined.b2b),(2,9,5));
        combined.merge(ev);
        assert_eq!((combined.cleared,combined.combo,combined.b2b),(4,1,2));
    }

    #[test]
    fn actual_fifth_kicks_upgrade_mini_for_both_rotation_directions() {
        for dir in [-1,1] {
            let mut g = Game::new(1);
            g.board.fill(0);
            g.active = Some(Active { kind:Kind::T,rot:0,x:3,y:10 });
            for (x,y) in [(4,12),(3,10),(3,9),(2,12),(2,14),(3,15)] {
                let x = if dir == 1 { x } else { 8-x };
                g.board[y*WIDTH+x] = CELL_GARBAGE;
            }
            assert!(g.fits(&g.active.unwrap()));
            assert!(g.try_rotate(dir));
            assert_eq!(g.combat.last_rotation,Some(RotationInfo { kick_index:4 }));
            assert!(g.grounded());
            let ev = g.apply_input(Input::HardDrop);
            assert_eq!(ev.spin,SpinKind::Full);
            assert_eq!(g.stats.score,400);
        }
    }

    #[test]
    fn i_wall_and_floor_kicks_are_transition_specific_in_both_directions() {
        for dir in [-1,1] {
            let mut wall = Game::new(1);
            wall.active = Some(Active { kind:Kind::I,rot:1,x:-2,y:5 });
            assert!(wall.try_rotate(dir));
            assert_eq!(wall.active.unwrap().x,0);
            assert_eq!(wall.combat.last_rotation.unwrap().kick_index,if dir == 1 { 2 } else { 1 });
            let mut floor = Game::new(1);
            floor.active = Some(Active { kind:Kind::I,rot:0,x:3,y:HEIGHT as i8-2 });
            assert!(floor.try_rotate(dir));
            assert_eq!(floor.active.unwrap().y,HEIGHT as i8-4);
            assert_eq!(floor.combat.last_rotation.unwrap().kick_index,if dir == 1 { 4 } else { 3 });
        }
    }

    #[test]
    fn mini_double_upgrades_to_full_before_attack_and_score() {
        let mut g = Game::new(1);
        g.board.fill(0);
        g.active = Some(Active { kind:Kind::T,rot:1,x:3,y:HEIGHT as i8-3 });
        for y in HEIGHT-2..HEIGHT { fill_row(&mut g,y,None); }
        for (x,y) in g.active.unwrap().cells() { g.board[y as usize*WIDTH+x as usize] = 0; }
        g.board[(HEIGHT-3)*WIDTH+3] = CELL_GARBAGE;
        g.combat.last_rotation = Some(RotationInfo { kick_index:0 });
        assert_eq!(g.detect_spin(g.active.unwrap()),SpinKind::Mini);
        let ev = g.apply_input(Input::HardDrop);
        assert_eq!((ev.spin,ev.cleared,ev.generated,ev.b2b),(SpinKind::Full,2,4,1));
        assert_eq!(g.stats.score,1200);
    }

    #[test]
    fn failed_rotation_keeps_last_success_and_successful_soft_drop_clears_it() {
        let mut g = Game::new(1);
        g.active = Some(Active { kind:Kind::T,rot:0,x:3,y:10 });
        for y in 7..16 { for x in 0..WIDTH { g.board[y*WIDTH+x] = CELL_GARBAGE; } }
        for (x,y) in g.active.unwrap().cells() { g.board[y as usize*WIDTH+x as usize] = 0; }
        g.combat.last_rotation = Some(RotationInfo { kick_index:2 });
        assert!(!g.try_rotate(1));
        assert!(!g.try_rotate(-1));
        assert_eq!(g.combat.last_rotation,Some(RotationInfo { kick_index:2 }));
        g.board.fill(0);
        g.apply_input(Input::SoftDrop);
        assert_eq!(g.combat.last_rotation,None);
    }

    #[test]
    fn full_single_mini_single_triple_and_zero_line_spin_attack_tables() {
        for (mini, expected_spin, expected_attack, expected_score) in [
            (false, SpinKind::Full, 2, 800), (true, SpinKind::Mini, 0, 200),
        ] {
            let mut g = Game::new(1);
            g.board.fill(0);
            let a = Active { kind:Kind::T,rot:0,x:3,y:HEIGHT as i8-3 };
            fill_row(&mut g,HEIGHT-2,None);
            for (x,y) in a.cells() { g.board[y as usize*WIDTH+x as usize] = 0; }
            for (x,y) in if mini { [(3,HEIGHT-3),(3,HEIGHT-1),(5,HEIGHT-1)] } else { [(3,HEIGHT-3),(5,HEIGHT-3),(3,HEIGHT-1)] } {
                g.board[y*WIDTH+x] = CELL_GARBAGE;
            }
            g.active = Some(a);
            g.combat.last_rotation = Some(RotationInfo { kick_index:0 });
            let mut leveled = g.clone();
            leveled.level = 2;
            leveled.combat.back_to_back = 1;
            let bonus = leveled.apply_input(Input::HardDrop);
            assert_eq!(bonus.generated,expected_attack+1);
            assert_eq!(leveled.stats.score,expected_score*3/2*3);
            let ev = g.apply_input(Input::HardDrop);
            assert_eq!((ev.spin,ev.cleared,ev.generated),(expected_spin,1,expected_attack));
            assert_eq!(g.stats.score,expected_score);
            assert_eq!(ev.b2b,1, "even Mini single with zero base attack is difficult");
        }
        let mut triple = Game::new(1);
        triple.board.fill(0);
        let a = Active { kind:Kind::T,rot:1,x:3,y:HEIGHT as i8-3 };
        for y in HEIGHT-3..HEIGHT { fill_row(&mut triple,y,None); }
        for (x,y) in a.cells() { triple.board[y as usize*WIDTH+x as usize] = 0; }
        triple.board[(HEIGHT-4)*WIDTH] = CELL_GARBAGE;
        triple.active = Some(a);
        triple.combat.last_rotation = Some(RotationInfo { kick_index:0 });
        let ev = triple.apply_input(Input::HardDrop);
        assert_eq!((ev.spin,ev.cleared,ev.generated),(SpinKind::Full,3,6));
        assert_eq!(triple.stats.score,1600);
        let mut zero = spin_fixture(0,true);
        zero.combat.back_to_back = 7;
        zero.combo = 4;
        let ev = zero.lock();
        assert_eq!((ev.b2b,ev.combo,ev.generated,ev.back_to_back),(7,-1,0,false));
        assert_eq!(zero.stats.score,400);
    }

    #[cfg(feature="serde")]
    #[test]
    fn combat_sidecar_does_not_change_legacy_game_bytes() {
        let g = Game::new(19);
        let legacy = postcard::to_allocvec(&g).unwrap();
        let mut changed = g.clone();
        changed.combat = CombatState { last_rotation:Some(RotationInfo { kick_index:4 }), back_to_back:77, garbage_ready_at: VecDeque::from([9]) };
        assert_eq!(legacy,postcard::to_allocvec(&changed).unwrap());
        let decoded: Game = postcard::from_bytes(&legacy).unwrap();
        assert_eq!(decoded.combat,CombatState::default());
        assert_eq!(decoded,g);
    }

    #[test]
    fn same_seed_same_sequence() {
        let mut a = Game::new(1234);
        let mut b = Game::new(1234);
        for i in 0..200 {
            let inp = [Input::Left, Input::RotateCw, Input::HardDrop, Input::Right, Input::Hold][i % 5];
            assert_eq!(a.apply_input(inp), b.apply_input(inp));
            a.tick();
            b.tick();
        }
        assert_eq!(a, b);
    }

    #[test]
    fn bag_contains_each_kind_once() {
        let mut g = Game::new(99);
        let mut seen = Vec::new();
        // 첫 블록 + 대기열에서 7개 수집
        seen.push(g.active.unwrap().kind);
        while seen.len() < 7 {
            g.board.iter_mut().for_each(|c| *c = 0);
            g.apply_input(Input::HardDrop);
            seen.push(g.active.unwrap().kind);
        }
        seen.sort_by_key(|k| k.code());
        seen.dedup();
        assert_eq!(seen.len(), 7);
    }

    #[test]
    fn walls_block_movement() {
        let mut g = Game::new(5);
        for _ in 0..20 {
            g.apply_input(Input::Left);
        }
        let min_x = g.active.unwrap().cells().map(|c| c.0).min().unwrap();
        assert_eq!(min_x, 0);
        for _ in 0..20 {
            g.apply_input(Input::Right);
        }
        let max_x = g.active.unwrap().cells().map(|c| c.0).max().unwrap();
        assert_eq!(max_x, WIDTH as i8 - 1);
    }

    #[test]
    fn hard_drop_locks_and_spawns() {
        let mut g = Game::new(1);
        let ev = g.apply_input(Input::HardDrop);
        assert!(ev.locked);
        assert!(g.active.is_some());
        assert_eq!(g.stats.pieces, 1);
        assert!(g.board[(HEIGHT - 1) * WIDTH..].iter().any(|&c| c != 0));
    }

    #[test]
    fn line_clear_and_attack() {
        let mut g = Game::new(3);
        // 맨 아래 4줄을 0번 열만 비워 둔다
        for y in HEIGHT - 4..HEIGHT {
            fill_row(&mut g, y, Some(0));
        }
        g.active = Some(Active { kind: Kind::I, rot: 1, x: -2, y: 0 });
        assert!(g.fits(&g.active.unwrap()));
        let ev = g.apply_input(Input::HardDrop);
        assert_eq!(ev.cleared, 4);
        assert_eq!(ev.attack, 4 + 4); // 4줄 + 전체 비우기
        assert_eq!(g.stats.lines, 4);
    }

    #[test]
    fn attack_cancels_pending_garbage() {
        let mut g = Game::new(3);
        for y in HEIGHT - 2..HEIGHT {
            fill_row(&mut g, y, Some(0));
        }
        g.board[(HEIGHT - 3) * WIDTH + 5] = CELL_GARBAGE; // 전체 비우기 방지
        g.receive_garbage(3);
        g.active = Some(Active { kind: Kind::I, rot: 1, x: -2, y: 0 });
        let ev = g.apply_input(Input::HardDrop);
        assert_eq!(ev.cleared, 2);
        assert_eq!(ev.cancelled, 1);
        assert_eq!(ev.attack, 0);
        assert_eq!(g.pending_total(), 2);
    }

    #[test]
    fn garbage_applied_on_lock_without_clear() {
        let mut g = Game::new(8);
        g.receive_garbage(2);
        g.elapsed += GARBAGE_DELAY_TICKS;
        let ev = g.apply_input(Input::HardDrop);
        assert_eq!(ev.garbage_applied, 2);
        assert_eq!(g.pending_total(), 0);
        let bottom = &g.board[(HEIGHT - 1) * WIDTH..];
        assert_eq!(bottom.iter().filter(|&&c| c == CELL_GARBAGE).count(), WIDTH - 1);
    }

    #[test]
    fn garbage_waits_for_delay_and_can_be_cancelled_meanwhile() {
        let mut g = Game::new(8);
        g.receive_garbage(2);
        assert_eq!(g.pending_schedule(), vec![(2, GARBAGE_DELAY_TICKS)]);
        let ev = g.apply_input(Input::HardDrop);
        assert_eq!(ev.garbage_applied, 0, "garbage must not rise before its delay");
        assert_eq!(g.pending_total(), 2);
        // 기다리는 동안 줄을 지우면 상쇄한다.
        g.receive_garbage(3);
        prepare_clear(&mut g, 2, false);
        let ev = g.apply_input(Input::HardDrop);
        assert_eq!((ev.generated, ev.cancelled), (1, 1));
        assert_eq!(g.pending_schedule(), vec![(1, GARBAGE_DELAY_TICKS), (3, GARBAGE_DELAY_TICKS)]);
        g.elapsed = GARBAGE_DELAY_TICKS;
        let ev = g.apply_input(Input::HardDrop);
        assert_eq!(ev.garbage_applied, 4, "both arrived chunks rise on the next lock");
        assert!(g.pending_schedule().is_empty());
        assert!(g.combat.garbage_ready_at.is_empty());
    }

    #[test]
    fn legacy_pending_without_arrival_time_is_ready_and_stays_paired() {
        let mut g = Game::new(8);
        g.pending_garbage.push_back(2); // 레거시 상태: 도착 시각 없음
        g.receive_garbage(1);
        assert_eq!(g.pending_schedule(), vec![(2, 0), (1, GARBAGE_DELAY_TICKS)]);
        let ev = g.apply_input(Input::HardDrop);
        assert_eq!(ev.garbage_applied, 2);
        assert_eq!(g.pending_schedule(), vec![(1, GARBAGE_DELAY_TICKS)]);
    }

    #[test]
    fn garbage_push_into_hidden_rows_does_not_kill() {
        let mut g = Game::new(8);
        // 보이는 판 맨 윗줄(스폰 위치 밖, 0열)에 블록이 있어도 방해줄 4줄로 바로 지지 않는다.
        g.board[HIDDEN * WIDTH] = CELL_GARBAGE;
        g.pending_garbage.push_back(4);
        let ev = g.apply_input(Input::HardDrop);
        assert_eq!(ev.garbage_applied, 4);
        assert!(!g.dead);
        assert_eq!(g.board[(HIDDEN - 4) * WIDTH], CELL_GARBAGE);
    }

    #[test]
    fn gravity_moves_piece_down() {
        let mut g = Game::new(2);
        let y0 = g.active.unwrap().y;
        for _ in 0..GRAVITY[0] {
            g.tick();
        }
        assert_eq!(g.active.unwrap().y, y0 + 1);
    }

    #[test]
    fn piece_locks_after_delay_on_ground() {
        let mut g = Game::new(2);
        while g.apply_input(Input::SoftDrop) == Events::default() && g.grounded() == false {}
        assert!(g.grounded());
        let pieces = g.stats.pieces;
        for _ in 0..LOCK_DELAY {
            g.tick();
        }
        assert_eq!(g.stats.pieces, pieces + 1);
    }

    #[test]
    fn topping_out_kills() {
        let mut g = Game::new(11);
        for _ in 0..200 {
            if g.dead {
                break;
            }
            g.apply_input(Input::HardDrop);
        }
        assert!(g.dead);
    }

    #[test]
    fn hold_swaps_once_per_piece() {
        let mut g = Game::new(4);
        let first = g.active.unwrap().kind;
        g.apply_input(Input::Hold);
        assert_eq!(g.hold, Some(first));
        let second = g.active.unwrap().kind;
        g.apply_input(Input::Hold);
        assert_eq!(g.active.unwrap().kind, second, "같은 블록에서 두 번 보관할 수 없음");
    }

    #[test]
    fn pack_roundtrip() {
        let cells: Vec<u8> = (0..VISIBLE_CELLS).map(|i| (i % 10) as u8).collect();
        let p = pack_cells(&cells);
        assert_eq!(p.len(), VISIBLE_CELLS / 2);
        let mut out = vec![0u8; VISIBLE_CELLS];
        unpack_cells(&p, &mut out);
        assert_eq!(cells, out);
    }
}
