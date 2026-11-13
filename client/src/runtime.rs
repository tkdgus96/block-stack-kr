//! 클라이언트 핵심: 네트워크, 입력 예측, 렌더 스케줄링, 성능 측정.
//!
//! 게임판은 Leptos 반응형 시스템을 거치지 않고 여기서 직접(명령형으로) 갱신한다.
//! Leptos 시그널은 점수표·로그·상태 표시처럼 드물게 바뀌는 UI 에만 사용한다.
//!
//! 렌더링 원칙
//! - 지속적인 requestAnimationFrame 루프 없음. 상태가 바뀌었을 때만 1회 예약
//! - 입력에 의한 갱신은 즉시 다음 프레임에 반영 (프레임 상한 무시)
//! - 서버/상대 게임판에 의한 갱신은 30fps(기본) 또는 60fps 상한, 상대 게임판은 10Hz(저사양 4Hz)
//! - 비활성 탭에서는 rAF 가 멈추므로 렌더링도 멈춘다. 게임은 서버에서 계속 진행된다

use crate::render::{Board, BoardSpec};
use crate::settings::{RendererKind, Settings, Theme};
use crate::sound::{self, Sfx};
use bs_engine::bot;
use bs_engine::game::{unpack_cells, Game, Input, PREVIEW, VISIBLE, VISIBLE_CELLS, WIDTH};
use bs_engine::protocol::{decode, encode, ChatLine, ClientMsg, Phase, PlayerInfo, PlayerStat, RoomSummary, ServerMsg};
use bs_engine::Kind;
use leptos::prelude::*;
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{BinaryType, KeyboardEvent, MessageEvent, WebSocket};

// ───────────────────────── UI 시그널 (저빈도) ─────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Conn {
    Connecting,
    Open,
    Closed,
    Offline,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PerfSummary {
    pub input_p50: f64,
    pub input_p95: f64,
    pub frame_p50: f64,
    pub board_p50: f64,
    pub board_max: f64,
    pub opp_p50: f64,
    pub rtt: f64,
    pub renders: u32,
    pub msgs: u32,
    pub kb_in: f64,
}

#[derive(Clone, Copy)]
pub struct Ui {
    pub conn: RwSignal<Conn>,
    pub room: RwSignal<String>,
    pub me: RwSignal<u8>,
    pub phase: RwSignal<Phase>,
    pub host: RwSignal<u8>,
    pub players: RwSignal<Vec<PlayerInfo>>,
    pub scores: RwSignal<Vec<PlayerStat>>,
    pub log: RwSignal<VecDeque<(String, String)>>,
    pub status: RwSignal<String>,
    pub countdown: RwSignal<u8>,
    pub ranking: RwSignal<Vec<u8>>,
    pub settings: RwSignal<Settings>,
    pub perf: RwSignal<PerfSummary>,
    pub rebinding: RwSignal<Option<usize>>,
    pub bench: RwSignal<bool>,
    /// 열린 방 목록 (방 목록 탭에서 요청할 때만 갱신)
    pub rooms: RwSignal<Vec<RoomSummary>>,
    pub rooms_loaded: RwSignal<bool>,
    /// 같은 방 채팅 (최근 100줄)
    pub chat: RwSignal<VecDeque<ChatLine>>,
}

impl Ui {
    pub fn new() -> Ui {
        Ui {
            conn: RwSignal::new(Conn::Offline),
            room: RwSignal::new(String::new()),
            me: RwSignal::new(0),
            phase: RwSignal::new(Phase::Lobby),
            host: RwSignal::new(0),
            players: RwSignal::new(Vec::new()),
            scores: RwSignal::new(Vec::new()),
            log: RwSignal::new(VecDeque::new()),
            status: RwSignal::new("대기".into()),
            countdown: RwSignal::new(0),
            ranking: RwSignal::new(Vec::new()),
            settings: RwSignal::new(Settings::load()),
            perf: RwSignal::new(PerfSummary::default()),
            rebinding: RwSignal::new(None),
            bench: RwSignal::new(false),
            rooms: RwSignal::new(Vec::new()),
            rooms_loaded: RwSignal::new(false),
            chat: RwSignal::new(VecDeque::new()),
        }
    }
}

// ───────────────────────── 유틸 ─────────────────────────

pub fn now() -> f64 {
    web_sys::window().and_then(|w| w.performance()).map(|p| p.now()).unwrap_or(0.0)
}

fn window() -> web_sys::Window {
    web_sys::window().unwrap()
}

fn clock() -> String {
    let d = js_sys::Date::new_0();
    format!("{:02}:{:02}:{:02}", d.get_hours(), d.get_minutes(), d.get_seconds())
}

struct Samples(VecDeque<f64>);

