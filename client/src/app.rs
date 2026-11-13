//! Spreadsheet Mode UI (Leptos).
//!
//! 게임판 셀은 여기서 그리지 않는다. 컨테이너(div id="my-board" 등)만 만들고
//! 실제 셀은 core.rs 의 렌더러가 직접 관리한다. 따라서 게임판 변경이
//! Leptos 반응형 갱신이나 페이지 재렌더링으로 이어지지 않는다.

use crate::runtime::{self as core, Conn, Ui};
use crate::settings::{key_label, RendererKind, Settings, Theme, ACTIONS};
use bs_engine::protocol::{ClientMsg, Phase, PlayerInfo, PlayerStat, MAX_PLAYERS};
use leptos::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tab {
    Game,
    Settings,
    Help,
    Log,
    Rooms,
}

#[component]
pub fn App() -> impl IntoView {
    let ui = Ui::new();
    let tab = RwSignal::new(Tab::Game);

    // 테마/저사양/애니메이션/셀 크기 → body class 와 CSS 변수 (설정 변경 시에만 실행)
    Effect::new(move |_| {
        let s = ui.settings.get();
        if let Some(body) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.body()) {
            let mut cls = String::from(match s.theme {
                Theme::HighContrast => "theme-hc",
                Theme::Arcade => "theme-arcade",
                Theme::Sheet => "theme-sheet",
            });
            if s.low_spec {
                cls.push_str(" low");
            }
            if s.effective_animations() {
                cls.push_str(" anim");
            }
            body.set_class_name(&cls);
            let _ = body.style().set_property("--cell", &format!("{}px", s.cell));
            let _ = body.style().set_property("--pcell", &format!("{}px", (s.cell as u32 * 2 / 3).max(10)));
            let mini = if s.low_spec { "6px" } else if s.theme == Theme::Arcade { "11px" } else { "8px" };
            let _ = body.style().set_property("--mcell", mini);
        }
    });

    core::init(ui);

    // 방 목록 탭을 열 때마다 서버에 최신 목록 요청
    Effect::new(move |_| {
        if tab.get() == Tab::Rooms {
            core::send(ClientMsg::ListRooms);
        }
    });

    // 테마에 따라 화면 구조 전체를 바꾼다. 게임판 컨테이너는 같은 id 를 쓰므로
    // 렌더러가 다음 프레임에 새 컨테이너를 찾아 다시 그린다 (runtime::ensure_board).
    let arcade = move || ui.settings.with(|s| s.theme == Theme::Arcade);
    view! {
        <Show when=arcade fallback=move || view! { <SheetShell ui=ui tab=tab/> }>
            <crate::arcade::ArcadeShell ui=ui tab=tab/>
        </Show>
    }
}

#[component]
fn SheetShell(ui: Ui, tab: RwSignal<Tab>) -> impl IntoView {
    view! {
        <div class="app">
            <TitleBar ui=ui/>
            <MenuBar ui=ui tab=tab/>
            <FormulaBar ui=ui/>
            <main class="workspace">
                <section class="sheet-area">
                    <div class:hidden=move || tab.get() != Tab::Game>
                        <GameSheet ui=ui/>
                    </div>
                    <div class:hidden=move || tab.get() != Tab::Settings>
                        <SettingsSheet ui=ui/>
                    </div>
                    <div class:hidden=move || tab.get() != Tab::Help>
                        <HelpSheet ui=ui/>
                    </div>
                    <div class:hidden=move || tab.get() != Tab::Rooms>
                        <RoomsSheet ui=ui tab=tab/>
                    </div>
                    <div class:hidden=move || tab.get() != Tab::Log>
                        <LogSheet ui=ui/>
                    </div>
                </section>
                <PlayerPanel ui=ui/>
            </main>
            <BottomBar ui=ui tab=tab/>
        </div>
    }
}

