//! 아케이드 모드 화면 (독자 디자인).
//!
//! 블록 퍼즐 대전 게임 장르의 일반적인 배치를 따른다:
//! 가운데 큰 내 게임판, 왼쪽 HOLD + 점수판, 오른쪽 NEXT 5개, 판 옆 공격 게이지,
//! 이름표·순위 배지·KO 수, 1:1 이면 큰 상대 판 / 다인이면 상대 판 슬롯 5개,
//! 게임판 위 카운트다운·K.O.·결과 표시.
//!
//! 게임판 셀은 여기서도 그리지 않는다. 같은 id(my-board, hold, next-N, opp-N)의 컨테이너만 만들고
//! runtime.rs 의 렌더러가 바뀐 셀만 직접 그린다. 모든 표시는 정지 상태이며 애니메이션이 없다.

use crate::app::{ChatPanel, HelpSheet, LogSheet, RoomsSheet, SettingsSheet, Tab};
use crate::runtime::{self as core, Conn, Ui};
use crate::settings::Theme;
use bs_engine::game::PREVIEW;
use bs_engine::protocol::{ClientMsg, Phase, PlayerInfo, PlayerStat};
use leptos::prelude::*;

const MAX_OPPONENT_SLOTS: usize = 5;

fn stat_of(ui: Ui, id: u8) -> PlayerStat {
    ui.scores.with(|s| s.iter().find(|x| x.id == id).cloned()).unwrap_or(PlayerStat { id, alive: true, ..Default::default() })
}

/// 현재 순위: 게임 종료 결과 > 확정된 탈락 순위 > 생존자 점수 순
fn rank_of(ui: Ui, id: u8) -> Option<usize> {
    let ranking = ui.ranking.get();
    if let Some(i) = ranking.iter().position(|&r| r == id) {
        return Some(i + 1);
    }
    let scores = ui.scores.get();
    let me = scores.iter().find(|s| s.id == id)?;
    if me.place > 0 {
        return Some(me.place as usize);
    }
    let mut alive: Vec<&PlayerStat> = scores.iter().filter(|s| s.alive).collect();
    alive.sort_by_key(|s| std::cmp::Reverse(s.score));
    alive.iter().position(|s| s.id == id).map(|p| p + 1)
}

fn badge_class(rank: Option<usize>) -> &'static str {
    match rank {
        Some(1) => "arc-badge gold",
        Some(2) => "arc-badge silver",
        Some(3) => "arc-badge bronze",
        _ => "arc-badge",
    }
}