impl Samples {
    const CAP: usize = 4000;
    fn new() -> Self {
        Samples(VecDeque::new())
    }
    fn push(&mut self, v: f64) {
        if self.0.len() >= Self::CAP {
            self.0.pop_front();
        }
        self.0.push_back(v);
    }
    fn pct(&self, p: f64) -> f64 {
        if self.0.is_empty() {
            return 0.0;
        }
        let mut v: Vec<f64> = self.0.iter().copied().collect();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        v[((v.len() - 1) as f64 * p).round() as usize]
    }
    fn max(&self) -> f64 {
        self.0.iter().copied().fold(0.0, f64::max)
    }
    fn json(&self) -> String {
        let parts: Vec<String> = self.0.iter().map(|v| format!("{v:.3}")).collect();
        format!("[{}]", parts.join(","))
    }
}

struct Perf {
    input: Samples,
    frame: Samples,
    board: Samples,
    opp: Samples,
    rtt: f64,
    renders: u32,
    msgs: u32,
    bytes_in: u64,
    total_renders: u64,
    total_cells: u64,
}

impl Perf {
    fn new() -> Perf {
        Perf {
            input: Samples::new(),
            frame: Samples::new(),
            board: Samples::new(),
            opp: Samples::new(),
            rtt: 0.0,
            renders: 0,
            msgs: 0,
            bytes_in: 0,
            total_renders: 0,
            total_cells: 0,
        }
    }
}

// ───────────────────────── 코어 상태 ─────────────────────────

struct Opp {
    cells: Vec<u8>,
    dirty: bool,
    board: Option<Board>,
}

struct BenchState {
    opps: Vec<Game>,
    plans: Vec<VecDeque<Input>>,
    own_plan: VecDeque<Input>,
    tick: u64,
    start: f64,
}

pub struct Core {
    ui: Ui,
    settings: Settings,
    ws: Option<WebSocket>,
    want_room: String,
    reconnect_ms: i32,
    closing: bool,
    me: u8,
    phase: Phase,
    server_game: Option<Game>,
    predicted: Option<Game>,
    pending: VecDeque<(u32, Input)>,
    seq: u32,
    last_received: u32,
    own: Option<Board>,
    previews: Vec<Option<Board>>,
    opps: HashMap<u8, Opp>,
    own_dirty: bool,
    opp_dirty: bool,
    raf_pending: bool,
    timer_pending: bool,
    last_render: f64,
    last_opp_render: f64,
    frame_marks: Vec<f64>,
    buf: Vec<u8>,
    held: [Option<i32>; 3],
    perf: Perf,
    bench: Option<BenchState>,
    board_marked: bool,
    opp_cell: u8,
}

struct Callbacks {
    raf: Closure<dyn FnMut(f64)>,
    timer: Closure<dyn FnMut()>,
    das: [Closure<dyn FnMut()>; 3],
    rep: [Closure<dyn FnMut()>; 3],
    reconnect: Closure<dyn FnMut()>,
}

thread_local! {
    static CORE: RefCell<Option<Core>> = const { RefCell::new(None) };
    static CB: RefCell<Option<Callbacks>> = const { RefCell::new(None) };
}

fn with<R>(f: impl FnOnce(&mut Core) -> R) -> Option<R> {
    CORE.with(|c| match c.try_borrow_mut() {
        Ok(mut c) => c.as_mut().map(f),
        Err(_) => None,
    })
}

fn with_cb(f: impl FnOnce(&Callbacks)) {
    CB.with(|cb| {
        if let Some(cb) = cb.borrow().as_ref() {
            f(cb)
        }
    });
}

const HELD_INPUTS: [Input; 3] = [Input::Left, Input::Right, Input::SoftDrop];

fn held_index(i: Input) -> Option<usize> {
    HELD_INPUTS.iter().position(|&h| h == i)
}

// ───────────────────────── 초기화 ─────────────────────────