#[component]
fn TitleBar(ui: Ui) -> impl IntoView {
    let conn_text = move || match ui.conn.get() {
        Conn::Open => "연결됨",
        Conn::Connecting => "연결 중…",
        Conn::Closed => "연결 끊김",
        Conn::Offline => "오프라인",
    };
    let conn_class = move || match ui.conn.get() {
        Conn::Open => "conn ok",
        Conn::Connecting => "conn wait",
        _ => "conn bad",
    };
    let copy = move |_| {
        if let Some(w) = web_sys::window() {
            let url = w.location().href().unwrap_or_default();
            let _ = w.navigator().clipboard().write_text(&url);
        }
    };
    view! {
        <header class="titlebar">
            <span class="doc-mark">"▦"</span>
            <span class="doc-title">
                {move || if ui.settings.get().theme == Theme::Arcade {
                    format!("BLOCK STACK · ROOM {}", ui.room.get())
                } else {
                    format!("블록_집계표_{}.sheet", ui.room.get())
                }}
            </span>
            <span class="spacer"></span>
            <span class="roomcode">"방 코드 "<b>{move || ui.room.get()}</b></span>
            <button class="link" on:click=copy title="초대 링크 복사">"링크 복사"</button>
            <span class=conn_class>{conn_text}</span>
        </header>
    }
}

#[component]
fn MenuBar(ui: Ui, tab: RwSignal<Tab>) -> impl IntoView {
    let code = RwSignal::new(String::new());
    let is_host = move || ui.host.get() == ui.me.get() && ui.me.get() != 0;
    let lobby = move || ui.phase.get() == Phase::Lobby;
    let can_start = move || is_host() && lobby() && ui.conn.get() == Conn::Open;
    view! {
        <nav class="menubar">
            <button on:click=move |_| core::join_room("")>"새 방"</button>
            <span class="join">
                <input
                    class="code-input"
                    placeholder="방 코드"
                    maxlength="8"
                    prop:value=move || code.get()
                    on:input=move |ev| code.set(event_target_value(&ev))
                />
                <button on:click=move |_| core::join_room(&code.get_untracked())>"참가"</button>
            </span>
            <span class="sep"></span>
            <button class="primary" disabled=move || !can_start() on:click=move |_| core::send(ClientMsg::Start)>
                "게임 시작"
            </button>
            <button disabled=move || !(is_host() && lobby()) on:click=move |_| core::send(ClientMsg::AddBot)>
                "연습봇 추가"
            </button>
            <button disabled=move || !(is_host() && lobby()) on:click=move |_| core::send(ClientMsg::RemoveBot)>
                "연습봇 제거"
            </button>
            <span class="sep"></span>
            <button
                title="Spreadsheet Mode / 아케이드 모드 전환"
                on:click=move |_| {
                    let mut s = ui.settings.get_untracked();
                    s.theme = if s.theme == Theme::Arcade { Theme::Sheet } else { Theme::Arcade };
                    ui.settings.set(s.clone());
                    core::apply_settings(s);
                }
            >
                {move || if ui.settings.get().theme == Theme::Arcade { "업무 화면" } else { "아케이드 모드" }}
            </button>
            <button on:click=move |_| tab.set(Tab::Settings)>"설정"</button>
            <button on:click=move |_| tab.set(Tab::Rooms)>"방 목록"</button>
            <button on:click=move |_| tab.set(Tab::Help)>"도움말"</button>
            <span class="spacer"></span>
            <span class="muted">
                {move || if is_host() { "방장" } else if ui.me.get() != 0 { "참가자" } else { "" }}
            </span>
        </nav>
    }
}

#[component]
fn FormulaBar(ui: Ui) -> impl IntoView {
    let formula = move || {
        let p = ui.phase.get();
        let st = ui.status.get();
        let f = match p {
            Phase::Lobby => "대기",
            Phase::Countdown => "카운트다운",
            Phase::Playing => "진행",
        };
        format!("={f}(\"{st}\")")
    };
    view! {
        <div class="formulabar">
            <div id="namebox" class="namebox">"A1"</div>
            <div class="fx">"fx"</div>
            <div class="formula">{formula}</div>
        </div>
    }
}