#[component]
pub fn ArcadeShell(ui: Ui, tab: RwSignal<Tab>) -> impl IntoView {
    let is_host = move || ui.host.get() == ui.me.get() && ui.me.get() != 0;
    let lobby = move || ui.phase.get() == Phase::Lobby;
    let copy = move |_| {
        if let Some(w) = web_sys::window() {
            let _ = w.navigator().clipboard().write_text(&w.location().href().unwrap_or_default());
        }
    };
    let to_sheet = move |_| {
        let mut s = ui.settings.get_untracked();
        s.theme = Theme::Sheet;
        ui.settings.set(s.clone());
        core::apply_settings(s);
    };
    let phase_text = move || match ui.phase.get() {
        Phase::Lobby if !ui.ranking.get().is_empty() => "GAME OVER".to_string(),
        Phase::Lobby => "READY".to_string(),
        Phase::Countdown => format!("START {}", ui.countdown.get()),
        Phase::Playing => "BATTLE".to_string(),
    };
    let conn = move || match ui.conn.get() {
        Conn::Open => ("arc-conn ok", "ONLINE"),
        Conn::Connecting => ("arc-conn wait", "연결 중"),
        _ => ("arc-conn bad", "OFFLINE"),
    };
    let tab_btn = move |t: Tab, label: &'static str| {
        view! {
            <button class="arc-tab" class:active=move || tab.get() == t on:click=move |_| tab.set(t)>{label}</button>
        }
    };

    view! {
        <div class="arc">
            <header class="arc-hud">
                <span class="arc-logo">"BLOCK STACK"</span>
                <span class="arc-room">"ROOM " <b>{move || ui.room.get()}</b></span>
                <button class="arc-btn small" on:click=copy>"초대 링크 복사"</button>
                <button class="arc-btn small" on:click=move |_| tab.set(Tab::Rooms)>"방 목록"</button>
                <span class="arc-phase">{phase_text}</span>
                <span class=move || conn().0>{move || conn().1}</span>
                <span class="spacer"></span>
                <button class="arc-btn" disabled=move || !(is_host() && lobby()) on:click=move |_| core::send(ClientMsg::AddBot)>"봇 +"</button>
                <button class="arc-btn" disabled=move || !(is_host() && lobby()) on:click=move |_| core::send(ClientMsg::RemoveBot)>"봇 −"</button>
                <button
                    class="arc-btn start"
                    disabled=move || !(is_host() && lobby() && ui.conn.get() == Conn::Open)
                    on:click=move |_| core::send(ClientMsg::Start)
                >
                    "START"
                </button>
                <button class="arc-btn small" on:click=to_sheet title="Spreadsheet Mode 로 전환">"업무 화면"</button>
            </header>

            <main class="arc-main">
                <div class="arc-stage" class:hidden=move || tab.get() != Tab::Game>
                    <MyField ui=ui/>
                    <Opponents ui=ui/>
                    <aside class="arc-chat"><ChatPanel ui=ui/></aside>
                </div>
                <div class="arc-page" class:hidden=move || tab.get() != Tab::Settings><SettingsSheet ui=ui/></div>
                <div class="arc-page" class:hidden=move || tab.get() != Tab::Help><HelpSheet ui=ui/></div>
                <div class="arc-page" class:hidden=move || tab.get() != Tab::Rooms><RoomsSheet ui=ui tab=tab/></div>
                <div class="arc-page" class:hidden=move || tab.get() != Tab::Log><LogSheet ui=ui/></div>
            </main>

            <footer class="arc-foot">
                <RankStrip ui=ui/>
                <div class="arc-foot-row">
                    {tab_btn(Tab::Game, "대전")}
                    {tab_btn(Tab::Settings, "설정")}
                    {tab_btn(Tab::Help, "도움말")}
                    {tab_btn(Tab::Rooms, "방 목록")}
                    {tab_btn(Tab::Log, "기록")}
                    <span class="arc-ticker">
                        {move || ui.log.with(|l| l.back().map(|(t, m)| format!("{t}  {m}")).unwrap_or_default())}
                    </span>
                    <span class="spacer"></span>
                    <span class="arc-perf">
                        {move || {
                            let p = ui.perf.get();
                            format!("입력 {:.1}ms · RTT {:.0}ms · 렌더 {}/s", p.input_p50, p.rtt, p.renders)
                        }}
                    </span>
                </div>
            </footer>
        </div>
    }
}

#[component]
fn MyField(ui: Ui) -> impl IntoView {
    let me = move || ui.me.get();
    let my = move || stat_of(ui, me());
    let my_name = move || {
        let id = me();
        ui.players.with(|p| p.iter().find(|x| x.id == id).map(|x| x.name.clone())).unwrap_or_else(|| "PLAYER".into())
    };
    let my_rank = move || rank_of(ui, me());
    // 게임판 위 표시: 카운트다운 / K.O. / 결과
    let overlay = move || -> Option<(String, &'static str)> {
        match ui.phase.get() {
            Phase::Countdown if ui.countdown.get() > 0 => Some((ui.countdown.get().to_string(), "count")),
            Phase::Playing if !my().alive && !ui.scores.with(|s| s.is_empty()) => Some(("K.O.".into(), "ko")),
            Phase::Lobby if !ui.ranking.get().is_empty() => {
                let r = ui.ranking.get();
                match r.iter().position(|&id| id == me()) {
                    Some(0) if r.len() > 1 => Some(("WIN!".into(), "win")),
                    Some(n) => Some((format!("{}위", n + 1), "lose")),
                    None => None,
                }
            }
            Phase::Lobby => Some(("READY".into(), "ready")),
            _ => None,
        }
    };
    view! {
        <section class="arc-me">
            <div class="arc-plate me">
                <span class=move || badge_class(my_rank())>{move || my_rank().map(|r| r.to_string()).unwrap_or_else(|| "-".into())}</span>
                <span class="arc-name">{my_name}</span>
                <span class="arc-ko">"KO " <b>{move || my().kos}</b></span>
            </div>
            <div class="arc-field">
                <div class="arc-left">
                    <div class="arc-box">
                        <div class="arc-label">"HOLD"</div>
                        <div id="hold" class="preview"></div>
                    </div>
                    <div class="arc-box arc-stats">
                        <div class="arc-label">"SCORE"</div>
                        <div class="arc-big">{move || my().score}</div>
                        <div class="arc-kv"><span>"LINES"</span><b>{move || my().lines}</b></div>
                        <div class="arc-kv"><span>"LEVEL"</span><b>{move || my().level}</b></div>
                        <div class="arc-kv"><span>"공격"</span><b>{move || my().sent}</b></div>
                        <div class="arc-kv"><span>"상쇄"</span><b>{move || my().cancelled}</b></div>
                    </div>
                </div>
                <div class="meter" title="받을 공격">
                    <div class="meter-fill" style:height=move || format!("{}%", (my().pending * 5).min(100))></div>
                </div>
                <div class="arc-board-wrap">
                    <div id="my-board" class="board"></div>
                    {move || overlay().map(|(text, kind)| view! { <div class=format!("arc-overlay {kind}")>{text}</div> })}
                </div>
                <div class="arc-right">
                    <div class="arc-box">
                        <div class="arc-label">"NEXT"</div>
                        {(0..PREVIEW)
                            .map(|i| view! { <div id=format!("next-{i}") class="preview" class:first=i == 0></div> })
                            .collect_view()}
                    </div>
                    <div class="arc-box arc-incoming" class:hot=move || { my().pending > 0 }>
                        <div class="arc-label">"받을 공격"</div>
                        <div class="arc-big">{move || my().pending}</div>
                    </div>
                </div>
            </div>
        </section>
    }
}