pub fn init(ui: Ui) {
    let settings = ui.settings.get_untracked();
    CORE.with(|c| {
        *c.borrow_mut() = Some(Core {
            ui,
            settings,
            ws: None,
            want_room: String::new(),
            reconnect_ms: 1000,
            closing: false,
            me: 0,
            phase: Phase::Lobby,
            server_game: None,
            predicted: None,
            pending: VecDeque::new(),
            seq: 0,
            last_received: 0,
            own: None,
            previews: (0..=PREVIEW).map(|_| None).collect(),
            opps: HashMap::new(),
            own_dirty: false,
            opp_dirty: false,
            raf_pending: false,
            timer_pending: false,
            last_render: 0.0,
            last_opp_render: 0.0,
            frame_marks: Vec::new(),
            buf: vec![0; VISIBLE_CELLS],
            held: [None; 3],
            perf: Perf::new(),
            bench: None,
            board_marked: false,
            opp_cell: 0,
        })
    });

    let mk_das = |i: usize| Closure::<dyn FnMut()>::new(move || with(|c| c.on_das(i)).unwrap_or(()));
    let mk_rep = |i: usize| {
        Closure::<dyn FnMut()>::new(move || {
            with(|c| c.local_input(HELD_INPUTS[i], now()));
        })
    };
    CB.with(|cb| {
        *cb.borrow_mut() = Some(Callbacks {
            raf: Closure::new(move |_ts: f64| {
                with(|c| c.on_frame());
            }),
            timer: Closure::new(move || {
                with(|c| {
                    c.timer_pending = false;
                    c.request_raf();
                });
            }),
            das: [mk_das(0), mk_das(1), mk_das(2)],
            rep: [mk_rep(0), mk_rep(1), mk_rep(2)],
            reconnect: Closure::new(move || {
                with(|c| c.connect());
            }),
        })
    });

    install_listeners();
    expose_perf_api();
    // 대기실에서도 빈 시트(게임판)를 바로 보여준다
    with(|c| {
        c.own_dirty = true;
        c.request(true);
    });

    // 1초마다 성능 요약 갱신 (비활성 탭에서는 브라우저가 자연스럽게 늦춘다)
    let summary = Closure::<dyn FnMut()>::new(move || {
        with(|c| c.publish_perf());
    });
    let _ = window().set_interval_with_callback_and_timeout_and_arguments_0(summary.as_ref().unchecked_ref(), 1000);
    summary.forget();

    // 5초마다 RTT 측정
    let ping = Closure::<dyn FnMut()>::new(move || {
        with(|c| c.send(&ClientMsg::Ping { t: now() as u32 }));
    });
    let _ = window().set_interval_with_callback_and_timeout_and_arguments_0(ping.as_ref().unchecked_ref(), 5000);
    ping.forget();

    // URL 파라미터: ?bench=dom|canvas&opps=5 → 서버 없이 렌더링 벤치마크, 그 외에는 접속
    let search = window().location().search().unwrap_or_default();
    let params = web_sys::UrlSearchParams::new_with_str(&search).ok();
    let get = |k: &str| params.as_ref().and_then(|p| p.get(k));
    if let Some(mode) = get("bench") {
        let opps = get("opps").and_then(|v| v.parse().ok()).unwrap_or(5usize).min(5);
        let kind = if mode == "canvas" { RendererKind::Canvas } else { RendererKind::Dom };
        let low = get("low").as_deref() == Some("1");
        start_bench(kind, opps, low);
    } else {
        let room = get("room").unwrap_or_default();
        with(|c| {
            c.want_room = room;
            c.connect();
        });
    }
}

fn install_listeners() {
    let w = window();
    let keydown = Closure::<dyn FnMut(KeyboardEvent)>::new(move |e: KeyboardEvent| {
        with(|c| c.on_keydown(&e));
    });
    let keyup = Closure::<dyn FnMut(KeyboardEvent)>::new(move |e: KeyboardEvent| {
        with(|c| c.on_keyup(&e));
    });
    let blur = Closure::<dyn FnMut()>::new(move || {
        with(|c| c.release_all());
    });
    let _ = w.add_event_listener_with_callback("keydown", keydown.as_ref().unchecked_ref());
    let _ = w.add_event_listener_with_callback("keyup", keyup.as_ref().unchecked_ref());
    let _ = w.add_event_listener_with_callback("blur", blur.as_ref().unchecked_ref());
    let doc = w.document().unwrap();
    let vis = Closure::<dyn FnMut()>::new(move || {
        let hidden = window().document().map(|d| d.hidden()).unwrap_or(false);
        with(|c| {
            if hidden {
                c.release_all();
            } else {
                // 다시 보이면 최신 상태를 한 번 그린다
                c.own_dirty = true;
                c.opp_dirty = true;
                c.request(true);
            }
        });
    });
    let _ = doc.add_event_listener_with_callback("visibilitychange", vis.as_ref().unchecked_ref());
    keydown.forget();
    keyup.forget();
    blur.forget();
    vis.forget();
}

/// 자동 측정 스크립트용: window.__bsPerfDump() → JSON 문자열, window.__bsPerfReset()
fn expose_perf_api() {
    let dump = Closure::<dyn FnMut() -> JsValue>::new(move || {
        with(|c| {
            let p = &c.perf;
            JsValue::from_str(&format!(
                "{{\"input\":{},\"frame\":{},\"board\":{},\"opp\":{},\"rtt\":{:.2},\"renders\":{},\"cells\":{},\"wasm_mem\":{}}}",
                p.input.json(),
                p.frame.json(),
                p.board.json(),
                p.opp.json(),
                p.rtt,
                p.total_renders,
                p.total_cells,
                wasm_memory_bytes()
            ))
        })
        .unwrap_or(JsValue::NULL)
    });
    let reset = Closure::<dyn FnMut()>::new(move || {
        with(|c| {
            let rtt = c.perf.rtt;
            c.perf = Perf::new();
            c.perf.rtt = rtt;
        });
    });
    let w = window();
    let _ = js_sys::Reflect::set(&w, &"__bsPerfDump".into(), dump.as_ref());
    let _ = js_sys::Reflect::set(&w, &"__bsPerfReset".into(), reset.as_ref());
    dump.forget();
    reset.forget();
}

// ───────────────────────── 공개 API (UI 에서 호출) ─────────────────────────

pub fn send(msg: ClientMsg) {
    with(|c| c.send(&msg));
}