#[component]
fn GameSheet(ui: Ui) -> impl IntoView {
    let my_stat = move || ui.scores.get().into_iter().find(|s| s.id == ui.me.get()).unwrap_or_default();
    let opponents = move || {
        let me = ui.me.get();
        ui.players.get().into_iter().filter(|p| p.id != me).collect::<Vec<PlayerInfo>>()
    };
    let alive_of = move |id: u8| ui.scores.get().iter().find(|s| s.id == id).map(|s| s.alive).unwrap_or(true);
    view! {
        <div class="play">
            <div class="board-wrap">
                <div class="board-row">
                    // 받을 공격 게이지 (아케이드 모드에서만 표시). 2Hz 점수표 갱신 때만 높이가 바뀐다
                    <div class="meter" title="받을 공격">
                        <div class="meter-fill" style:height=move || format!("{}%", (my_stat().pending * 5).min(100))></div>
                    </div>
                    <div id="my-board" class="board"></div>
                </div>
                <Show when=move || ui.phase.get() == Phase::Countdown>
                    <div class="note">"시작까지 " {move || ui.countdown.get()} "초"</div>
                </Show>
            </div>
            <div class="side-col">
                <table class="grid-table">
                    <tr><th>"보관"</th></tr>
                    <tr><td><div id="hold" class="preview"></div></td></tr>
                    <tr><th>"다음"</th></tr>
                    <tr><td><div id="next-0" class="preview"></div></td></tr>
                    <tr><td><div id="next-1" class="preview"></div></td></tr>
                    <tr><td><div id="next-2" class="preview"></div></td></tr>
                </table>
                <table class="grid-table kv">
                    <tr><th>"레벨"</th><td class="num">{move || my_stat().level}</td></tr>
                    <tr><th>"줄"</th><td class="num">{move || my_stat().lines}</td></tr>
                    <tr><th>"점수"</th><td class="num">{move || my_stat().score}</td></tr>
                    <tr>
                        <th>"받을 공격"</th>
                        <td class="num" class:warn=move || { my_stat().pending > 0 }>{move || my_stat().pending}</td>
                    </tr>
                </table>
            </div>
            <div class="opps">
                <For
                    each=opponents
                    key=|p: &PlayerInfo| p.id
                    children=move |p: PlayerInfo| {
                        let id = p.id;
                        view! {
                            <figure class="opp" class:out=move || !alive_of(id)>
                                <figcaption>{p.name.clone()}</figcaption>
                                <div id=format!("opp-{id}") class="mini"></div>
                            </figure>
                        }
                    }
                />
            </div>
        </div>
    }
}

#[component]
fn PlayerPanel(ui: Ui) -> impl IntoView {
    let rows = move || {
        let players = ui.players.get();
        let scores = ui.scores.get();
        let ranking = ui.ranking.get();
        let me = ui.me.get();
        let host = ui.host.get();
        let name_of = |id: u8| players.iter().find(|p| p.id == id).map(|p| p.name.clone()).unwrap_or_else(|| "-".into());
        let mut list: Vec<(PlayerInfo, PlayerStat)> = players
            .iter()
            .map(|p| (p.clone(), scores.iter().find(|s| s.id == p.id).cloned().unwrap_or(PlayerStat { id: p.id, ..Default::default() })))
            .collect();
        // 순위: 게임 종료 결과 > 생존 여부 > 점수
        list.sort_by_key(|(p, s)| {
            let r = ranking.iter().position(|&id| id == p.id).unwrap_or(99);
            (r, !s.alive, std::cmp::Reverse(s.score))
        });
        list.into_iter()
            .enumerate()
            .map(|(i, (p, s))| {
                let place = if s.place > 0 { s.place.to_string() } else { (i + 1).to_string() };
                let state = if !p.in_game && ranking.is_empty() {
                    "대기"
                } else if s.alive {
                    "생존"
                } else {
                    "탈락"
                };
                let target = s.target.map(|t| name_of(t)).unwrap_or_default();
                let mut name = p.name.clone();
                if p.id == host {
                    name.push_str(" ★");
                }
                view! {
                    <tr class:me={p.id == me} class:dead={!s.alive && p.in_game}>
                        <td class="rh">{i + 1}</td>
                        <td class="num">{place}</td>
                        <td>{name}{if p.bot { " (봇)" } else { "" }}</td>
                        <td>{state}</td>
                        <td class="num">{s.score}</td>
                        <td class="num">{s.lines}</td>
                        <td class="num">{s.sent}</td>
                        <td class="num">{s.received}</td>
                        <td class="num">{s.cancelled}</td>
                        <td class="num" class:warn={s.pending > 0}>{s.pending}</td>
                        <td>{target}</td>
                        <td class="num">{p.wins}</td>
                    </tr>
                }
            })
            .collect_view()
    };
    view! {
        <aside class="panel">
            <table class="grid-table players">
                <thead>
                    <tr class="colhead">
                        <th class="rh"></th><th>"A"</th><th>"B"</th><th>"C"</th><th>"D"</th><th>"E"</th><th>"F"</th>
                        <th>"G"</th><th>"H"</th><th>"I"</th><th>"J"</th><th>"K"</th><th>"L"</th>
                    </tr>
                    <tr>
                        <th class="rh"></th><th>"순위"</th><th>"이름"</th><th>"상태"</th><th>"점수"</th><th>"줄"</th>
                        <th>"공격"</th><th>"받음"</th><th>"상쇄"</th><th>"대기"</th><th>"대상"</th><th>"승"</th>
                    </tr>
                </thead>
                <tbody>{rows}</tbody>
            </table>
            <p class="muted small">"공격: 보낸 줄 · 받음: 실제로 올라온 줄 · 상쇄: 줄 삭제로 막아낸 줄 · 대기: 다음 고정 때 올라올 줄"</p>
            <ChatPanel ui=ui/>
        </aside>
    }
}

