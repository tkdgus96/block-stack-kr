//! 방(Room) 하나 = Tokio 태스크 하나.
//!
//! 방 태스크가 모든 플레이어의 게임 상태를 소유하고 고정 60Hz tick으로 진행한다.
//! 클라이언트는 입력만 보내고, 브라우저 탭이 비활성화되어 타이머가 느려져도
//! 게임은 서버에서 계속 진행된다.

use bs_engine::bot;
use bs_engine::game::{pack_cells, Game, Input, TICKS_PER_SECOND, VISIBLE_CELLS};
use bs_engine::protocol::{encode, CombatEvent, CombatStatus, BattleStat, ChatLine, ClientMsg, RoomConfig, MemberState, MatchSummary, RoundPlayer, Phase, PlayerInfo, PlayerStat, RoomSummary, ServerMsg, MAX_CHAT_LEN};
use bs_engine::rng::Rng;
use std::collections::{HashMap, VecDeque};
use axum::body::Bytes;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};

pub type Outbox = mpsc::Sender<Bytes>;
pub type Registry = Arc<Mutex<HashMap<String, RoomEntry>>>;

/// 방 레지스트리 항목: 명령 채널 + 방 목록용 요약 (방 태스크가 인원/상태가 바뀔 때 갱신)
pub struct RoomEntry {
    pub tx: mpsc::Sender<RoomCmd>,
    pub summary: RoomSummary,
    pub public: bool,
    pub coop: bool,
    pub config: RoomConfig,
}

const TICK: Duration = Duration::from_nanos(1_000_000_000 / TICKS_PER_SECOND as u64);
/// 상대 게임판 전송 주기 (tick) → 10Hz
const BOARDS_EVERY: u64 = 6;
/// 점수표 전송 주기 (tick) → 2Hz
const SCORES_EVERY: u64 = 30;
#[cfg(test)]
const BATTLE_TICKS: u32 = 120 * TICKS_PER_SECOND;
const COUNTDOWN_TICKS: u32 = 3 * TICKS_PER_SECOND;
/// 한 tick에 처리할 플레이어당 최대 입력 수 (스팸 방지)
const MAX_INPUTS_PER_TICK: usize = 16;
const MAX_QUEUED_INPUTS: usize = 128;
/// 새로 들어온 사람에게 보여줄 최근 채팅 수
const CHAT_HISTORY: usize = 30;
/// 채팅 도배 방지: 3초에 최대 5줄
const CHAT_WINDOW: Duration = Duration::from_secs(3);
const CHAT_MAX_IN_WINDOW: usize = 5;

pub enum RoomCmd {
    Join { name: String, tx: Outbox, reply: oneshot::Sender<Result<u8, String>> },
    Msg { id: u8, msg: ClientMsg },
    Leave { id: u8 },
}

struct BotBrain {
    plan: VecDeque<Input>,
    wait: u32,
    style: bot::Style,
    /// 생각하는 시간 (tick) — 봇마다 다르게
    think: u32,
}

struct Player {
    id: u8,
    name: String,
    tx: Option<Outbox>,
    bot: Option<BotBrain>,
    wins: u32,
    in_game: bool,
    game: Option<Game>,
    inputs: VecDeque<(u32, Input)>,
    ack: u32,
    sent_rev: u32,
    sent_ack: u32,
    board_rev: Option<u32>,
    place: u8,
    target: Option<u8>,
    /// 이번 판에 쓰러뜨린 상대 수 (KO)
    kos: u32,
    /// 마지막으로 나에게 공격을 보낸 플레이어 → 내가 탈락하면 그 사람의 KO
    last_attacker: Option<u8>,
    /// 최근 채팅 시각 (도배 방지)
    chat_times: VecDeque<tokio::time::Instant>,
    rounds: u32,
    total_sent: u64,
    team: u8,
    ready: bool,
    combat_v2: bool,
    profile: Option<String>,
}

impl Player {
    fn alive(&self) -> bool {
        self.in_game && self.game.as_ref().is_some_and(|g| !g.dead)
    }
}

pub struct Room {
    code: String,
    registry: Registry,
    players: Vec<Player>,
    host: Option<u8>,
    phase: Phase,
    countdown: u32,
    elapsed_ticks: u32,
    ticks: u64,
    next_id: u8,
    rng: Rng,
    participants: usize,
    finished: u8,
    last_scores: Vec<PlayerStat>,
    chat: VecDeque<ChatLine>,
    config: RoomConfig,
    match_id: u64,
    last_summary: Option<MatchSummary>,
    departed: Vec<RoundPlayer>,
    profiles: HashMap<u8, String>,
    coop: bs_engine::protocol::CoopState,
}

pub fn spawn_room(code: String, registry: Registry) -> mpsc::Sender<RoomCmd> {
    let (tx, rx) = mpsc::channel(512);
    let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1);
    let room = Room {
        code,
        registry,
        players: Vec::new(),
        host: None,
        phase: Phase::Lobby,
        countdown: 0,
        elapsed_ticks: 0,
        ticks: 0,
        next_id: 1,
        rng: Rng::new(seed),
        participants: 0,
        finished: 0,
        last_scores: Vec::new(),
        chat: VecDeque::new(),
        config: RoomConfig::default(), match_id: seed, last_summary: None, departed: Vec::new(), profiles: HashMap::new(), coop: Default::default(),
    };
    tokio::spawn(room.run(rx));
    tx
}