pub fn apply_settings(s: Settings) {
    s.save();
    with(|c| {
        let rebuild = c.settings.renderer != s.renderer || c.settings.cell != s.cell || c.settings.theme != s.theme;
        c.settings = s;
        if rebuild {
            c.own = None;
            c.previews.iter_mut().for_each(|p| *p = None);
            c.opps.values_mut().for_each(|o| {
                o.board = None;
                o.dirty = true;
            });
        }
        c.own_dirty = true;
        c.opp_dirty = true;
        c.request(true);
    });
}

pub fn join_room(code: &str) {
    let code: String = code.chars().filter(|c| c.is_ascii_alphanumeric()).take(8).collect::<String>().to_ascii_uppercase();
    with(|c| {
        c.want_room = code;
        c.reconnect_now();
    });
}

pub fn reconnect() {
    with(|c| c.reconnect_now());
}

// ───────────────────────── 구현 ─────────────────────────

impl Core {
    fn log(&self, text: impl Into<String>) {
        let text = text.into();
        self.ui.log.update(|l| {
            if l.len() >= 100 {
                l.pop_front();
            }
            l.push_back((clock(), text));
        });
    }

    fn send(&self, msg: &ClientMsg) {
        if let Some(ws) = &self.ws {
            if ws.ready_state() == WebSocket::OPEN {
                let _ = ws.send_with_u8_array(&encode(msg));
            }
        }
    }

    fn reconnect_now(&mut self) {
        if let Some(ws) = self.ws.take() {
            self.closing = true;
            ws.set_onclose(None);
            let _ = ws.close();
        }
        self.reconnect_ms = 1000;
        self.connect();
    }

    fn connect(&mut self) {
        let loc = window().location();
        let proto = if loc.protocol().unwrap_or_default() == "https:" { "wss" } else { "ws" };
        let host = loc.host().unwrap_or_default();
        let Ok(ws) = WebSocket::new(&format!("{proto}://{host}/ws")) else {
            self.ui.conn.set(Conn::Closed);
            return;
        };
        ws.set_binary_type(BinaryType::Arraybuffer);
        self.closing = false;
        self.ui.conn.set(Conn::Connecting);
        self.ui.status.set("서버에 연결하는 중".into());

        let onopen = Closure::<dyn FnMut()>::new(move || {
            with(|c| {
                c.ui.conn.set(Conn::Open);
                c.reconnect_ms = 1000;
                let mut name = c.settings.name.trim().to_string();
                if name.is_empty() {
                    name = format!("사용자{:02}", (js_sys::Math::random() * 100.0) as u32);
                }
                c.send(&ClientMsg::Join { room: c.want_room.clone(), name });
            });
        });
        let onmsg = Closure::<dyn FnMut(MessageEvent)>::new(move |e: MessageEvent| {
            let bytes = js_sys::Uint8Array::new(&e.data()).to_vec();
            with(|c| {
                c.perf.msgs += 1;
                c.perf.bytes_in += bytes.len() as u64;
                if let Some(m) = decode::<ServerMsg>(&bytes) {
                    c.on_server(m);
                }
            });
        });
        let onclose = Closure::<dyn FnMut()>::new(move || {
            with(|c| c.on_closed());
        });
        ws.set_onopen(Some(onopen.as_ref().unchecked_ref()));
        ws.set_onmessage(Some(onmsg.as_ref().unchecked_ref()));
        ws.set_onclose(Some(onclose.as_ref().unchecked_ref()));
        onopen.forget();
        onmsg.forget();
        onclose.forget();
        self.ws = Some(ws);
    }

    fn on_closed(&mut self) {
        self.ws = None;
        self.ui.conn.set(Conn::Closed);
        self.phase = Phase::Lobby;
        self.ui.phase.set(Phase::Lobby);
        if self.closing {
            return;
        }
        let delay = self.reconnect_ms;
        self.reconnect_ms = (self.reconnect_ms * 2).min(15_000);
        self.ui.status.set(format!("연결 끊김 — {}초 후 재연결", delay / 1000));
        self.log("서버 연결이 끊어졌습니다");
        with_cb(|cb| {
            let _ = window().set_timeout_with_callback_and_timeout_and_arguments_0(cb.reconnect.as_ref().unchecked_ref(), delay);
        });
    }