#[component]
pub(crate) fn SettingsSheet(ui: Ui) -> impl IntoView {
    let set = move |f: &dyn Fn(&mut Settings)| {
        let mut s = ui.settings.get_untracked();
        f(&mut s);
        ui.settings.set(s.clone());
        core::apply_settings(s);
    };
    let s = move || ui.settings.get();
    let check = move |label: &'static str, get: fn(&Settings) -> bool, put: fn(&mut Settings, bool), note: &'static str| {
        view! {
            <tr>
                <th>{label}</th>
                <td>
                    <input type="checkbox" prop:checked=move || get(&s()) on:change=move |ev| {
                        let v = event_target_checked(&ev);
                        set(&|x: &mut Settings| put(x, v));
                    }/>
                </td>
                <td class="muted">{note}</td>
            </tr>
        }
    };
    let num = move |label: &'static str, get: fn(&Settings) -> u16, put: fn(&mut Settings, u16), note: &'static str| {
        view! {
            <tr>
                <th>{label}</th>
                <td>
                    <input type="number" min="0" max="500" class="num-input" prop:value=move || get(&s()).to_string() on:change=move |ev| {
                        let v = event_target_value(&ev).parse().unwrap_or(0);
                        set(&|x: &mut Settings| put(x, v));
                    }/>
                </td>
                <td class="muted">{note}</td>
            </tr>
        }
    };
    view! {
        <div class="sheet-page">
            <table class="grid-table form">
                <tr class="colhead"><th>"항목"</th><th>"값"</th><th>"설명"</th></tr>
                <tr>
                    <th>"테마"</th>
                    <td>
                        <select on:change=move |ev| {
                            let t = match event_target_value(&ev).as_str() {
                                "hc" => Theme::HighContrast,
                                "arcade" => Theme::Arcade,
                                _ => Theme::Sheet,
                            };
                            set(&|x: &mut Settings| x.theme = t);
                        }>
                            <option value="sheet" selected=move || s().theme == Theme::Sheet>"Spreadsheet Mode (기본)"</option>
                            <option value="hc" selected=move || s().theme == Theme::HighContrast>"고대비 모드"</option>
                            <option value="arcade" selected=move || s().theme == Theme::Arcade>"아케이드 모드"</option>
                        </select>
                    </td>
                    <td class="muted">"고대비: 진한 색상과 검은 테두리 · 아케이드: 어두운 배경, 선명한 입체 블록, 공격 게이지"</td>
                </tr>
                {check("저사양 모드", |s| s.low_spec, |s, v| s.low_spec = v, "효과 끔, 30fps 고정, 상대 게임판 4Hz·축소")}
                {check("애니메이션", |s| s.animations, |s, v| s.animations = v, "HTML 셀 렌더러의 색 전환 효과 (기본 끔, 동작 줄이기 설정 시 항상 끔)")}
                {check("사운드", |s| s.sound, |s, v| s.sound = v, "짧은 효과음 (기본 끔, 배경음악 없음)")}
                {check("고스트 블록", |s| s.ghost, |s, v| s.ghost = v, "블록이 떨어질 위치 표시")}
                {check("60fps 렌더링", |s| s.fps60, |s, v| s.fps60 = v, "기본은 30fps. 입력 반응은 항상 즉시 반영")}
                <tr>
                    <th>"게임판 크기"</th>
                    <td>
                        <select on:change=move |ev| {
                            let v: u8 = event_target_value(&ev).parse().unwrap_or(22);
                            set(&|x: &mut Settings| x.cell = v);
                        }>
                            <option value="16" selected=move || s().cell == 16>"작게 (16px)"</option>
                            <option value="22" selected=move || s().cell == 22>"보통 (22px)"</option>
                            <option value="28" selected=move || s().cell == 28>"크게 (28px)"</option>
                        </select>
                    </td>
                    <td class="muted">"셀 한 칸의 크기"</td>
                </tr>
                <tr>
                    <th>"렌더러"</th>
                    <td>
                        <select on:change=move |ev| {
                            let c = event_target_value(&ev) == "canvas";
                            set(&|x: &mut Settings| x.renderer = if c { RendererKind::Canvas } else { RendererKind::Dom });
                        }>
                            <option value="canvas" selected=move || s().renderer == RendererKind::Canvas>"Canvas 2D (기본)"</option>
                            <option value="dom" selected=move || s().renderer == RendererKind::Dom>"HTML 셀 (CSS Grid)"</option>
                        </select>
                    </td>
                    <td class="muted">"둘 다 GPU 없이 동작. 소프트웨어 렌더링 측정에서 Canvas 2D 의 CPU 사용량이 약 30% 낮음"</td>
                </tr>
                {num("DAS (ms)", |s| s.das, |s, v| s.das = v.min(500), "좌우 키를 누르고 있을 때 반복 시작까지")}
                {num("ARR (ms)", |s| s.arr, |s, v| s.arr = v.min(200), "좌우 반복 간격 (0 = 벽까지 즉시)")}
                {num("내리기 반복 (ms)", |s| s.sdr, |s, v| s.sdr = v.min(200), "천천히 내리기 반복 간격")}
                <tr>
                    <th>"이름"</th>
                    <td>
                        <input class="name-input" maxlength="16" prop:value=move || s().name on:change=move |ev| {
                            let v = event_target_value(&ev);
                            set(&|x: &mut Settings| x.name = v.clone());
                        }/>
                    </td>
                    <td class="muted">"다음 접속부터 적용 "<button class="link" on:click=move |_| core::reconnect()>"지금 다시 접속"</button></td>
                </tr>
            </table>
            <h3 class="sheet-h">"키보드 조작"</h3>
            <table class="grid-table form">
                <tr class="colhead"><th>"동작"</th><th>"키"</th><th></th></tr>
                {ACTIONS
                    .iter()
                    .enumerate()
                    .map(|(i, (_, label))| {
                        view! {
                            <tr>
                                <th>{*label}</th>
                                <td class="key">
                                    {move || if ui.rebinding.get() == Some(i) { "키를 누르세요…".to_string() } else { key_label(&s().keys[i]) }}
                                </td>
                                <td><button class="link" on:click=move |_| ui.rebinding.set(Some(i))>"변경"</button></td>
                            </tr>
                        }
                    })
                    .collect_view()}
            </table>
            <p class="muted small">"X 키는 시계 방향 회전 보조키로도 동작합니다. 설정은 이 브라우저에만 저장됩니다."</p>
            <button on:click=move |_| set(&|x: &mut Settings| { let name = x.name.clone(); *x = Settings::default(); x.name = name; })>
                "기본값으로 되돌리기"
            </button>
        </div>
    }
}

