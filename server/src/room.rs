//! 방(Room) 하나 = Tokio 태스크 하나.
//!
//! 방 태스크가 모든 플레이어의 게임 상태를 소유하고 고정 60Hz tick으로 진행한다.
//! 클라이언트는 입력만 보내고, 브라우저 탭이 비활성화되어 타이머가 느려져도
//! 게임은 서버에서 계속 진행된다.

use bs_engine::bot;
use bs_engine::game::{pack_cells, Game, Input, TICKS_PER_SECOND, VISIBLE_CELLS};
use bs_engine::protocol::{encode, ChatLine, ClientMsg, Phase, PlayerInfo, PlayerStat, RoomSummary, ServerMsg, MAX_CHAT_LEN, MAX_PLAYERS};
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
}

const TICK: Duration = Duration::from_micros(1_000_000 / TICKS_PER_SECOND as u64);
/// 상대 게임판 전송 주기 (tick) → 10Hz
const BOARDS_EVERY: u64 = 6;
/// 점수표 전송 주기 (tick) → 2Hz
const SCORES_EVERY: u64 = 30;
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
    ticks: u64,
    next_id: u8,
    rng: Rng,
    participants: usize,
    finished: u8,
    last_scores: Vec<PlayerStat>,
    chat: VecDeque<ChatLine>,
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
        ticks: 0,
        next_id: 1,
        rng: Rng::new(seed),
        participants: 0,
        finished: 0,
        last_scores: Vec::new(),
        chat: VecDeque::new(),
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
        if self.players.len() >= MAX_PLAYERS {
            return Err("방이 가득 찼습니다 (최대 6명)".into());
        }
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        self.players.push(Player {
            id,
            name,
            tx,
            bot: bot.then(|| {
                let n = self.players.iter().filter(|p| p.bot.is_some()).count() as u32;
                BotBrain { plan: VecDeque::new(), wait: 30, style: bot::Style::variant(n), think: 20 + 6 * (n % 4) }
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
        });
        if self.host.is_none() && !bot {
            self.host = Some(id);
        }
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
                    self.broadcast(&self.lobby_msg());
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
        if p.alive() {
            self.finished += 1;
        }
        if self.host == Some(id) {
            self.host = self.players.iter().find(|p| p.bot.is_none()).map(|p| p.id);
        }
        self.log(format!("{} 님이 나갔습니다", p.name));
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
            ClientMsg::Start if is_host && self.phase == Phase::Lobby => self.start(),
            ClientMsg::AddBot if is_host && self.phase == Phase::Lobby => {
                let n = self.players.iter().filter(|p| p.bot.is_some()).count() + 1;
                if self.add_player(format!("연습봇{n}"), None, true).is_ok() {
                    self.broadcast(&self.lobby_msg());
                }
            }
            ClientMsg::RemoveBot if is_host && self.phase == Phase::Lobby => {
                if let Some(idx) = self.players.iter().rposition(|p| p.bot.is_some()) {
                    self.players.remove(idx);
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
        self.ticks = 0;
        self.finished = 0;
        self.participants = self.players.len();
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
        self.broadcast(&ServerMsg::Started { seed });
        self.broadcast(&ServerMsg::Countdown { seconds: 3 });
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
                } else if self.countdown % TICKS_PER_SECOND == 0 {
                    self.broadcast(&ServerMsg::Countdown { seconds: (self.countdown / TICKS_PER_SECOND) as u8 });
                }
            }
            Phase::Playing => self.simulate(),
        }
    }

    fn simulate(&mut self) {
        // 1) 입력 + 중력 처리, 공격 수집
        let mut attacks: Vec<(u8, u8)> = Vec::new();
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
                    evs.push(game.apply_input(i));
                    brain.wait = 5;
                } else {
                    brain.plan = bot::plan_with(game, &brain.style).into();
                    brain.wait = brain.think;
                }
            } else {
                for _ in 0..MAX_INPUTS_PER_TICK {
                    let Some((seq, input)) = p.inputs.pop_front() else { break };
                    evs.push(game.apply_input(input));
                    p.ack = seq;
                }
            }
            evs.push(game.tick());
            for ev in evs {
                if ev.attack > 0 {
                    attacks.push((p.id, ev.attack));
                }
                if ev.died {
                    deaths.push(p.id);
                    break;
                }
            }
        }

        // 2) 공격 분배 (대상이 없거나 탈락했으면 무작위 생존자로 변경)
        for (from, lines) in attacks {
            let alive: Vec<u8> = self.players.iter().filter(|q| q.alive() && q.id != from).map(|q| q.id).collect();
            if alive.is_empty() {
                continue;
            }
            let cur = self.players.iter().find(|q| q.id == from).and_then(|q| q.target);
            let target = match cur {
                Some(t) if alive.contains(&t) => t,
                _ => alive[self.rng.below(alive.len() as u32) as usize],
            };
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
            let _ = tx.try_send(Bytes::from(encode(&ServerMsg::YourState { ack: p.ack, game: game.clone() })));
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
        let s = self.stats();
        if force || s != self.last_scores {
            self.broadcast(&ServerMsg::Scores(s.clone()));
            self.last_scores = s;
        }
    }

    fn check_game_over(&mut self) {
        if self.phase != Phase::Playing {
            return;
        }
        let alive: Vec<u8> = self.players.iter().filter(|p| p.alive()).map(|p| p.id).collect();
        let over = if self.participants >= 2 { alive.len() <= 1 } else { alive.is_empty() };
        if !over {
            return;
        }
        if self.participants >= 2 {
            if let Some(&w) = alive.first() {
                if let Some(p) = self.players.iter_mut().find(|p| p.id == w) {
                    p.wins += 1;
                    p.place = 1;
                }
                self.log(format!("{} 승리", self.name_of(w)));
            }
        }
        let mut ranking: Vec<(u8, u8)> =
            self.players.iter().filter(|p| p.in_game).map(|p| (if p.place == 0 { 1 } else { p.place }, p.id)).collect();
        ranking.sort();
        self.send_states();
        self.send_boards(true);
        self.send_scores(true);
        self.phase = Phase::Lobby;
        for p in &mut self.players {
            p.in_game = false;
        }
        self.broadcast(&ServerMsg::GameOver { ranking: ranking.into_iter().map(|r| r.1).collect() });
        self.broadcast(&self.lobby_msg());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bs_engine::protocol::decode;

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
        reg.lock().unwrap().insert("TEST1".into(), RoomEntry { tx: room.clone(), summary: Default::default() });

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