    fn on_server(&mut self, msg: ServerMsg) {
        match msg {
            ServerMsg::Welcome { you, room } => {
                // 다른 방으로 옮기면 이전 방 채팅은 지운다 (새 방의 최근 기록이 곧 도착)
                if self.ui.room.get_untracked() != room {
                    self.ui.chat.set(VecDeque::new());
                }
                self.me = you;
                self.ui.me.set(you);
                self.want_room = room.clone();
                self.ui.room.set(room.clone());
                self.ui.status.set("대기 중 — 방장이 시작하면 게임이 시작됩니다".into());
                // 공유 가능한 URL 로 변경 (페이지 이동 없음)
                if let Ok(h) = window().history() {
                    let _ = h.replace_state_with_url(&JsValue::NULL, "", Some(&format!("?room={room}")));
                }
            }
            ServerMsg::Lobby { phase, host, players } => {
                self.phase = phase;
                self.ui.phase.set(phase);
                self.ui.host.set(host);
                self.opps.retain(|id, _| players.iter().any(|p| p.id == *id));
                self.ui.players.set(players);
            }
            ServerMsg::Started { seed } => {
                let g = Game::new(seed);
                self.server_game = Some(g.clone());
                self.predicted = Some(g);
                self.pending.clear();
                self.seq = 0;
                self.last_received = 0;
                self.ui.ranking.set(Vec::new());
                self.own_dirty = true;
                self.request(true);
            }
            ServerMsg::Countdown { seconds } => {
                self.ui.countdown.set(seconds);
                if seconds > 0 {
                    self.ui.status.set(format!("시작까지 {seconds}초"));
                } else {
                    self.phase = Phase::Playing;
                    self.ui.status.set("진행 중".into());
                }
            }
            ServerMsg::YourState { ack, game } => {
                if game.stats.received > self.last_received && self.settings.sound {
                    sound::play(Sfx::Garbage);
                }
                self.last_received = game.stats.received;
                let was_alive = self.server_game.as_ref().is_some_and(|g| !g.dead);
                if was_alive && game.dead {
                    self.ui.status.set("탈락 — 다른 플레이어의 게임판을 관전 중".into());
                    if self.settings.sound {
                        sound::play(Sfx::GameOver);
                    }
                }
                while self.pending.front().is_some_and(|p| p.0 <= ack) {
                    self.pending.pop_front();
                }
                let mut p = game.clone();
                for (_, i) in &self.pending {
                    p.apply_input(*i);
                }
                self.server_game = Some(game);
                if self.predicted.as_ref() != Some(&p) {
                    self.predicted = Some(p);
                    self.own_dirty = true;
                    self.request(false);
                }
            }
            ServerMsg::Boards(list) => {
                for (id, packed) in list {
                    if id == self.me {
                        continue;
                    }
                    let o = self.opps.entry(id).or_insert_with(|| Opp { cells: vec![0; VISIBLE_CELLS], dirty: true, board: None });
                    unpack_cells(&packed, &mut o.cells);
                    o.dirty = true;
                }
                self.opp_dirty = true;
                self.request(false);
            }
            ServerMsg::Scores(s) => self.ui.scores.set(s),
            ServerMsg::Log(t) => self.log(t),
            ServerMsg::GameOver { ranking } => {
                let mine = ranking.iter().position(|&id| id == self.me).map(|p| p + 1);
                self.ui.status.set(match mine {
                    Some(1) if ranking.len() > 1 => "게임 종료 — 1위".into(),
                    Some(n) => format!("게임 종료 — {n}위 / {}명", ranking.len()),
                    None => "게임 종료".into(),
                });
                self.ui.ranking.set(ranking);
                self.phase = Phase::Lobby;
                self.release_all();
                if self.settings.sound {
                    sound::play(Sfx::GameOver);
                }
            }
            ServerMsg::Pong { t } => {
                self.perf.rtt = (now() as u32).wrapping_sub(t) as f64;
            }
            ServerMsg::Chat(lines) => {
                self.ui.chat.update(|c| {
                    for l in lines {
                        if c.len() >= 100 {
                            c.pop_front();
                        }
                        c.push_back(l);
                    }
                });
            }
            ServerMsg::Rooms(list) => {
                self.ui.rooms.set(list);
                self.ui.rooms_loaded.set(true);
            }
            ServerMsg::Error(e) => {
                self.ui.status.set(format!("오류: {e}"));
                self.log(format!("오류: {e}"));
            }
        }
    }

    // ── 입력 ──