#[component]
pub(crate) fn HelpSheet(ui: Ui) -> impl IntoView {
    let _ = ui;
    view! {
        <div class="sheet-page">
            <table class="grid-table form">
                <tr class="colhead"><th>"항목"</th><th>"내용"</th></tr>
                <tr><th>"목표"</th><td>"가로 한 줄을 채우면 줄이 지워집니다. 두 줄 이상 지우면 상대에게 방해줄을 보냅니다."</td></tr>
                <tr><th>"방어"</th><td>"받을 공격이 쌓여 있을 때 줄을 지우면 먼저 상쇄합니다. 상쇄하지 못한 줄은 다음 블록을 고정할 때 아래에서 올라옵니다."</td></tr>
                <tr><th>"공격량"</th><td>"2줄=1, 3줄=2, 4줄=4, 연속 삭제 보너스, 판 전체 비우기 +4"</td></tr>
                <tr><th>"승리"</th><td>"블록이 맨 위를 넘으면 탈락합니다. 마지막까지 남은 사람이 승리합니다 (2~6명)."</td></tr>
                <tr><th>"초대"</th><td>"상단의 '링크 복사'로 같은 방 주소를 공유하세요."</td></tr>
                <tr><th>"기본 조작"</th><td>"← → 이동 · ↓ 천천히 · Space 바로 내리기 · ↑/X 회전 · Z 반대 회전 · C 보관"</td></tr>
            </table>
        </div>
    }
}