impl Room {
    async fn run(mut self, mut rx: mpsc::Receiver<RoomCmd>) {
        let mut interval = tokio::time::interval(TICK);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Burst);
        loop {
            let ticking = self.phase != Phase::Lobby;
            tokio::select! {
                cmd = rx.recv() => {
                    let Some(cmd) = cmd else { break };
                    let was_lobby = self.phase == Phase::Lobby;
                    self.handle(cmd);
                    if was_lobby && self.phase != Phase::Lobby {
                        interval.reset();
                    }
                    self.publish_summary();
                    if !self.players.iter().any(|p| p.tx.is_some()) {
                        break;
                    }
                }
                _ = interval.tick(), if ticking => {
                    let before = self.phase;
                    self.step();
                    if self.phase != before {
                        self.publish_summary();
                    }
                }
            }
        }
        // 수신 측을 먼저 닫아야 레지스트리의 Sender가 closed 로 보인다
        drop(rx);
        let mut reg = self.registry.lock().unwrap();
        if reg.get(&self.code).is_some_and(|e| e.tx.is_closed()) {
            reg.remove(&self.code);
        }
    }

    /// 방 목록에 보일 요약을 레지스트리에 기록
    fn publish_summary(&self) {
        let humans = self.players.iter().filter(|p| p.bot.is_none()).count() as u8;
        let summary = RoomSummary {
            code: self.code.clone(),
            host: self.host.map(|h| self.name_of(h)).unwrap_or_default(),
            players: self.players.len() as u8,
            humans,
            bots: self.players.len() as u8 - humans,
            playing: self.phase != Phase::Lobby,
        };
        if let Some(e) = self.registry.lock().unwrap().get_mut(&self.code) {
            e.summary = summary;
            e.public = self.config.public;
            e.coop = self.coop.enabled;
            e.config = self.config.clone();
        }
    }

    fn send(&self, id: u8, msg: &ServerMsg) {
        if let Some(tx) = self.players.iter().find(|p| p.id == id).and_then(|p| p.tx.as_ref()) {
            let _ = tx.try_send(Bytes::from(encode(msg)));
        }
    }

    fn broadcast(&self, msg: &ServerMsg) {
        let bytes = Bytes::from(encode(msg));
        for p in &self.players {
            if let Some(tx) = &p.tx {
                // 느린 클라이언트 때문에 방 전체가 멈추지 않도록 꽉 차면 버린다
                let _ = tx.try_send(bytes.clone());
            }
        }
    }

    fn log(&self, text: String) {
        self.broadcast(&ServerMsg::Log(text));
    }

    fn lobby_msg(&self) -> ServerMsg {
        ServerMsg::Lobby {
            phase: self.phase,
            host: self.host.unwrap_or(0),
            players: self
                .players
                .iter()
                .map(|p| PlayerInfo { id: p.id, name: p.name.clone(), bot: p.bot.is_some(), wins: p.wins, in_game: p.in_game })
                .collect(),
        }
    }

    fn name_of(&self, id: u8) -> String {
        self.players.iter().find(|p| p.id == id).map(|p| p.name.clone()).unwrap_or_else(|| "?".into())
    }

    fn add_player(&mut self, name: String, tx: Option<Outbox>, bot: bool) -> Result<u8, String> {
        if self.players.len() >= self.config.max_players as usize {
            return Err(format!("방이 가득 찼습니다 (최대 {}명)", self.config.max_players));
        }
        let team = if self.players.iter().filter(|p| p.team == 0).count() <= self.players.iter().filter(|p| p.team == 1).count() { 0 } else { 1 };
        let mut id = self.next_id;
        while self.players.iter().any(|p| p.id == id) { id = id.wrapping_add(1).max(1); }
        self.next_id = id.wrapping_add(1).max(1);
        self.players.push(Player {
            id,
            name,
            tx,
            bot: bot.then(|| {
                let n = self.players.iter().filter(|p| p.bot.is_some()).count() as u32;
                BotBrain { plan: VecDeque::new(), wait: 30, style: bot::Style::variant(n), think: Self::bot_think(self.config.bot_level, n) }
            }),
            wins: 0,
            in_game: false,
            game: None,
            inputs: VecDeque::new(),
            ack: 0,
            sent_rev: u32::MAX,
            sent_ack: 0,
            board_rev: None,
            place: 0,
            target: None,
            kos: 0,
            last_attacker: None,
            chat_times: VecDeque::new(),
            rounds: 0,
            total_sent: 0,
            team, ready: false, combat_v2: false, profile: None,
        });
        if self.host.is_none() && !bot {
            self.host = Some(id);
        }
        self.reset_ready();
        Ok(id)
    }

    fn handle(&mut self, cmd: RoomCmd) {
        match cmd {
            RoomCmd::Join { name, tx, reply } => {
                let res = self.add_player(name.clone(), Some(tx), false);
                let ok = res.clone().ok();
                let _ = reply.send(res);
                if let Some(id) = ok {
                    self.send(id, &ServerMsg::Welcome { you: id, room: self.code.clone() });
                    self.send(id, &ServerMsg::CoopState(self.coop.clone()));
                    self.broadcast(&self.lobby_msg());
                    self.broadcast(&self.room_state());
                    if self.phase == Phase::Lobby {
                        if let Some(summary) = &self.last_summary { self.send(id, &ServerMsg::MatchSummary(summary.clone())); }
                    }
                    self.send(id, &self.match_info());
                    self.send(id, &ServerMsg::BattleStats(self.battle_stats()));
                    self.send(id, &ServerMsg::CombatStatus(self.combat_status()));
                    if !self.chat.is_empty() {
                        self.send(id, &ServerMsg::Chat(self.chat.iter().cloned().collect()));
                    }
                    self.log(format!("{name} 님이 입장했습니다"));
                    if self.phase != Phase::Lobby {
                        self.send(id, &ServerMsg::Log("게임 진행 중입니다. 다음 판부터 참가합니다".into()));
                        self.send(id, &ServerMsg::Scores(self.last_scores.clone()));
                    }
                }
            }
            RoomCmd::Leave { id } => self.remove_player(id),
            RoomCmd::Msg { id, msg } => self.on_msg(id, msg),
        }
    }

    fn remove_player(&mut self, id: u8) {
        let Some(idx) = self.players.iter().position(|p| p.id == id) else { return };
        let p = self.players.remove(idx);
        if p.in_game {
            let mut row = Self::round_player(&p);
            if row.place == 0 { row.place = (self.participants as u8).saturating_sub(self.finished); }
            self.departed.push(row);
        }
        self.reset_ready();
        if p.alive() {
            self.finished += 1;
        }
        if self.host == Some(id) {
            self.host = self.players.iter().find(|p| p.bot.is_none()).map(|p| p.id);
        }
        self.log(format!("{} 님이 나갔습니다", p.name));
        self.broadcast(&self.room_state());
        self.broadcast(&self.lobby_msg());
        if self.phase == Phase::Playing {
            self.check_game_over();
        }
    }

    fn on_msg(&mut self, id: u8, msg: ClientMsg) {
        let is_host = self.host == Some(id);
        match msg {
            ClientMsg::Join { .. } => {}
            ClientMsg::Ping { t } => self.send(id, &ServerMsg::Pong { t }),
            ClientMsg::Chat { text } => self.on_chat(id, &text),
            ClientMsg::EnableCombat => {
                if let Some(p) = self.players.iter_mut().find(|p| p.id == id) {
                    p.combat_v2 = true;
                    p.sent_rev = p.game.as_ref().map_or(u32::MAX, |g| g.rev.wrapping_sub(1));
                }
                self.send(id, &ServerMsg::CombatStatus(self.combat_status()));
                self.send_states();
            }
            ClientMsg::AttachProfile { token } => {
                if self.phase == Phase::Lobby && crate::profile::exists(&token) {
                    if let Some(p) = self.players.iter_mut().find(|p| p.id == id) { p.profile = Some(token); }
                }
            }
            ClientMsg::SetCoop { enabled } if is_host && self.phase == Phase::Lobby => {
                self.last_summary = None;
                self.coop = bs_engine::protocol::CoopState { enabled, goal: 80, progress: 0, won: None };
                if enabled { self.config.teams = false; }
                self.reset_ready();
                self.broadcast(&self.room_state());
                self.broadcast(&ServerMsg::CoopState(self.coop.clone()));
            }
            ClientMsg::Start if is_host && self.phase == Phase::Lobby => {
                if let Err(error) = self.can_start(true) { self.send(id, &ServerMsg::Error(error)); } else { self.start(); }
            }
            ClientMsg::SetRoomConfig(config) => self.configure(id, config),
            ClientMsg::SetTeam { team } => self.set_team(id, team),
            ClientMsg::Ready { ready } => self.set_ready(id, ready),
            ClientMsg::AddBot if is_host && self.phase == Phase::Lobby => {
                let n = self.players.iter().filter(|p| p.bot.is_some()).count() + 1;
                if self.add_player(format!("연습봇{n}"), None, true).is_ok() {
                    self.broadcast(&self.lobby_msg());
                    self.broadcast(&self.room_state());
                }
            }
            ClientMsg::RemoveBot if is_host && self.phase == Phase::Lobby => {
                if let Some(idx) = self.players.iter().rposition(|p| p.bot.is_some()) {
                    self.players.remove(idx);
                    self.reset_ready();
                    self.broadcast(&self.room_state());
                    self.broadcast(&self.lobby_msg());
                }
            }
            ClientMsg::Input { seq, input } => {
                if self.phase != Phase::Playing {
                    return;
                }
                if let Some(p) = self.players.iter_mut().find(|p| p.id == id) {
                    if p.alive() && p.inputs.len() < MAX_QUEUED_INPUTS {
                        p.inputs.push_back((seq, input));
                    }
                }
            }
            _ => self.send(id, &ServerMsg::Error("지금은 할 수 없는 동작입니다".into())),
        }
    }

    fn bot_think(level: u8, n: u32) -> u32 {
        match level { 0 => 48 + 12 * (n % 4), 2 => 6 + 2 * (n % 4), _ => 20 + 6 * (n % 4) }
    }

    fn reset_ready(&mut self) {
        for p in &mut self.players { p.ready = p.bot.is_some(); }
    }

    fn room_state(&self) -> ServerMsg {
        ServerMsg::RoomState { config: self.config.clone(), members: self.players.iter().map(|p|
            MemberState { id: p.id, team: p.team, ready: p.ready || p.bot.is_some() }
        ).collect() }
    }

    fn configure(&mut self, id: u8, config: RoomConfig) {
        if self.host != Some(id) || self.phase != Phase::Lobby {
            self.send(id, &ServerMsg::Error("방장만 대기실에서 방 설정을 바꿀 수 있습니다".into()));
            return;
        }
        if !config.valid() || (config.max_players as usize) < self.players.len() {
            self.send(id, &ServerMsg::Error("잘못된 방 설정입니다. 시간 60/120/180초, 정원 2~6명, 팀전 정원 4/6명을 선택하세요".into()));
            return;
        }
        let rebalance = config.teams && (!self.config.teams || config.max_players != self.config.max_players);
        self.config = config;
        if self.config.teams { self.coop.enabled = false; self.broadcast(&ServerMsg::CoopState(self.coop.clone())); }
        for (index, p) in self.players.iter_mut().enumerate() {
            if rebalance { p.team = (index % 2) as u8; }
            if let Some(brain) = &mut p.bot {
                brain.think = Self::bot_think(self.config.bot_level, index as u32);
                brain.plan.clear();
            }
        }
        self.reset_ready();
        self.broadcast(&self.room_state());
    }

    fn set_team(&mut self, id: u8, team: u8) {
        if self.phase != Phase::Lobby || !self.config.teams || team > 1 {
            self.send(id, &ServerMsg::Error("대기실의 팀전에서 A/B 팀을 선택하세요".into()));
            return;
        }
        if let Some(p) = self.players.iter_mut().find(|p| p.id == id && p.bot.is_none()) {
            p.team = team;
            self.reset_ready();
            self.broadcast(&self.room_state());
        }
    }

    fn can_start(&self, host_start: bool) -> Result<(), String> {
        if self.players.is_empty() { return Err("참가자가 없습니다".into()); }
        if self.config.teams {
            let half = self.config.max_players as usize / 2;
            if self.players.len() != self.config.max_players as usize ||
                !(0..=1).all(|team| self.players.iter().filter(|p| p.team == team).count() == half) {
                return Err("팀전은 정원을 채운 2:2 또는 3:3으로 시작할 수 있습니다".into());
            }
        }
        if self.players.iter().any(|p|
            p.bot.is_none() && !p.ready && !(host_start && self.host == Some(p.id))) {
            return Err("다른 참가자가 모두 준비를 마쳐야 합니다".into());
        }
        Ok(())
    }

    fn set_ready(&mut self, id: u8, ready: bool) {
        if self.phase != Phase::Lobby {
            self.send(id, &ServerMsg::Error("대기실에서 재대결을 준비하세요".into()));
            return;
        }
        if let Some(p) = self.players.iter_mut().find(|p| p.id == id && p.bot.is_none()) { p.ready = ready; }
        self.broadcast(&self.room_state());
        if self.last_summary.is_some() && self.players.iter().all(|p| p.bot.is_some() || p.ready) {
            match self.can_start(false) {
                Ok(()) => self.start(),
                Err(error) => self.send(id, &ServerMsg::Error(error)),
            }
        }
    }

    fn attack_targets(&self, from: u8) -> Vec<u8> {
        if self.coop.enabled { return Vec::new(); }
        let team = self.players.iter().find(|p| p.id == from).map(|p| p.team);
        self.players.iter().filter(|p| p.alive() && p.id != from &&
            (!self.config.teams || Some(p.team) != team)).map(|p| p.id).collect()
    }

    fn round_player(p: &Player) -> RoundPlayer {
        let g = p.game.as_ref();
        RoundPlayer { id: p.id, name: p.name.clone(), team: p.team, place: p.place,
            score: g.map_or(0, |g| g.stats.score), sent: g.map_or(0, |g| g.stats.sent),
            active_ticks: g.map_or(0, |g| g.elapsed), kos: p.kos }
    }

    fn round_players(&self) -> Vec<RoundPlayer> {
        self.players.iter().filter(|p| p.in_game).map(Self::round_player).chain(self.departed.iter().cloned()).collect()
    }

    fn limit_ticks(&self) -> u32 { self.config.duration_secs as u32 * TICKS_PER_SECOND }

    fn on_chat(&mut self, id: u8, text: &str) {
        let text: String = text.trim().chars().filter(|c| !c.is_control()).take(MAX_CHAT_LEN).collect();
        if text.is_empty() {
            return;
        }
        let now = tokio::time::Instant::now();
        let Some(p) = self.players.iter_mut().find(|p| p.id == id) else { return };
        while p.chat_times.front().is_some_and(|t| now.duration_since(*t) > CHAT_WINDOW) {
            p.chat_times.pop_front();
        }
        if p.chat_times.len() >= CHAT_MAX_IN_WINDOW {
            self.send(id, &ServerMsg::Error("채팅을 너무 빨리 보내고 있습니다. 잠시 후 다시 보내세요".into()));
            return;
        }
        p.chat_times.push_back(now);
        let line = ChatLine { from: id, name: p.name.clone(), text };
        if self.chat.len() >= CHAT_HISTORY {
            self.chat.pop_front();
        }
        self.chat.push_back(line.clone());
        self.broadcast(&ServerMsg::Chat(vec![line]));
    }

    fn start(&mut self) {
        let seed = self.rng.next_u64();
        self.phase = Phase::Countdown;
        self.countdown = COUNTDOWN_TICKS;
        self.elapsed_ticks = 0;
        self.ticks = 0;
        self.finished = 0;
        self.participants = self.players.len();
        self.match_id = self.match_id.wrapping_add(1);
        self.departed.clear();
        self.profiles = self.players.iter().filter_map(|p| p.profile.clone().map(|token| (p.id,token))).collect();
        self.coop.progress = 0; self.coop.won = None;
        self.broadcast(&ServerMsg::CoopState(self.coop.clone()));
        self.reset_ready();
        for p in &mut self.players {
            p.in_game = true;
            p.game = Some(Game::new(seed));
            p.inputs.clear();
            p.ack = 0;
            p.sent_rev = u32::MAX;
            p.sent_ack = 0;
            p.board_rev = None;
            p.place = 0;
            p.target = None;
            p.kos = 0;
            p.last_attacker = None;
            if let Some(b) = &mut p.bot {
                b.plan.clear();
                b.wait = 30;
            }
        }
        self.broadcast(&self.lobby_msg());
        self.broadcast(&self.room_state());
        self.broadcast(&ServerMsg::Started { seed });
        self.broadcast(&ServerMsg::CombatStatus(self.combat_status()));
        self.broadcast(&ServerMsg::Countdown { seconds: 3 });
        self.broadcast(&self.match_info());
        self.log(format!("게임 시작 ({}명)", self.participants));
        self.send_states();
        self.send_boards(true);
        self.send_scores(true);
    }

    fn step(&mut self) {
        self.ticks += 1;
        match self.phase {
            Phase::Lobby => {}
            Phase::Countdown => {
                self.countdown = self.countdown.saturating_sub(1);
                if self.countdown == 0 {
                    self.phase = Phase::Playing;
                    self.broadcast(&self.lobby_msg());
                    self.broadcast(&ServerMsg::Countdown { seconds: 0 });
                    self.broadcast(&self.match_info());
                } else if self.countdown % TICKS_PER_SECOND == 0 {
                    self.broadcast(&ServerMsg::Countdown { seconds: (self.countdown / TICKS_PER_SECOND) as u8 });
                }
            }
            Phase::Playing => self.simulate(),
        }
    }

    fn simulate(&mut self) {
        self.elapsed_ticks = self.elapsed_ticks.saturating_add(1);
        // 1) 입력 + 중력 처리, 공격 수집
        let mut locks: Vec<CombatEvent> = Vec::new();
        let mut deaths: Vec<u8> = Vec::new();
        for p in self.players.iter_mut() {
            if !p.alive() {
                continue;
            }
            let game = p.game.as_mut().unwrap();
            let mut evs = Vec::new();
            if let Some(brain) = &mut p.bot {
                if brain.wait > 0 {
                    brain.wait -= 1;
                } else if let Some(i) = brain.plan.pop_front() {
                    let event = game.apply_input(i);
                    evs.push((event, game.stats.pieces));
                    brain.wait = match self.config.bot_level { 0 => 10, 2 => 2, _ => 5 };
                } else {
                    brain.plan = bot::plan_with(game, &brain.style).into();
                    brain.wait = brain.think;
                }
            } else {
                for _ in 0..MAX_INPUTS_PER_TICK {
                    let Some((seq, input)) = p.inputs.pop_front() else { break };
                    let event = game.apply_input(input);
                    evs.push((event, game.stats.pieces));
                    p.ack = seq;
                }
            }
            let event = game.tick();
            evs.push((event, game.stats.pieces));
            for (ev, piece) in evs {
                if ev.locked {
                    locks.push(CombatEvent {
                        match_id: self.match_id, player: p.id, piece,
                        cleared: ev.cleared, spin: ev.spin, combo: ev.combo,
                        b2b: ev.b2b, back_to_back: ev.back_to_back,
                        perfect_clear: ev.perfect_clear, generated: ev.generated,
                        cancelled: ev.cancelled, sent: ev.attack, target: None,
                    });
                }
                if ev.died {
                    deaths.push(p.id);
                    break;
                }
            }
        }

        if self.coop.enabled {
            self.coop.progress = self.coop.progress.saturating_add(locks.iter().map(|e| e.sent as u32).sum::<u32>());
            if self.elapsed_ticks % (10 * TICKS_PER_SECOND) == 0 {
                let wave = (2 + self.elapsed_ticks / (30 * TICKS_PER_SECOND)).min(6) as u8;
                for p in self.players.iter_mut().filter(|p| p.alive()) { p.game.as_mut().unwrap().receive_garbage(wave); }
                self.log(format!("공동 생존: 모든 생존자에게 방해줄 {wave}줄!"));
            }
            if !locks.is_empty() || self.elapsed_ticks % TICKS_PER_SECOND == 0 { self.broadcast(&ServerMsg::CoopState(self.coop.clone())); }
        }
        // 2) 공격 분배 (대상이 없거나 탈락했으면 무작위 생존자로 변경)
        for event in &mut locks {
            let (from, lines) = (event.player, event.sent);
            if lines == 0 { continue; }
            let alive = self.attack_targets(from);
            if alive.is_empty() {
                event.sent = 0;
                continue;
            }
            let cur = self.players.iter().find(|q| q.id == from).and_then(|q| q.target);
            let target = match cur {
                Some(t) if alive.contains(&t) => t,
                _ => alive[self.rng.below(alive.len() as u32) as usize],
            };
            event.target = Some(target);
            if let Some(src) = self.players.iter_mut().find(|q| q.id == from) {
                src.target = Some(target);
            }
            if let Some(dst) = self.players.iter_mut().find(|q| q.id == target) {
                dst.game.as_mut().unwrap().receive_garbage(lines);
                dst.last_attacker = Some(from);
            }
            if lines >= 4 {
                self.log(format!("{} → {} 공격 {}줄", self.name_of(from), self.name_of(target), lines));
            }
        }

        // Preserve each lock rather than inferring changes from the final score.
        for event in locks { self.broadcast(&ServerMsg::CombatEvent(event)); }

        // 3) 탈락 처리
        for id in deaths {
            let place = (self.participants as u8).saturating_sub(self.finished);
            self.finished += 1;
            let mut killer = None;
            if let Some(p) = self.players.iter_mut().find(|p| p.id == id) {
                p.place = place;
                killer = p.last_attacker;
            }
            if let Some(k) = killer.and_then(|k| self.players.iter_mut().find(|p| p.id == k)) {
                k.kos += 1;
                let kname = k.name.clone();
                self.log(format!("{kname} → {} K.O.! ({}위)", self.name_of(id), place));
            } else {
                self.log(format!("{} 탈락 ({}위)", self.name_of(id), place));
            }
        }

        // 4) 전송
        self.send_states();
        if self.ticks % BOARDS_EVERY == 0 {
            self.send_boards(false);
        }
        if self.ticks % SCORES_EVERY == 0 {
            self.send_scores(false);
        }
        self.check_game_over();
        if self.phase == Phase::Playing && self.elapsed_ticks % TICKS_PER_SECOND == 0 {
            self.broadcast(&self.match_info());
        }
    }

    /// 사람 플레이어에게 자기 상태를 보낸다 (변경되었을 때만)
    fn send_states(&mut self) {
        for p in self.players.iter_mut() {
            let (Some(tx), Some(game)) = (&p.tx, &p.game) else { continue };
            if !p.in_game || (game.rev == p.sent_rev && p.ack == p.sent_ack) {
                continue;
            }
            p.sent_rev = game.rev;
            p.sent_ack = p.ack;
            let message = if p.combat_v2 {
                ServerMsg::YourStateV2 { ack: p.ack, game: game.clone(), combat: game.combat.clone() }
            } else {
                ServerMsg::YourState { ack: p.ack, game: game.clone() }
            };
            let _ = tx.try_send(Bytes::from(encode(&message)));
        }
    }

    fn send_boards(&mut self, force: bool) {
        let mut boards = Vec::new();
        let mut cells = [0u8; VISIBLE_CELLS];
        for p in self.players.iter_mut() {
            let Some(game) = &p.game else { continue };
            if !p.in_game || (!force && p.board_rev == Some(game.rev)) {
                continue;
            }
            p.board_rev = Some(game.rev);
            game.render_visible(false, &mut cells);
            boards.push((p.id, pack_cells(&cells)));
        }
        if !boards.is_empty() {
            self.broadcast(&ServerMsg::Boards(boards));
        }
    }

    fn stats(&self) -> Vec<PlayerStat> {
        self.players
            .iter()
            .filter(|p| p.in_game)
            .filter_map(|p| {
                let g = p.game.as_ref()?;
                Some(PlayerStat {
                    id: p.id,
                    score: g.stats.score,
                    lines: g.stats.lines,
                    level: g.level,
                    sent: g.stats.sent,
                    received: g.stats.received,
                    cancelled: g.stats.cancelled,
                    pending: g.pending_total(),
                    alive: !g.dead,
                    place: p.place,
                    target: p.target,
                    kos: p.kos,
                })
            })
            .collect()
    }

    fn send_scores(&mut self, force: bool) {
        self.broadcast(&ServerMsg::BattleStats(self.battle_stats()));
        let s = self.stats();
        if force || s != self.last_scores {
            self.broadcast(&ServerMsg::Scores(s.clone()));
            self.last_scores = s;
        }
    }

    fn combat_status(&self) -> Vec<CombatStatus> {
        self.players.iter().map(|p| CombatStatus {
            id: p.id,
            combo: p.game.as_ref().map_or(-1, |g| g.combo),
            b2b: p.game.as_ref().map_or(0, |g| g.combat.back_to_back),
        }).collect()
    }

    fn match_info(&self) -> ServerMsg {
        let timed = self.participants >= 2 || self.coop.enabled;
        let remaining_secs = if timed {
            self.limit_ticks().saturating_sub(self.elapsed_ticks).div_ceil(TICKS_PER_SECOND) as u16
        } else {
            0
        };
        ServerMsg::MatchInfo { remaining_secs, elapsed_ticks: self.elapsed_ticks, timed }
    }

    fn battle_stats(&self) -> Vec<BattleStat> {
        self.players.iter().map(|p| BattleStat {
            id: p.id,
            active_ticks: p.game.as_ref().map_or(0, |g| g.elapsed),
            rounds: p.rounds,
            total_sent: p.total_sent,
        }).collect()
    }

    fn check_game_over(&mut self) {
        if self.phase != Phase::Playing {
            return;
        }
        if self.coop.enabled {
            if self.coop.progress >= self.coop.goal || !self.players.iter().any(|p| p.alive()) || self.elapsed_ticks >= self.limit_ticks() {
                self.finish(self.elapsed_ticks >= self.limit_ticks());
            }
            return;
        }
        if self.participants >= 2 && self.elapsed_ticks >= self.limit_ticks() {
            self.finish(true);
            return;
        }
        let alive = self.players.iter().filter(|p| p.alive()).count();
        let over = if self.config.teams {
            !(0..=1).all(|team| self.players.iter().any(|p| p.alive() && p.team == team))
        } else if self.participants >= 2 { alive <= 1 } else { alive == 0 };
        if over {
            self.finish(false);
        }
    }

    fn finish(&mut self, timed_out: bool) {
        if self.phase != Phase::Playing {
            return;
        }
        let winners = if self.coop.enabled {
            let won = self.coop.progress >= self.coop.goal;
            self.coop.won = Some(won);
            for p in self.players.iter_mut().filter(|p| p.in_game) { p.place = if won { 1 } else { 0 }; }
            self.broadcast(&ServerMsg::CoopState(self.coop.clone()));
            self.log(if won { "협동 성공! 공동 공격 목표 달성".into() } else { "협동 종료 · 다음 도전에서 다시 만나요".into() });
            if won { self.round_players().iter().map(|p| p.id).collect() } else { Vec::new() }
        } else if self.config.teams {
            let rows = self.round_players();
            let metrics = |team| {
                let alive = self.players.iter().filter(|p| p.alive() && p.team == team).count();
                let (sent, kos, score) = rows.iter().filter(|p| p.team == team).fold((0u64, 0u64, 0u64), |a, p|
                    (a.0 + p.sent as u64, a.1 + p.kos as u64, a.2 + p.score as u64));
                (alive, sent, kos, score)
            };
            let a = metrics(0); let b = metrics(1);
            let comparison = if !timed_out && a.0 == 0 && b.0 == 0 { std::cmp::Ordering::Equal } else { a.cmp(&b) };
            let places = match comparison {
                std::cmp::Ordering::Greater => [1, 2],
                std::cmp::Ordering::Less => [2, 1],
                _ => [1, 1],
            };
            for p in self.players.iter_mut().filter(|p| p.in_game) { p.place = places[p.team as usize]; }
            for p in &mut self.departed { p.place = places[p.team as usize]; }
            let mut winners: Vec<_> = rows.iter().filter(|p| places[p.team as usize] == 1).map(|p| p.id).collect();
            winners.sort_unstable();
            if places[0] != places[1] {
                for p in self.players.iter_mut().filter(|p| p.in_game && winners.contains(&p.id)) {
                    p.wins = p.wins.saturating_add(1);
                }
                self.log(format!("{}팀 승리", if places[0] == 1 { "A" } else { "B" }));
            } else { self.log("팀전 공동 1위 무승부".into()); }
            winners
        } else {
            // Survivors rank above eliminated players; preserve eliminated places.
            let mut survivors: Vec<_> = self.players.iter().filter(|p| p.alive()).map(|p| {
                let g = p.game.as_ref().unwrap();
                (p.id, (g.stats.sent, p.kos, g.stats.score))
            }).collect();
            survivors.sort_by(|a, b| b.1.cmp(&a.1));
            let mut winners = Vec::new();
            let mut previous = None;
            let mut place = 0;
            for (index, &(id, metrics)) in survivors.iter().enumerate() {
                if previous != Some(metrics) { place = index as u8 + 1; previous = Some(metrics); }
                if let Some(p) = self.players.iter_mut().find(|p| p.id == id) { p.place = place; }
                if place == 1 && self.participants >= 2 { winners.push(id); }
            }
            if winners.len() == 1 {
                let winner = winners[0];
                if let Some(p) = self.players.iter_mut().find(|p| p.id == winner) { p.wins = p.wins.saturating_add(1); }
                self.log(format!("{} 승리{}", self.name_of(winner), if timed_out { " (제한 시간 종료)" } else { "" }));
            } else if winners.len() > 1 { self.log("제한 시간 종료 · 공동 1위 무승부".into()); }
            winners
        };
        for p in self.players.iter_mut().filter(|p| p.in_game) {
            p.rounds = p.rounds.saturating_add(1);
            p.total_sent = p.total_sent.saturating_add(p.game.as_ref().map_or(0, |g| g.stats.sent as u64));
        }
        let mut rows = self.round_players();
        rows.sort_by_key(|p| (p.place, p.id));
        let summary = MatchSummary { match_id: self.match_id, elapsed_ticks: self.elapsed_ticks, timed_out,
            teams: self.config.teams, winners: winners.clone(), players: rows.clone() };
        if !self.coop.enabled && self.participants >= 2 {
            for (token, (sent, won)) in crate::profile::merged_results(&summary, &self.profiles) {
                let key = format!("{}:{}",self.code,self.match_id);
                tokio::task::spawn_blocking(move || { if crate::profile::record_match(&token, &key, sent, won).is_err() { eprintln!("Profile result could not be persisted"); } });
            }
        }
        self.last_summary = Some(summary.clone());
        let mut ranking: Vec<(u8, u8)> = rows.iter().map(|p| (p.place, p.id)).collect();
        ranking.sort();
        self.send_states();
        self.send_boards(true);
        self.send_scores(true);
        self.broadcast(&self.match_info());
        self.phase = Phase::Lobby;
        for p in &mut self.players {
            p.in_game = false;
        }
        self.broadcast(&ServerMsg::GameOver { ranking: ranking.into_iter().map(|r| r.1).collect() });
        self.broadcast(&ServerMsg::BattleResult { timed_out, winners });
        self.broadcast(&ServerMsg::MatchSummary(summary));
        self.reset_ready();
        self.broadcast(&self.room_state());
        self.broadcast(&self.lobby_msg());
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use bs_engine::protocol::{decode, MAX_PLAYERS};

    fn fixture(count: usize) -> (Room, Vec<mpsc::Receiver<Bytes>>) {
        let mut room = Room {
            code: "CLOCK".into(), registry: Default::default(), players: Vec::new(), host: None,
            phase: Phase::Lobby, countdown: 0, elapsed_ticks: 0, ticks: 0, next_id: 1,
            rng: Rng::new(1), participants: 0, finished: 0, last_scores: Vec::new(), chat: VecDeque::new(),
            config: RoomConfig::default(), match_id: 1, last_summary: None, departed: Vec::new(), profiles: HashMap::new(), coop: Default::default(),
        };
        let receivers = (0..count).map(|i| {
            let (tx, rx) = mpsc::channel(4096);
            room.add_player(format!("p{i}"), Some(tx), false).unwrap();
            rx
        }).collect();
        (room, receivers)
    }

    #[tokio::test(start_paused = true)]
    async fn first_match_requires_guest_readiness_and_host_start_is_consent() {
        let (mut room, _) = fixture(2);
        room.on_msg(1, ClientMsg::Start);
        assert_eq!(room.phase, Phase::Lobby);
        room.on_msg(2, ClientMsg::Ready { ready: true });
        assert_eq!(room.phase, Phase::Lobby);
        room.on_msg(1, ClientMsg::Start);
        assert_eq!(room.phase, Phase::Countdown);
    }

    #[tokio::test(start_paused = true)]
    async fn coop_is_host_only_has_waves_no_friendly_attack_and_shared_success() {
        let (mut room, _) = fixture(2);
        room.on_msg(2, ClientMsg::SetCoop { enabled: true });
        assert!(!room.coop.enabled);
        room.on_msg(1, ClientMsg::SetCoop { enabled: true });
        room.start(); freeze_falling(&mut room);
        ticks(&mut room, COUNTDOWN_TICKS + 600).await;
        assert!(room.players.iter().all(|p| p.game.as_ref().unwrap().pending_total() > 0));
        assert!(room.attack_targets(1).is_empty());
        room.players[1].game.as_mut().unwrap().dead = true;
        room.check_game_over();
        assert_eq!(room.phase, Phase::Playing, "one survivor can continue");
        room.coop.progress = 80; room.check_game_over();
        assert_eq!(room.coop.won, Some(true));
        assert_eq!(room.last_summary.as_ref().unwrap().winners, vec![1,2]);
        assert!(room.players.iter().all(|p| p.wins == 0));
    }

    #[tokio::test(start_paused = true)]
    async fn solo_coop_has_deadline_and_failure_is_not_a_win() {
        let (mut room, _) = fixture(1);
        room.on_msg(1, ClientMsg::SetCoop { enabled: true });
        room.start(); freeze_falling(&mut room); ticks(&mut room, COUNTDOWN_TICKS).await;
        assert!(matches!(room.match_info(),ServerMsg::MatchInfo {timed:true,..}));
        room.elapsed_ticks = room.limit_ticks(); room.check_game_over();
        assert_eq!(room.coop.won, Some(false)); assert!(room.last_summary.as_ref().unwrap().winners.is_empty());
    }

    // No active falling piece: keep both players alive to exercise the full
    // authoritative timer without depending on a particular board strategy.
    fn freeze_falling(room: &mut Room) {
        for p in &mut room.players {
            p.game.as_mut().unwrap().active = None;
        }
    }

    async fn ticks(room: &mut Room, count: u32) {
        for _ in 0..count {
            tokio::time::advance(TICK).await;
            room.step();
        }
    }

    fn drain(rx: &mut mpsc::Receiver<Bytes>) -> Vec<ServerMsg> {
        let mut messages = Vec::new();
        while let Ok(bytes) = rx.try_recv() {
            messages.push(decode(&bytes).unwrap());
        }
        messages
    }

    fn tetris_well(game: &mut Game, rows: usize) {
        use bs_engine::game::{Active, HEIGHT, WIDTH};
        use bs_engine::piece::Kind;
        game.board.fill(0);
        for y in HEIGHT - rows..HEIGHT {
            for x in 0..WIDTH { if x != 5 { game.board[y * WIDTH + x] = 8; } }
        }
        game.active = Some(Active { kind: Kind::I, rot: 1, x: 3, y: 0 });
        game.next[0] = Kind::I;
    }

    fn combat_events(messages: Vec<ServerMsg>) -> Vec<CombatEvent> {
        messages.into_iter().filter_map(|m| if let ServerMsg::CombatEvent(e) = m { Some(e) } else { None }).collect()
    }

    #[test]
    fn combat_opt_in_hydrates_active_state_without_sending_v2_to_legacy() {
        let (mut room, mut receivers) = fixture(2);
        room.start();
        room.phase = Phase::Playing;
        drain(&mut receivers[0]); drain(&mut receivers[1]);
        let game = room.players[0].game.as_mut().unwrap();
        game.combo = 3;
        game.combat.back_to_back = 4;
        game.combat.last_rotation = Some(bs_engine::game::RotationInfo { kick_index: 2 });
        let expected = game.combat.clone();
        room.on_msg(1, ClientMsg::EnableCombat);
        let messages = drain(&mut receivers[0]);
        assert!(messages.iter().any(|m| matches!(m, ServerMsg::YourStateV2 { combat, game, .. } if *combat == expected && game.combo == 3)));
        assert!(!messages.iter().any(|m| matches!(m, ServerMsg::YourState { .. })));
        assert!(messages.iter().any(|m| matches!(m, ServerMsg::CombatStatus(v) if v[0].combo == 3 && v[0].b2b == 4)));
        room.players[0].game.as_mut().unwrap().rev += 1;
        room.players[1].game.as_mut().unwrap().rev += 1;
        room.send_states();
        assert!(drain(&mut receivers[0]).iter().any(|m| matches!(m, ServerMsg::YourStateV2 { .. })));
        let legacy = drain(&mut receivers[1]);
        assert!(legacy.iter().any(|m| matches!(m, ServerMsg::YourState { .. })));
        assert!(!legacy.iter().any(|m| matches!(m, ServerMsg::YourStateV2 { .. })));
    }

    #[test]
    fn multiple_same_tick_locks_keep_piece_order_and_actual_enemy_targets() {
        let (mut room, mut receivers) = fixture(4);
        room.config.teams = true;
        room.start(); room.phase = Phase::Playing;
        drain(&mut receivers[0]);
        tetris_well(room.players[0].game.as_mut().unwrap(), 8);
        room.players[0].target = Some(3); // same team: must be retargeted.
        room.on_msg(1, ClientMsg::Input { seq: 1, input: Input::HardDrop });
        room.on_msg(1, ClientMsg::Input { seq: 2, input: Input::RotateCw });
        room.on_msg(1, ClientMsg::Input { seq: 3, input: Input::HardDrop });
        room.on_msg(1, ClientMsg::Input { seq: 4, input: Input::HardDrop });
        room.simulate();
        let events = combat_events(drain(&mut receivers[0]));
        assert_eq!(events.len(), 3);
        assert_eq!(events.iter().map(|e| e.piece).collect::<Vec<_>>(), vec![1, 2, 3]);
        assert!(events.iter().all(|e| e.match_id == room.match_id && e.player == 1));
        assert_eq!(events.iter().map(|e| e.combo).collect::<Vec<_>>(), vec![0, 1, -1]);
        assert_eq!((events[0].b2b, events[1].b2b, events[2].b2b), (1, 2, 2));
        assert!(!events[0].back_to_back && events[1].back_to_back);
        assert!(events[1].perfect_clear);
        for event in &events[..2] {
            assert!(matches!(event.target, Some(2) | Some(4)));
            assert!(event.sent > 0);
            assert_eq!(event.generated - event.cancelled, event.sent);
        }
        assert_eq!((events[2].sent, events[2].target), (0, None));
        let sent: u32 = events.iter().map(|e| e.sent as u32).sum();
        assert_eq!(room.players[0].game.as_ref().unwrap().stats.sent, sent);
        assert_eq!(room.players.iter().skip(1).map(|p| p.game.as_ref().unwrap().pending_total()).sum::<u32>(), sent);
    }

    #[test]
    fn gravity_lock_emits_one_zero_clear_reset_event_without_input() {
        use bs_engine::game::{Active, HEIGHT, LOCK_DELAY};
        use bs_engine::piece::Kind;
        let (mut room, mut receivers) = fixture(1);
        room.start(); room.phase = Phase::Playing;
        drain(&mut receivers[0]);
        let game = room.players[0].game.as_mut().unwrap();
        game.active = Some(Active { kind: Kind::O, rot: 0, x: 4, y: HEIGHT as i8 - 2 });
        game.combo = 4;
        game.combat.back_to_back = 2;
        for _ in 0..LOCK_DELAY + 2 { room.simulate(); }
        let events = combat_events(drain(&mut receivers[0]));
        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert_eq!((event.piece, event.cleared, event.combo, event.b2b), (1, 0, -1, 2));
        assert_eq!((event.generated, event.cancelled, event.sent, event.target), (0, 0, 0, None));
    }

    #[test]
    fn cancellation_and_missing_enemies_report_zero_delivered_lines() {
        for (count, cancel) in [(2, true), (1, false)] {
            let (mut room, mut receivers) = fixture(count);
            room.start(); room.phase = Phase::Playing;
            drain(&mut receivers[0]);
            let game = room.players[0].game.as_mut().unwrap();
            tetris_well(game, 4);
            if cancel { game.receive_garbage(8); }
            room.on_msg(1, ClientMsg::Input { seq: 1, input: Input::HardDrop });
            room.simulate();
            let events = combat_events(drain(&mut receivers[0]));
            assert_eq!(events.len(), 1);
            let event = &events[0];
            assert_eq!(event.generated, 8);
            assert_eq!((event.sent, event.target), (0, None));
            assert_eq!(event.cancelled, if cancel { 8 } else { 0 });
            assert_eq!(room.players[0].game.as_ref().unwrap().stats.sent, if cancel { 0 } else { 8 }, "retain engine outgoing stats even without enemy");
        }
    }

    #[test]
    fn combat_status_late_join_snapshot_and_new_match_reset() {
        let (mut room, mut receivers) = fixture(1);
        room.start(); room.phase = Phase::Playing;
        room.players[0].game.as_mut().unwrap().combo = 5;
        room.players[0].game.as_mut().unwrap().combat.back_to_back = 3;
        let (tx, mut rx) = mpsc::channel(256);
        let (reply, _) = oneshot::channel();
        room.handle(RoomCmd::Join { name: "spectator".into(), tx, reply });
        assert!(drain(&mut rx).contains(&ServerMsg::CombatStatus(vec![
            CombatStatus { id: 1, combo: 5, b2b: 3 }, CombatStatus { id: 2, combo: -1, b2b: 0 },
        ])));
        drain(&mut receivers[0]);
        let previous_match = room.match_id;
        room.start();
        assert_ne!(room.match_id, previous_match);
        assert!(drain(&mut receivers[0]).contains(&ServerMsg::CombatStatus(vec![
            CombatStatus { id: 1, combo: -1, b2b: 0 }, CombatStatus { id: 2, combo: -1, b2b: 0 },
        ])));
    }

    #[tokio::test(start_paused = true)]
    async fn battle_expires_at_7200_playing_ticks_excluding_countdown() {
        let (mut room, mut receivers) = fixture(2);
        room.start();
        freeze_falling(&mut room);
        ticks(&mut room, COUNTDOWN_TICKS - 1).await;
        assert_eq!(room.phase, Phase::Countdown);
        assert_eq!(room.elapsed_ticks, 0);
        assert_eq!(room.players[0].game.as_ref().unwrap().elapsed, 0);
        ticks(&mut room, 1).await;
        assert_eq!(room.phase, Phase::Playing);
        assert_eq!(room.elapsed_ticks, 0);
        ticks(&mut room, BATTLE_TICKS - 1).await;
        assert_eq!(room.phase, Phase::Playing);
        assert_eq!(room.match_info(), ServerMsg::MatchInfo { remaining_secs: 1, elapsed_ticks: 7199, timed: true });
        drain(&mut receivers[0]);
        ticks(&mut room, 1).await;
        assert_eq!(room.phase, Phase::Lobby);
        assert_eq!(room.elapsed_ticks, BATTLE_TICKS);
        assert!(room.players.iter().all(|p| p.place == 1 && p.wins == 0 && p.rounds == 1));
        let messages = drain(&mut receivers[0]);
        assert!(messages.contains(&ServerMsg::MatchInfo { remaining_secs: 0, elapsed_ticks: 7200, timed: true }));
        assert!(messages.contains(&ServerMsg::BattleResult { timed_out: true, winners: vec![1, 2] }));
        assert!(messages.contains(&ServerMsg::GameOver { ranking: vec![1, 2] }));
        assert!(messages.iter().any(|m| matches!(m, ServerMsg::BattleStats(v) if v.iter().all(|p| p.rounds == 1 && p.active_ticks == 7200))));
        room.check_game_over();
        room.finish(true);
        assert!(room.players.iter().all(|p| p.rounds == 1), "finish counted twice");
    }

    #[tokio::test(start_paused = true)]
    async fn solo_practice_is_unlimited_and_clock_updates_each_second() {
        let (mut room, mut receivers) = fixture(1);
        room.start();
        freeze_falling(&mut room);
        ticks(&mut room, COUNTDOWN_TICKS).await;
        drain(&mut receivers[0]);
        ticks(&mut room, BATTLE_TICKS + TICKS_PER_SECOND).await;
        assert_eq!(room.phase, Phase::Playing);
        assert_eq!(room.players[0].rounds, 0);
        let messages = drain(&mut receivers[0]);
        let clocks: Vec<_> = messages.iter().filter_map(|m| {
            if let ServerMsg::MatchInfo { remaining_secs, elapsed_ticks, timed } = m {
                assert!(!timed);
                assert_eq!(*remaining_secs, 0);
                Some(*elapsed_ticks)
            } else { None }
        }).collect();
        assert_eq!(clocks, (1..=121).map(|s| s * TICKS_PER_SECOND).collect::<Vec<_>>());
        assert_eq!(room.battle_stats()[0].active_ticks, 7260);
    }

    #[tokio::test(start_paused = true)]
    async fn elimination_finishes_early_and_counts_each_round_once() {
        let (mut room, mut receivers) = fixture(1);
        room.add_player("bot".into(), None, true).unwrap();
        room.start();
        freeze_falling(&mut room);
        ticks(&mut room, COUNTDOWN_TICKS + 60).await;
        room.players[0].game.as_mut().unwrap().stats.sent = 7;
        room.players[0].game.as_mut().unwrap().dead = true;
        room.players[0].place = 2;
        room.players[1].game.as_mut().unwrap().stats.sent = 12;
        room.check_game_over();
        assert_eq!(room.phase, Phase::Lobby);
        assert_eq!(room.players[1].wins, 1);
        assert_eq!(room.players[1].rounds, 1);
        assert_eq!(room.players[1].total_sent, 12);
        assert!(drain(&mut receivers[0]).contains(&ServerMsg::BattleResult { timed_out: false, winners: vec![2] }));
        room.start();
        freeze_falling(&mut room);
        assert_eq!(room.battle_stats()[0], BattleStat { id: 1, active_ticks: 0, rounds: 1, total_sent: 7 });
        ticks(&mut room, COUNTDOWN_TICKS + 60).await;
        room.players[0].game.as_mut().unwrap().stats.sent = 3;
        room.players[0].game.as_mut().unwrap().dead = true;
        room.players[0].place = 2;
        room.players[1].game.as_mut().unwrap().stats.sent = 8;
        room.check_game_over();
        assert_eq!(room.players[0].rounds, 2);
        assert_eq!(room.players[0].total_sent, 10);
        assert_eq!(room.players[1].rounds, 2);
        assert_eq!(room.players[1].total_sent, 20);
    }

    #[tokio::test(start_paused = true)]
    async fn timeout_orders_survivors_by_sent_then_kos_then_score_and_preserves_dead_place() {
        let (mut room, mut receivers) = fixture(6);
        room.start();
        freeze_falling(&mut room);
        ticks(&mut room, COUNTDOWN_TICKS).await;
        // id 1 has the highest score but less attack; id 2 loses on KO;
        // id 3 loses on score; ids 4/5 share first; dead id 6 stays last.
        for (p, (sent, kos, score)) in room.players.iter_mut().zip([
            (4, 99, 999), (5, 0, 999), (5, 1, 10), (5, 1, 20), (5, 1, 20), (99, 99, 999),
        ]) {
            let g = p.game.as_mut().unwrap();
            g.stats.sent = sent;
            g.stats.score = score;
            p.kos = kos;
        }
        room.players[5].game.as_mut().unwrap().dead = true;
        room.players[5].place = 6;
        room.elapsed_ticks = BATTLE_TICKS;
        room.check_game_over();
        assert_eq!(room.players.iter().map(|p| p.place).collect::<Vec<_>>(), vec![5, 4, 3, 1, 1, 6]);
        assert!(room.players.iter().all(|p| p.wins == 0));
        assert!(drain(&mut receivers[0]).contains(&ServerMsg::BattleResult { timed_out: true, winners: vec![4, 5] }));
    }

    #[tokio::test(start_paused = true)]
    async fn dead_players_apm_denominator_stops_and_late_join_gets_clock_and_stats() {
        let (mut room, _) = fixture(3);
        room.start();
        freeze_falling(&mut room);
        ticks(&mut room, COUNTDOWN_TICKS + 120).await;
        room.players[0].game.as_mut().unwrap().dead = true;
        room.players[0].game.as_mut().unwrap().stats.sent = 6;
        room.players[0].place = 3;
        ticks(&mut room, 180).await;
        let stats = room.battle_stats();
        assert_eq!(stats[0].active_ticks, 120);
        assert_eq!(stats[1].active_ticks, 300);
        assert_eq!(6 * 3600 / stats[0].active_ticks, 180, "dead player's APM must not fall while spectating");
        let (tx, mut rx) = mpsc::channel(128);
        let (reply, _) = oneshot::channel();
        room.handle(RoomCmd::Join { name: "late".into(), tx, reply });
        let messages = drain(&mut rx);
        assert!(messages.contains(&ServerMsg::MatchInfo { remaining_secs: 115, elapsed_ticks: 300, timed: true }));
        assert!(messages.iter().any(|m| matches!(m, ServerMsg::BattleStats(v) if v.len() == 4 && v[0].active_ticks == 120 && v[3].active_ticks == 0)));
        assert!(!room.players[3].in_game);
        room.players[1].game.as_mut().unwrap().stats.sent = 8;
        room.players[2].game.as_mut().unwrap().stats.sent = 2;
        room.elapsed_ticks = BATTLE_TICKS;
        room.check_game_over();
        assert_eq!(room.players[1].wins, 1, "unique timeout leader wins");
        assert!(drain(&mut rx).contains(&ServerMsg::BattleResult { timed_out: true, winners: vec![2] }));
        assert_eq!(room.players[3].rounds, 0, "spectator must not get a completed round");
    }

    #[tokio::test(start_paused = true)]
    async fn settings_are_host_only_atomic_capacity_checked_and_change_clock() {
        let (mut room, mut rx) = fixture(3);
        let config = RoomConfig { public: false, duration_secs: 60, max_players: 4, bot_level: 2, teams: true };
        room.on_msg(2, ClientMsg::SetRoomConfig(config.clone()));
        assert_eq!(room.config, RoomConfig::default());
        assert!(drain(&mut rx[1]).iter().any(|m| matches!(m, ServerMsg::Error(_))));
        room.on_msg(1, ClientMsg::SetRoomConfig(RoomConfig { max_players: 2, ..config.clone() }));
        assert_eq!(room.config, RoomConfig::default());
        room.on_msg(1, ClientMsg::SetRoomConfig(RoomConfig { duration_secs: 90, ..config.clone() }));
        assert_eq!(room.config, RoomConfig::default());
        room.on_msg(1, ClientMsg::SetRoomConfig(config.clone()));
        assert_eq!(room.config, config);
        assert_eq!(room.limit_ticks(), 3600);
        assert_eq!(room.players.iter().map(|p| p.team).collect::<Vec<_>>(), vec![0,1,0]);
        room.on_msg(1, ClientMsg::Start);
        assert_eq!(room.phase, Phase::Lobby, "incomplete teams must not start");
        room.on_msg(1, ClientMsg::AddBot);
        room.on_msg(1, ClientMsg::AddBot);
        assert_eq!(room.players.len(), 4, "configured capacity must limit bots");
        assert!(room.add_player("extra".into(), None, false).is_err());
        for p in &mut room.players { p.ready = true; }
        room.on_msg(1, ClientMsg::Start);
        assert_eq!(room.phase, Phase::Countdown);
        room.on_msg(1, ClientMsg::SetRoomConfig(RoomConfig::default()));
        assert_eq!(room.config, config, "settings cannot change during a match");
    }

    #[tokio::test(start_paused = true)]
    async fn teams_exclude_friendly_targets_and_share_win_including_eliminated_members() {
        let (mut room, mut rx) = fixture(4);
        room.configure(1, RoomConfig { teams: true, max_players: 4, ..Default::default() });
        room.start(); freeze_falling(&mut room);
        ticks(&mut room, COUNTDOWN_TICKS + 1).await;
        assert_eq!(room.attack_targets(1), vec![2,4]);
        // A player can be eliminated while their teammate continues the team round.
        room.players[0].game.as_mut().unwrap().dead = true;
        room.players[0].place = 4;
        room.check_game_over();
        assert_eq!(room.phase, Phase::Playing);
        room.players[1].game.as_mut().unwrap().dead = true;
        room.players[3].game.as_mut().unwrap().dead = true;
        room.check_game_over();
        assert_eq!(room.phase, Phase::Lobby);
        assert_eq!(room.players.iter().map(|p| p.place).collect::<Vec<_>>(), vec![1,2,1,2]);
        assert_eq!(room.players.iter().map(|p| p.wins).collect::<Vec<_>>(), vec![1,0,1,0]);
        let summary = room.last_summary.clone().unwrap();
        assert_eq!(summary.winners, vec![1,3]);
        assert!(summary.teams);
        assert_eq!(summary.players.len(), 4);
        let messages = drain(&mut rx[0]);
        assert_eq!(messages.iter().filter(|m| matches!(m, ServerMsg::MatchSummary(_))).count(), 1);
        room.finish(false);
        assert!(drain(&mut rx[0]).is_empty(), "summary must only be broadcast once");
    }

    #[tokio::test(start_paused = true)]
    async fn team_timeout_ties_draw_and_departed_members_keep_their_result() {
        let (mut room, _) = fixture(4);
        room.configure(1, RoomConfig { teams: true, max_players: 4, ..Default::default() });
        room.start(); freeze_falling(&mut room);
        ticks(&mut room, COUNTDOWN_TICKS).await;
        room.elapsed_ticks = room.limit_ticks();
        room.check_game_over();
        assert!(room.players.iter().all(|p| p.place == 1 && p.wins == 0));
        assert_eq!(room.last_summary.as_ref().unwrap().winners, vec![1,2,3,4]);
        room.start(); freeze_falling(&mut room);
        ticks(&mut room, COUNTDOWN_TICKS).await;
        room.players[0].game.as_mut().unwrap().stats.sent = 12;
        room.remove_player(1);
        assert_eq!(room.phase, Phase::Playing);
        room.players.iter_mut().filter(|p| p.team == 1).for_each(|p| p.game.as_mut().unwrap().dead = true);
        room.check_game_over();
        let summary = room.last_summary.as_ref().unwrap();
        assert_eq!(summary.players.len(), 4);
        assert_eq!(summary.winners, vec![1,3]);
        assert!(summary.players.iter().any(|p| p.id == 1 && p.sent == 12 && p.place == 1));
    }

    #[tokio::test(start_paused = true)]
    async fn rematch_requires_readiness_and_membership_changes_reset_it() {
        let (mut room, mut receivers) = fixture(2);
        room.on_msg(2, ClientMsg::Ready { ready: true });
        room.on_msg(1, ClientMsg::Start);
        freeze_falling(&mut room);
        ticks(&mut room, COUNTDOWN_TICKS + 1).await;
        room.players[1].game.as_mut().unwrap().dead = true;
        room.players[1].place = 2;
        room.check_game_over();
        let first_id = room.last_summary.as_ref().unwrap().match_id;
        room.on_msg(1, ClientMsg::Start);
        assert_eq!(room.phase, Phase::Lobby, "host cannot force an unready rematch");
        room.on_msg(2, ClientMsg::Ready { ready: true });
        assert_eq!(room.phase, Phase::Lobby);
        let (tx, mut late) = mpsc::channel(128);
        let (reply, _) = oneshot::channel();
        room.handle(RoomCmd::Join { name: "new".into(), tx, reply });
        assert!(room.players.iter().all(|p| !p.ready));
        let replay = drain(&mut late);
        assert!(replay.contains(&ServerMsg::MatchSummary(room.last_summary.clone().unwrap())));
        assert!(!room.last_summary.as_ref().unwrap().players.iter().any(|p| p.id == 3), "new spectator must not be recorded in prior match");
        room.remove_player(3);
        room.on_msg(2, ClientMsg::Ready { ready: true });
        room.configure(1, RoomConfig::default());
        assert!(room.players.iter().all(|p| !p.ready));
        room.on_msg(2, ClientMsg::Ready { ready: true });
        room.on_msg(1, ClientMsg::Ready { ready: true });
        assert_eq!(room.phase, Phase::Countdown, "everyone ready should automatically rematch");
        assert!(room.match_id > first_id);
        assert!(room.players.iter().all(|p| !p.ready));
        assert!(drain(&mut receivers[0]).iter().any(|m| matches!(m, ServerMsg::RoomState { .. })));
    }

    #[tokio::test(start_paused = true)]
    async fn team_selection_resets_ready_and_difficulty_changes_bot_speed() {
        let (mut room, _) = fixture(1);
        room.on_msg(1, ClientMsg::AddBot);
        room.configure(1, RoomConfig { teams: true, max_players: 4, bot_level: 0, ..Default::default() });
        let slow = room.players[1].bot.as_ref().unwrap().think;
        room.players[0].ready = true;
        room.on_msg(1, ClientMsg::SetTeam { team: 1 });
        assert_eq!(room.players[0].team, 1);
        assert!(!room.players[0].ready);
        assert!(room.players[1].ready);
        room.on_msg(1, ClientMsg::SetTeam { team: 9 });
        assert_eq!(room.players[0].team, 1);
        room.configure(1, RoomConfig { teams: true, max_players: 4, bot_level: 2, ..Default::default() });
        assert!(room.players[1].bot.as_ref().unwrap().think < slow);
    }

    #[tokio::test(start_paused = true)]
    async fn three_vs_three_requires_balance_and_timeout_prefers_survival_then_team_attack() {
        let (mut room, _) = fixture(6);
        room.configure(1, RoomConfig { teams: true, ..Default::default() });
        room.set_team(1, 1);
        room.on_msg(1, ClientMsg::Start);
        assert_eq!(room.phase, Phase::Lobby, "unbalanced full teams must not start");
        room.set_team(1, 0);
        for p in &mut room.players { p.ready = true; }
        room.on_msg(1, ClientMsg::Start);
        freeze_falling(&mut room);
        ticks(&mut room, COUNTDOWN_TICKS).await;
        room.players[0].game.as_mut().unwrap().stats.sent = 100;
        room.players[0].game.as_mut().unwrap().dead = true;
        room.elapsed_ticks = room.limit_ticks();
        room.check_game_over();
        assert_eq!(room.last_summary.as_ref().unwrap().winners, vec![2,4,6]);
        room.start(); freeze_falling(&mut room);
        ticks(&mut room, COUNTDOWN_TICKS).await;
        room.players[0].game.as_mut().unwrap().stats.sent = 5;
        room.players[2].game.as_mut().unwrap().stats.sent = 7;
        room.players[1].game.as_mut().unwrap().stats.sent = 10;
        room.players[1].kos = 99;
        room.elapsed_ticks = room.limit_ticks();
        room.check_game_over();
        assert_eq!(room.last_summary.as_ref().unwrap().winners, vec![1,3,5], "sum team attack before KO");
    }

    async fn recv_until<F: FnMut(&ServerMsg) -> bool>(rx: &mut mpsc::Receiver<Bytes>, mut f: F) -> Vec<ServerMsg> {
        let mut seen = Vec::new();
        loop {
            let b = tokio::time::timeout(Duration::from_secs(120), rx.recv()).await.expect("timeout").expect("closed");
            let m: ServerMsg = decode(&b).expect("decode");
            let done = f(&m);
            seen.push(m);
            if done {
                return seen;
            }
        }
    }

    #[tokio::test(start_paused = true)]
    async fn full_round_with_bot_ends_and_counts_win() {
        let reg: Registry = Default::default();
        let room = spawn_room("TEST1".into(), reg.clone());
        reg.lock().unwrap().insert("TEST1".into(), RoomEntry { tx: room.clone(), summary: Default::default(), public: true, coop: false, config: RoomConfig::default() });

        let (tx, mut rx) = mpsc::channel(4096);
        let (rtx, rrx) = oneshot::channel();
        room.send(RoomCmd::Join { name: "사람".into(), tx, reply: rtx }).await.unwrap();
        let me = rrx.await.unwrap().unwrap();

        room.send(RoomCmd::Msg { id: me, msg: ClientMsg::AddBot }).await.unwrap();
        room.send(RoomCmd::Msg { id: me, msg: ClientMsg::Start }).await.unwrap();
        recv_until(&mut rx, |m| matches!(m, ServerMsg::Countdown { seconds: 0 })).await;

        // 사람 플레이어는 계속 바로 내리기만 해서 빨리 탈락한다
        for seq in 1..=60 {
            room.send(RoomCmd::Msg { id: me, msg: ClientMsg::Input { seq, input: Input::HardDrop } }).await.unwrap();
        }
        let msgs = recv_until(&mut rx, |m| matches!(m, ServerMsg::GameOver { .. })).await;
        let ranking = match msgs.last() {
            Some(ServerMsg::GameOver { ranking }) => ranking.clone(),
            _ => unreachable!(),
        };
        assert_eq!(ranking.len(), 2);
        assert_ne!(ranking[0], me, "봇이 1위여야 함");
        // 권위 있는 상태가 전송되었고, ack 가 입력 seq 를 따라간다
        assert!(msgs.iter().any(|m| matches!(m, ServerMsg::YourState { ack, .. } if *ack > 0)));
        assert!(msgs.iter().any(|m| matches!(m, ServerMsg::Boards(_))));

        // 다음 로비 메시지에서 봇의 승수가 1
        let lobby = recv_until(&mut rx, |m| matches!(m, ServerMsg::Lobby { .. })).await;
        if let Some(ServerMsg::Lobby { players, phase, .. }) = lobby.last() {
            assert_eq!(*phase, Phase::Lobby);
            assert_eq!(players.iter().find(|p| p.bot).unwrap().wins, 1);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn room_rejects_seventh_player() {
        let reg: Registry = Default::default();
        let room = spawn_room("FULL1".into(), reg.clone());
        let mut keep = Vec::new();
        for i in 0..7 {
            let (tx, rx) = mpsc::channel(64);
            keep.push(rx);
            let (rtx, rrx) = oneshot::channel();
            room.send(RoomCmd::Join { name: format!("p{i}"), tx, reply: rtx }).await.unwrap();
            let res = rrx.await.unwrap();
            assert_eq!(res.is_ok(), i < MAX_PLAYERS, "player {i}");
        }
    }

    #[tokio::test(start_paused = true)]
    async fn chat_is_broadcast_rate_limited_and_replayed_to_newcomers() {
        let reg: Registry = Default::default();
        let room = spawn_room("CHAT1".into(), reg.clone());
        let (tx_a, mut rx_a) = mpsc::channel(256);
        let (rtx, rrx) = oneshot::channel();
        room.send(RoomCmd::Join { name: "가".into(), tx: tx_a, reply: rtx }).await.unwrap();
        let a = rrx.await.unwrap().unwrap();

        // 6줄 연속 → 5줄만 통과, 6번째는 오류
        for i in 0..6 {
            room.send(RoomCmd::Msg { id: a, msg: ClientMsg::Chat { text: format!("  안녕 {i}\n ") } }).await.unwrap();
        }
        let mut lines = Vec::new();
        let mut errors = 0;
        let msgs = recv_until(&mut rx_a, |m| matches!(m, ServerMsg::Error(_))).await;
        for m in msgs {
            match m {
                ServerMsg::Chat(v) => lines.extend(v),
                ServerMsg::Error(_) => errors += 1,
                _ => {}
            }
        }
        assert_eq!(lines.len(), 5);
        assert_eq!(errors, 1);
        assert_eq!(lines[0].text, "안녕 0", "앞뒤 공백·제어문자 제거");
        assert_eq!(lines[0].name, "가");

        // 3초 지나면 다시 보낼 수 있고, 너무 긴 글은 잘린다
        tokio::time::sleep(Duration::from_secs(4)).await;
        room.send(RoomCmd::Msg { id: a, msg: ClientMsg::Chat { text: "가".repeat(500) } }).await.unwrap();
        let m = recv_until(&mut rx_a, |m| matches!(m, ServerMsg::Chat(_))).await;
        if let Some(ServerMsg::Chat(v)) = m.last() {
            assert_eq!(v[0].text.chars().count(), MAX_CHAT_LEN);
        }

        // 나중에 들어온 사람은 최근 기록 6줄을 한꺼번에 받는다
        let (tx_b, mut rx_b) = mpsc::channel(256);
        let (rtx, rrx) = oneshot::channel();
        room.send(RoomCmd::Join { name: "나".into(), tx: tx_b, reply: rtx }).await.unwrap();
        rrx.await.unwrap().unwrap();
        let m = recv_until(&mut rx_b, |m| matches!(m, ServerMsg::Chat(_))).await;
        if let Some(ServerMsg::Chat(v)) = m.last() {
            assert_eq!(v.len(), 6);
        }
    }
}