    fn on_keydown(&mut self, e: &KeyboardEvent) {
        if let Some(t) = e.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()) {
            let tag = t.tag_name();
            if tag == "INPUT" || tag == "SELECT" || tag == "TEXTAREA" {
                return;
            }
        }
        let code = e.code();
        // Enter: 채팅 입력창으로 이동 (게임 키에는 기본으로 쓰이지 않음)
        if (code == "Enter" || code == "NumpadEnter") && self.ui.rebinding.get_untracked().is_none() {
            if let Some(el) = window().document().and_then(|d| d.get_element_by_id("chat-input")) {
                if let Ok(input) = el.dyn_into::<web_sys::HtmlElement>() {
                    e.prevent_default();
                    self.release_all();
                    let _ = input.focus();
                    return;
                }
            }
        }
        if let Some(i) = self.ui.rebinding.get_untracked() {
            e.prevent_default();
            if code != "Escape" {
                self.settings.keys[i] = code;
                let s = self.settings.clone();
                s.save();
                self.ui.settings.set(s);
            }
            self.ui.rebinding.set(None);
            return;
        }
        let Some(input) = self.settings.action_for_code(&code) else { return };
        e.prevent_default();
        if e.repeat() {
            return; // 브라우저 자동 반복 대신 자체 DAS/ARR 사용
        }
        let t0 = e.time_stamp();
        self.local_input(input, t0);
        if let Some(h) = held_index(input) {
            // 반대 방향 이동 중이면 멈춤 (마지막 입력 우선)
            if h < 2 {
                self.stop_held(1 - h);
            }
            self.stop_held(h);
            let delay = if h == 2 { self.settings.sdr.max(16) } else { self.settings.das };
            with_cb(|cb| {
                let id = window()
                    .set_timeout_with_callback_and_timeout_and_arguments_0(cb.das[h].as_ref().unchecked_ref(), delay as i32)
                    .ok();
                self.held[h] = id;
            });
        }
    }

    fn on_keyup(&mut self, e: &KeyboardEvent) {
        if let Some(input) = self.settings.action_for_code(&e.code()) {
            if let Some(h) = held_index(input) {
                self.stop_held(h);
            }
        }
    }

    fn on_das(&mut self, h: usize) {
        let input = HELD_INPUTS[h];
        let rate = if h == 2 { self.settings.sdr } else { self.settings.arr };
        if rate == 0 {
            // ARR 0: 벽까지 즉시 이동
            for _ in 0..WIDTH {
                self.local_input(input, now());
            }
            self.held[h] = None;
            return;
        }
        self.local_input(input, now());
        with_cb(|cb| {
            let id = window()
                .set_interval_with_callback_and_timeout_and_arguments_0(cb.rep[h].as_ref().unchecked_ref(), rate.max(10) as i32)
                .ok();
            self.held[h] = id;
        });
    }

    fn stop_held(&mut self, h: usize) {
        if let Some(id) = self.held[h].take() {
            window().clear_timeout_with_handle(id);
            window().clear_interval_with_handle(id);
        }
    }

    fn release_all(&mut self) {
        for h in 0..3 {
            self.stop_held(h);
        }
    }

    /// 로컬 입력: 즉시 예측 상태에 반영하고 서버로 비동기 전송 (응답을 기다리지 않음)
    fn local_input(&mut self, input: Input, t0: f64) {
        if self.phase != Phase::Playing {
            return;
        }
        let Some(g) = self.predicted.as_mut() else { return };
        if g.dead || self.pending.len() > 96 {
            return;
        }
        let before = g.rev;
        let ev = g.apply_input(input);
        let changed = g.rev != before;
        if self.settings.sound && ev.locked {
            sound::play(if ev.cleared > 0 { Sfx::Clear } else { Sfx::Lock });
        }
        self.seq = self.seq.wrapping_add(1);
        if self.bench.is_none() {
            self.pending.push_back((self.seq, input));
            self.send(&ClientMsg::Input { seq: self.seq, input });
        }
        if changed {
            // 입력은 rAF 를 기다리지 않고 핸들러 안에서 바로 DOM 에 반영한다.
            // className 쓰기만 하고 레이아웃을 읽지 않으므로 강제 동기 레이아웃이 없다.
            // 실제 페인트는 브라우저의 다음 프레임에서 한 번에 처리된다.
            self.own_dirty = true;
            self.perf.renders += 1;
            self.perf.total_renders += 1;
            self.render_own();
            self.perf.input.push(now() - t0);
            self.frame_marks.push(t0);
            self.request_raf(); // 다음 프레임 시작 시각 측정용 (1회)
        }
    }

    // ── 렌더 스케줄링 ──

    fn request_raf(&mut self) {
        if self.raf_pending {
            return;
        }
        self.raf_pending = true;
        with_cb(|cb| {
            let _ = window().request_animation_frame(cb.raf.as_ref().unchecked_ref());
        });
    }

    fn request_timer(&mut self, ms: f64) {
        if self.timer_pending || self.raf_pending {
            return;
        }
        self.timer_pending = true;
        with_cb(|cb| {
            let _ = window().set_timeout_with_callback_and_timeout_and_arguments_0(cb.timer.as_ref().unchecked_ref(), ms.ceil() as i32);
        });
    }

    fn frame_budget(&self) -> f64 {
        if self.settings.effective_fps60() {
            0.0
        } else {
            32.0
        }
    }

    fn opp_interval(&self) -> f64 {
        if self.settings.low_spec {
            250.0
        } else {
            100.0
        }
    }

    /// 렌더 예약. urgent=입력에 의한 갱신(상한 무시)
    fn request(&mut self, urgent: bool) {
        if urgent {
            self.request_raf();
            return;
        }
        if self.raf_pending {
            return;
        }
        let wait = self.last_render + self.frame_budget() - now();
        if wait > 1.0 {
            self.request_timer(wait);
        } else {
            self.request_raf();
        }
    }

    fn on_frame(&mut self) {
        self.raf_pending = false;
        let t = now();
        // 입력이 반영된 DOM 이 그려질 프레임의 시작 시각 (입력 → 화면 근사치)
        for m in self.frame_marks.drain(..) {
            self.perf.frame.push(t - m);
        }
        let opp_due = self.opp_dirty && t - self.last_opp_render >= self.opp_interval();
        if self.own_dirty || opp_due {
            self.perf.renders += 1;
            self.perf.total_renders += 1;
            self.last_render = t;
            self.render_own();
            if opp_due {
                self.render_opps(t);
            }
        }
        if self.opp_dirty && !self.raf_pending {
            let wait = self.last_opp_render + self.opp_interval() - now();
            self.request_timer(wait.max(1.0));
        }
    }

    fn ensure_board(slot: &mut Option<Board>, id: &str, kind: RendererKind, s: &Settings, spec: BoardSpec) -> bool {
        if slot.as_ref().is_some_and(|b| b.is_connected()) {
            return true;
        }
        let Some(el) = window().document().and_then(|d| d.get_element_by_id(id)) else {
            *slot = None;
            return false;
        };
        *slot = Some(Board::new(kind, s.theme, el, spec));
        true
    }

    fn render_own(&mut self) {
        let s = &self.settings;
        if self.own_dirty {
            self.own_dirty = false;
            let first = self.own.is_none();
            let spec = BoardSpec { w: WIDTH, h: VISIBLE, headers: s.theme != Theme::Arcade, grid: true, cell_px: s.cell };
            let ready = Self::ensure_board(&mut self.own, "my-board", s.renderer, s, spec);
            if ready && first && !self.board_marked {
                self.board_marked = true;
                if let Some(p) = window().performance() {
                    let _ = p.mark("bs-board-ready");
                }
            }
            if let Some(g) = &self.predicted {
                let t0 = now();
                g.render_visible(s.ghost, &mut self.buf);
                if ready {
                    self.perf.total_cells += self.own.as_mut().unwrap().update(&self.buf) as u64;
                }
                // 보관 / 다음 블록 미리보기
                let mut small = [0u8; 8];
                let hold_code = if g.hold_used { Some(8) } else { None };
                for i in 0..=PREVIEW {
                    let kind = if i == 0 { g.hold } else { g.next.get(i - 1).copied() };
                    preview_cells(kind, if i == 0 { hold_code } else { None }, &mut small);
                    let id = if i == 0 { "hold".to_string() } else { format!("next-{}", i - 1) };
                    let spec = BoardSpec { w: 4, h: 2, headers: false, grid: false, cell_px: (s.cell * 2 / 3).max(10) };
                    if Self::ensure_board(&mut self.previews[i], &id, s.renderer, s, spec) {
                        self.previews[i].as_mut().unwrap().update(&small);
                    }
                }
                // 이름 상자: 현재 블록 위치를 셀 주소로 표시
                if let Some(el) = window().document().and_then(|d| d.get_element_by_id("namebox")) {
                    let txt = match g.active_anchor() {
                        Some((x, y)) => format!("{}{}", (b'A' + x) as char, y + 1),
                        None => "—".into(),
                    };
                    if el.text_content().as_deref() != Some(txt.as_str()) {
                        el.set_text_content(Some(&txt));
                    }
                }
                self.perf.board.push(now() - t0);
            }
        }
    }

    fn render_opps(&mut self, t: f64) {
        {
            self.opp_dirty = false;
            self.last_opp_render = t;
            let t0 = now();
            let mut any = false;
            let s = self.settings.clone();
            let mini = opp_cell_px(&s, self.opps.len());
            if mini != self.opp_cell {
                // 상대 수가 바뀌어 크기가 달라지면 (예: 1:1 ↔ 다인) 축소판을 새로 만든다
                self.opp_cell = mini;
                self.opps.values_mut().for_each(|o| {
                    o.board = None;
                    o.dirty = true;
                });
            }
            for (id, o) in self.opps.iter_mut() {
                if !o.dirty {
                    continue;
                }
                let spec = BoardSpec { w: WIDTH, h: VISIBLE, headers: false, grid: false, cell_px: mini };
                if Self::ensure_board(&mut o.board, &format!("opp-{id}"), s.renderer, &s, spec) {
                    o.dirty = false;
                    any = true;
                    self.perf.total_cells += o.board.as_mut().unwrap().update(&o.cells) as u64;
                }
            }
            if any {
                self.perf.opp.push(now() - t0);
            }
        }
    }

    fn publish_perf(&mut self) {
        let p = &mut self.perf;
        let kb = p.bytes_in as f64 / 1024.0;
        let sum = PerfSummary {
            input_p50: p.input.pct(0.5),
            input_p95: p.input.pct(0.95),
            frame_p50: p.frame.pct(0.5),
            board_p50: p.board.pct(0.5),
            board_max: p.board.max(),
            opp_p50: p.opp.pct(0.5),
            rtt: p.rtt,
            renders: p.renders,
            msgs: p.msgs,
            kb_in: kb,
        };
        p.renders = 0;
        p.msgs = 0;
        p.bytes_in = 0;
        if self.ui.perf.get_untracked() != sum {
            self.ui.perf.set(sum);
        }
    }

    // ── 벤치마크 (서버 없이 렌더링 경로만 측정) ──

    /// 경과 시간 기준으로 밀린 tick 을 처리한다. CPU 가 느려 타이머가 늦게 와도
    /// 초당 작업량(60 tick, 입력 10회, 상대 판 10Hz)이 일정하게 유지된다.
    fn bench_step(&mut self) {
        let Some(b) = self.bench.as_ref() else { return };
        let due = ((now() - b.start) / (1000.0 / 60.0)) as u64;
        let mut n = 0;
        while self.bench.as_ref().is_some_and(|b| b.tick < due) && n < 30 {
            self.bench_tick();
            n += 1;
        }
        if let Some(b) = self.bench.as_mut() {
            b.tick = b.tick.max(due); // 30 tick 이상 밀리면 버린다
        }
    }

    fn bench_tick(&mut self) {
        let Some(b) = self.bench.as_mut() else { return };
        b.tick += 1;
        // 내 게임: 서버 tick 대신 로컬 tick (60Hz 가정, 16ms 간격 호출)
        let mut own_changed = false;
        let mut input = None;
        if let Some(g) = self.predicted.as_mut() {
            if g.dead {
                *g = Game::new(b.tick);
                b.own_plan.clear();
            }
            let before = g.rev;
            g.tick();
            own_changed = g.rev != before;
            // 약 100ms 마다 사람처럼 한 번 입력
            if b.tick % 6 == 0 {
                if b.own_plan.is_empty() {
                    b.own_plan = bot::plan(g).into();
                }
                input = b.own_plan.pop_front();
            }
        }
        // 상대: 10Hz 로 게임판 갱신 (서버 Boards 메시지와 같은 주기)
        if b.tick % 6 == 0 {
            for (i, g) in b.opps.iter_mut().enumerate() {
                if g.dead {
                    *g = Game::new(b.tick + i as u64);
                    b.plans[i].clear();
                }
                for _ in 0..6 {
                    g.tick();
                }
                if b.plans[i].is_empty() {
                    b.plans[i] = bot::plan(g).into();
                }
                if let Some(inp) = b.plans[i].pop_front() {
                    g.apply_input(inp);
                }
                let id = (i + 2) as u8;
                let o = self.opps.entry(id).or_insert_with(|| Opp { cells: vec![0; VISIBLE_CELLS], dirty: true, board: None });
                g.render_visible(false, &mut o.cells);
                o.dirty = true;
            }
            self.opp_dirty = true;
        }
        if own_changed {
            self.own_dirty = true;
            self.request(false);
        }
        if let Some(i) = input {
            self.local_input(i, now());
        }
        if self.opp_dirty {
            self.request(false);
        }
    }
}