/// 같은 방 채팅 (두 테마 공용). 메시지가 올 때만 다시 그린다.
#[component]
pub(crate) fn ChatPanel(ui: Ui) -> impl IntoView {
    let text = RwSignal::new(String::new());
    let list_ref = NodeRef::<leptos::html::Div>::new();
    // 새 메시지가 오면 맨 아래로 스크롤 (DOM 갱신 후 다음 프레임에 1회)
    Effect::new(move |_| {
        ui.chat.track();
        request_animation_frame(move || {
            if let Some(el) = list_ref.get_untracked() {
                el.set_scroll_top(el.scroll_height());
            }
        });
    });
    let submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        let t = text.get_untracked();
        if !t.trim().is_empty() {
            core::send(ClientMsg::Chat { text: t });
            text.set(String::new());
        }
    };
    let on_key = move |ev: leptos::ev::KeyboardEvent| {
        // Esc: 입력창을 빠져나와 바로 게임 조작으로
        if ev.key() == "Escape" {
            if let Some(el) = ev.target().and_then(|t| wasm_bindgen::JsCast::dyn_into::<web_sys::HtmlElement>(t).ok()) {
                let _ = el.blur();
            }
        }
    };
    view! {
        <div class="chat">
            <div class="chat-head">
                <span>"채팅"</span>
                <span class="muted small">"Enter 입력 · Esc 게임으로"</span>
            </div>
            <div class="chat-list" node_ref=list_ref>
                {move || {
                    let me = ui.me.get();
                    let lines = ui.chat.get();
                    if lines.is_empty() {
                        return view! { <div class="chat-empty muted">"아직 대화가 없습니다"</div> }.into_any();
                    }
                    lines
                        .iter()
                        .map(|l| {
                            view! {
                                <div class="chat-line" class:mine={l.from == me}>
                                    <b class="chat-name">{l.name.clone()}</b>
                                    <span class="chat-text">{l.text.clone()}</span>
                                </div>
                            }
                        })
                        .collect_view()
                        .into_any()
                }}
            </div>
            <form class="chat-form" on:submit=submit>
                <input
                    id="chat-input"
                    autocomplete="off"
                    maxlength="120"
                    placeholder="메시지 입력"
                    prop:value=move || text.get()
                    on:input=move |ev| text.set(event_target_value(&ev))
                    on:keydown=on_key
                />
                <button type="submit">"보내기"</button>
            </form>
        </div>
    }
}