#[component]
fn Opponents(ui: Ui) -> impl IntoView {
    let opponents = move || {
        let me = ui.me.get();
        ui.players.get().into_iter().filter(|p| p.id != me).collect::<Vec<PlayerInfo>>()
    };
    let duel = move || opponents().len() <= 1;
    // 상대가 없으면 빈 자리 1개, 1:1 이면 큰 상대 판만, 다인이면 남은 자리를 빈 슬롯으로 표시
    let empty_slots = move || match opponents().len() {
        0 => 1,
        1 => 0,
        n => MAX_OPPONENT_SLOTS.saturating_sub(n),
    };
    view! {
        <section class="arc-opps" class:duel=duel>
            <For
                each=opponents
                key=|p: &PlayerInfo| p.id
                children=move |p: PlayerInfo| {
                    let id = p.id;
                    let st = move || stat_of(ui, id);
                    let rank = move || rank_of(ui, id);
                    let in_game = p.in_game;
                    view! {
                        <div class="arc-opp" class:out=move || in_game && !st().alive>
                            <div class="arc-plate">
                                <span class=move || badge_class(rank())>{move || rank().map(|r| r.to_string()).unwrap_or_else(|| "-".into())}</span>
                                <span class="arc-name" title=p.name.clone()>{p.name.clone()}</span>
                                <span class="arc-ko">"KO " <b>{move || st().kos}</b></span>
                            </div>
                            <div class="arc-opp-board">
                                <div id=format!("opp-{id}") class="mini"></div>
                                <Show when=move || in_game && !st().alive && ui.phase.get() == Phase::Playing>
                                    <div class="arc-overlay ko small">"K.O."</div>
                                </Show>
                            </div>
                            <div class="arc-opp-meta">
                                <span>{move || st().score}</span>
                                <span class:hot=move || { st().pending > 0 }>"대기 " {move || st().pending}</span>
                            </div>
                        </div>
                    }
                }
            />
            {move || {
                (0..empty_slots())
                    .map(|_| view! { <div class="arc-opp empty"><div class="arc-empty">"빈 자리"<br/><small>"초대 링크로 친구를 부르거나 봇 + 를 누르세요"</small></div></div> })
                    .collect_view()
            }}
        </section>
    }
}

#[component]
fn RankStrip(ui: Ui) -> impl IntoView {
    let chips = move || {
        let players = ui.players.get();
        let me = ui.me.get();
        let mut list: Vec<(usize, PlayerInfo, PlayerStat)> = players
            .into_iter()
            .map(|p| (rank_of(ui, p.id).unwrap_or(99), p.clone(), stat_of(ui, p.id)))
            .collect();
        list.sort_by_key(|x| (x.0, std::cmp::Reverse(x.2.score)));
        list.into_iter()
            .map(|(r, p, s)| {
                let rank = (r != 99).then_some(r);
                view! {
                    <span class="arc-chip" class:me={p.id == me} class:out={p.in_game && !s.alive}>
                        <span class=badge_class(rank)>{rank.map(|r| r.to_string()).unwrap_or_else(|| "-".into())}</span>
                        {p.name.clone()}
                        <b>{s.score}</b>
                        <small>{format!("승 {}", p.wins)}</small>
                    </span>
                }
            })
            .collect_view()
    };
    view! { <div class="arc-ranks">{chips}</div> }
}