/// 상대 게임판 셀 크기. 아케이드 모드 1:1 대전은 상대 판을 크게 보여준다 (style.css 의 --mcell 과 동일 규칙)
pub fn opp_cell_px(s: &Settings, opponents: usize) -> u8 {
    if s.low_spec {
        6
    } else if s.theme == Theme::Arcade {
        if opponents <= 1 {
            18
        } else {
            12
        }
    } else {
        8
    }
}

fn wasm_memory_bytes() -> f64 {
    wasm_bindgen::memory()
        .dyn_into::<js_sys::WebAssembly::Memory>()
        .ok()
        .and_then(|m| m.buffer().dyn_into::<js_sys::ArrayBuffer>().ok())
        .map(|b| b.byte_length() as f64)
        .unwrap_or(0.0)
}

fn preview_cells(kind: Option<Kind>, force_code: Option<u8>, out: &mut [u8; 8]) {
    out.fill(0);
    let Some(k) = kind else { return };
    let cells = k.cells(0);
    let min_y = cells.iter().map(|c| c.1).min().unwrap_or(0);
    let shift_x = if k == Kind::O { 1 } else { 0 };
    for (x, y) in cells {
        let (x, y) = ((x + shift_x) as usize, (y - min_y) as usize);
        if x < 4 && y < 2 {
            out[y * 4 + x] = force_code.unwrap_or(k.code());
        }
    }
}