/// 현재 열린 방 목록 (두 테마 공용)
#[component]
pub(crate) fn RoomsSheet(ui: Ui, tab: RwSignal<Tab>) -> impl IntoView {
    let refresh = move |_| core::send(ClientMsg::ListRooms);
    let rows = move || {
        let mine = ui.room.get();
        let list = ui.rooms.get();
        if list.is_empty() {
            let msg = if ui.rooms_loaded.get() { "열린 방이 없습니다" } else { "불러오는 중…" };
            return view! { <tr><td colspan="6" class="muted">{msg}</td></tr> }.into_any();
        }
        list.into_iter()
            .map(|r| {
                let code = r.code.clone();
                let here = r.code == mine;
                let full = r.players as usize >= MAX_PLAYERS;
                let (label, enabled) = if here {
                    ("참가 중", false)
                } else if full {
                    ("가득 참", false)
                } else {
                    ("참가", true)
                };
                let state = if r.playing { "게임 중 · 다음 판부터 참가" } else { "대기 중" };
                view! {
                    <tr class:me=here>
                        <td class="room-code">{r.code.clone()}</td>
                        <td>{r.host.clone()}</td>
                        <td class="num">{format!("{}/{}", r.players, MAX_PLAYERS)}</td>
                        <td class="muted">{format!("사람 {} · 봇 {}", r.humans, r.bots)}</td>
                        <td class:playing=r.playing>{state}</td>
                        <td>
                            <button class="primary" disabled=!enabled on:click=move |_| {
                                core::join_room(&code);
                                tab.set(Tab::Game);
                            }>{label}</button>
                        </td>
                    </tr>
                }
                .into_any()
            })
            .collect_view()
            .into_any()
    };
    view! {
        <div class="sheet-page rooms-page">
            <div class="rooms-head">
                <button on:click=refresh>"새로고침"</button>
                <span class="muted small">"사람이 있는 방만 표시합니다 (대기 중인 방 → 인원 많은 순, 최대 50개)"</span>
            </div>
            <table class="grid-table rooms">
                <tr class="colhead"><th>"방 코드"</th><th>"방장"</th><th>"인원"</th><th>"구성"</th><th>"상태"</th><th></th></tr>
                {rows}
            </table>
        </div>
    }
}

#[component]
pub(crate) fn LogSheet(ui: Ui) -> impl IntoView {
    view! {
        <div class="sheet-page">
            <table class="grid-table log">
                <tr class="colhead"><th>"시각"</th><th>"내용"</th></tr>
                {move || {
                    ui.log
                        .get()
                        .iter()
                        .rev()
                        .map(|(t, m)| view! { <tr><td class="num">{t.clone()}</td><td>{m.clone()}</td></tr> })
                        .collect_view()
                }}
            </table>
        </div>
    }
}

#[component]
fn BottomBar(ui: Ui, tab: RwSignal<Tab>) -> impl IntoView {
    let tab_btn = move |t: Tab, label: &'static str| {
        view! {
            <button class="sheet-tab" class:active=move || tab.get() == t on:click=move |_| tab.set(t)>
                {label}
            </button>
        }
    };
    let last_log = move || ui.log.with(|l| l.back().map(|(t, m)| format!("{t}  {m}")).unwrap_or_default());
    let perf = move || {
        let p = ui.perf.get();
        format!(
            "입력→화면 {:.1}ms (p95 {:.1}) · 판 갱신 {:.2}ms · RTT {:.0}ms · 렌더 {}/s · 수신 {:.1}KB/s",
            p.input_p50, p.input_p95, p.board_p50, p.rtt, p.renders, p.kb_in
        )
    };
    view! {
        <footer class="bottombar">
            <div class="sheet-tabs">
                {tab_btn(Tab::Game, "대전")}
                {tab_btn(Tab::Settings, "설정")}
                {tab_btn(Tab::Help, "도움말")}
                {tab_btn(Tab::Rooms, "방 목록")}
                {tab_btn(Tab::Log, "기록")}
            </div>
            <div class="statusbar">
                <span class="st-phase">
                    {move || match ui.phase.get() {
                        Phase::Lobby => "준비",
                        Phase::Countdown => "카운트다운",
                        Phase::Playing => "진행 중",
                    }}
                </span>
                <span class="st-log">{last_log}</span>
                <span class="spacer"></span>
                <span class="st-perf">{perf}</span>
            </div>
        </footer>
    }
}