fn start_bench(kind: RendererKind, opps: usize, low: bool) {
    with(|c| {
        let mut s = c.settings.clone();
        s.renderer = kind;
        s.low_spec = low;
        c.settings = s.clone();
        c.ui.settings.set(s);
        c.ui.bench.set(true);
        c.ui.conn.set(Conn::Offline);
        c.ui.room.set("BENCH".into());
        c.me = 1;
        c.ui.me.set(1);
        c.phase = Phase::Playing;
        c.ui.phase.set(Phase::Playing);
        c.ui.status.set(format!("렌더링 벤치마크 — {} / 상대 {}명", if kind == RendererKind::Dom { "DOM 그리드" } else { "Canvas 2D" }, opps));
        let mut players = vec![PlayerInfo { id: 1, name: "나".into(), bot: false, wins: 0, in_game: true }];
        for i in 0..opps {
            players.push(PlayerInfo { id: (i + 2) as u8, name: format!("상대{}", i + 1), bot: true, wins: 0, in_game: true });
        }
        c.ui.players.set(players);
        c.predicted = Some(Game::new(1));
        c.bench = Some(BenchState {
            opps: (0..opps).map(|i| Game::new(100 + i as u64)).collect(),
            plans: vec![VecDeque::new(); opps],
            own_plan: VecDeque::new(),
            tick: 0,
            start: now(),
        });
        c.own_dirty = true;
        c.request(true);
    });
    let step = Closure::<dyn FnMut()>::new(move || {
        with(|c| c.bench_step());
    });
    let _ = window().set_interval_with_callback_and_timeout_and_arguments_0(step.as_ref().unchecked_ref(), 16);
    step.forget();
}
