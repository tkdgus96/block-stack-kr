//! 아케이드 모드 화면 (독자 디자인).
//!
//! 블록 퍼즐 대전 게임 장르의 일반적인 배치를 따른다:
//! 가운데 큰 내 게임판, 왼쪽 HOLD + 점수판, 오른쪽 NEXT 5개, 판 옆 공격 게이지,
//! 이름표·순위 배지·KO 수, 1:1 이면 큰 상대 판 / 다인이면 상대 판 슬롯 5개,
//! 게임판 위 카운트다운·K.O.·결과 표시.
//!
//! 게임판 셀은 여기서도 그리지 않는다. 같은 id(my-board, hold, next-N, opp-N)의 컨테이너만 만들고
//! runtime.rs 의 렌더러가 바뀐 셀만 직접 그린다. 게임판은 변경된 셀만 갱신하며 이벤트 피드백은 짧게 표시한다.

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

/// 현재 순위: 확정 순위 또는 생존자의 공격 줄·KO·점수 순 (동점 공동 순위)
fn rank_of(ui: Ui, id: u8) -> Option<usize> {
    if ui.coop.get().enabled { return None; }
    let final_place = ui.scores.with(|s| s.iter().find(|s| s.id == id).map_or(0, |s| s.place));
    if !ui.ranking.with(|r| r.is_empty()) && final_place > 0 { return Some(final_place as usize); }
    let ranking = ui.ranking.get();
    if let Some(i) = ranking.iter().position(|&r| r == id) {
        return Some(i + 1);
    }
    if ui.room_config.get().teams { return None; }
    let scores = ui.scores.get();
    let me = scores.iter().find(|s| s.id == id)?;
    if me.place > 0 {
        return Some(me.place as usize);
    }
    if !me.alive { return None; }
    let metrics = (me.sent, me.kos, me.score);
    Some(1 + scores.iter().filter(|s| s.alive && (s.sent, s.kos, s.score) > metrics).count())
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
        Phase::Lobby if !ui.ranking.get().is_empty() => "MATCH COMPLETE".to_string(),
        Phase::Lobby => "WAITING FOR PLAYERS".to_string(),
        Phase::Countdown => format!("STARTING IN {}", ui.countdown.get()),
        Phase::Playing => "BATTLE IN PROGRESS".to_string(),
    };
    let conn = move || match ui.conn.get() {
        Conn::Open => ("arc-conn ok", "ONLINE"),
        Conn::Connecting => ("arc-conn wait", "연결 중"),
        _ => ("arc-conn bad", "OFFLINE"),
    };
    let tab_btn = move |t: Tab, label: &'static str| {
        view! {
            <button class="arc-tab" class:active=move || tab.get() == t aria-current=move || if tab.get() == t { "page" } else { "false" }
                on:click=move |_| tab.set(t)>{label}</button>
        }
    };
    view! {
        <div class="arc">
            <header class="arc-hud">
                <a class="arc-brand" href="#" on:click=move |ev| { ev.prevent_default(); tab.set(Tab::Game); } aria-label="Block Stack 대전 화면">
                    <span class="arc-brand-mark" aria-hidden="true">"▦"</span>
                    <span class="arc-logo">"BLOCK"<b>"STACK"</b><small>"MULTIPLAYER ARENA"</small></span>
                </a>
                <nav class="arc-nav" aria-label="게임 메뉴">
                    {tab_btn(Tab::Game, "대전 아레나")}
                    {tab_btn(Tab::Rooms, "방 찾기")}
                    {tab_btn(Tab::Practice, "연습실")}
                    {tab_btn(Tab::Records, "개인 기록")}
                    {tab_btn(Tab::Settings, "설정")}
                    {tab_btn(Tab::Help, "플레이 가이드")}
                </nav>
                <span class="spacer"></span>
                <span class=move || conn().0>{move || conn().1}</span>
                <button class="arc-btn small" on:click=to_sheet title="Spreadsheet Mode로 전환">"▤ 업무 화면"</button>
            </header>
            <main class="arc-main">
                <div class="arc-arena" class:hidden=move || tab.get() != Tab::Game>
                    <section class="arc-matchbar" aria-label="현재 대전방">
                        <div><div class="arc-eyebrow">"CLASSIC / GARBAGE BATTLE"</div><h1>{move || if ui.room_config.get().teams { "함께 승리하세요.".into() } else { format!("{}분 안에 승부하세요.",ui.room_config.get().duration_secs / 60) }}</h1>
                            <p>"끝까지 생존하고 더 많은 공격을 보내세요. 1인 연습은 시간 제한 없이."</p></div>
                        <div class="arc-match-info"><crate::room_controls::IdentitySummary ui=ui/><crate::battle::MatchClock ui=ui/><span class="arc-phase">{phase_text}</span>
                            <div><span class="arc-room">"ROOM " <b>{move || ui.room.get()}</b></span>
                                <button class="arc-btn small" on:click=copy>"초대 링크 ↗"</button></div>
                        </div>
                    </section>
                    <crate::room_controls::LobbyPresence ui=ui/>
                    <crate::battle::MatchResults ui=ui/>
                    <div class="arc-stage">
                        <div class="arc-play-area">
                            <div class="arc-section-title"><span>"01 / YOUR FIELD"</span><span>"실시간 블록 대전"</span></div>
                            <MyField ui=ui/>
                            <crate::touch::TouchControls ui=ui/>
                            <div class="arc-controls"><span><kbd>"←"</kbd><kbd>"→"</kbd>" 이동"</span><span><kbd>"↑"</kbd>" 회전"</span>
                                <span><kbd>"SPACE"</kbd>" 드롭"</span><span><kbd>"C"</kbd>" 홀드"</span>
                                <button class="link" on:click=move |_| tab.set(Tab::Settings)>"키 설정 ↗"</button></div>
                        </div>
                        <div class="arc-rivals-area">
                            <div class="arc-section-title"><span>{move || if ui.room_config.get().teams { "02 / TEAMS" } else { "02 / RIVALS" }}</span><span>{move || format!("{} / {} PLAYERS", ui.players.with(|p| p.len()),ui.room_config.get().max_players)}</span></div>
                            <Opponents ui=ui/>
                        </div>
                        <aside class="arc-social">
                            <div class="arc-section-title"><span>"03 / ROOM LOUNGE"</span><span>"● LIVE"</span></div>
                            <div class="arc-room-actions">
                                <div class="arc-action-title"><b>"함께하면 더 재밌죠"</b><span>{move || format!("{}명 참가 중", ui.players.with(|p| p.len()))}</span></div>
                                <p>{move || if lobby() { if is_host() { "친구를 초대하거나 연습봇을 추가해 시작하세요." } else { "방장이 게임을 시작할 때까지 채팅하며 기다려 주세요." } } else { "게임 중 입장한 플레이어는 다음 판부터 참가합니다." }}</p>
                                <div class="arc-bot-actions">
                                    <button class="arc-btn" disabled=move || !(is_host() && lobby() && ui.players.with(|p| p.len() < ui.room_config.get().max_players as usize)) on:click=move |_| core::send(ClientMsg::AddBot)>"+ 연습봇"</button>
                                    <button class="arc-btn" disabled=move || !(is_host() && lobby() && ui.players.with(|p| p.iter().any(|p| p.bot))) on:click=move |_| core::send(ClientMsg::RemoveBot)>"봇 제거"</button>
                                </div>
                                <button class="arc-btn start" disabled=move || !crate::room_controls::can_start(ui)
                                    on:click=move |_| core::send(ClientMsg::Start)>{move || if !lobby() { "대전 진행 중" } else if is_host() { "준비하고 시작  →" } else { "방장 시작 대기" }}</button>
                            </div>
                            <crate::room_controls::RoomControls ui=ui/><crate::coop::CoopPanel ui=ui/>
                            <div class="arc-chat"><ChatPanel ui=ui/></div>
                        </aside>
                    </div>
                    <crate::battle::AttackStats ui=ui/>
                </div>
                <div class="arc-page" class:hidden=move || tab.get() != Tab::Settings><SettingsSheet ui=ui/></div>
                <div class="arc-page" class:hidden=move || tab.get() != Tab::Help><HelpSheet ui=ui/></div>
                <div class="arc-page" class:hidden=move || tab.get() != Tab::Rooms><RoomsSheet ui=ui tab=tab/></div>
                <div class="arc-page" class:hidden=move || tab.get() != Tab::Records><crate::records::PersonalRecords ui=ui/><crate::profile::ProfilePanel/><crate::replay::ReplayPanel ui=ui/></div>
                <Show when=move || tab.get() == Tab::Practice && ui.phase.get() != Phase::Lobby><div class="arc-page"><p>"경기 중입니다. 대전 화면에서 현재 경기를 마친 뒤 연습실을 이용하세요."</p></div></Show>
                <Show when=move || tab.get() == Tab::Practice && ui.phase.get() == Phase::Lobby><div class="arc-page"><crate::practice::PracticeLab/></div></Show>
                <div class="arc-page" class:hidden=move || tab.get() != Tab::Log><LogSheet ui=ui/></div>
            </main>
            <footer class="arc-foot">
                <RankStrip ui=ui/>
                <div class="arc-foot-row">
                    <span class="arc-live-dot" aria-hidden="true"></span>
                    <span class="arc-ticker" role="status">{move || ui.status.get()}</span>
                    <button class="arc-tab" on:click=move |_| tab.set(Tab::Log)>"상황 로그 ↗"</button>
                    <span class="spacer"></span>
                    <span class="arc-perf">{move || { let p = ui.perf.get(); format!("RTT {:.0}ms · 입력 {:.1}ms", p.rtt, p.input_p50) }}</span>
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
    let incoming = crate::combat::attack_pulse(ui, move || ui.me.get());
    // 게임판 위 표시: 카운트다운 / K.O. / 결과
    let overlay = move || -> Option<(String, &'static str)> {
        match ui.phase.get() {
            Phase::Countdown if ui.countdown.get() > 0 => Some((ui.countdown.get().to_string(), "count")),
            Phase::Playing if !my().alive && !ui.scores.with(|s| s.is_empty()) => Some(("K.O.".into(), "ko")),
            Phase::Lobby if !ui.ranking.get().is_empty() => {
                if ui.coop.get().enabled { return Some((if ui.coop.get().won == Some(true) { "협동 성공!".into() } else { "도전 종료".into() },if ui.coop.get().won == Some(true) { "win" } else { "lose" })); }
                if crate::battle::is_draw(ui) && ui.battle_result.with(|r| r.as_ref().is_some_and(|(_, winners)| winners.contains(&me()))) { return Some(("DRAW".into(), "win")); }
                match rank_of(ui, me()) {
                    Some(1) if ui.battle_result.with(|r| r.as_ref().is_some_and(|(_, winners)| winners.contains(&me()))) => Some(("WIN!".into(), "win")),
                    Some(n) => Some((format!("{}위", n), "lose")),
                    None => None,
                }
            }
            Phase::Lobby => Some(("READY".into(), "ready")),
            _ => None,
        }
    };
    view! {
        <section class="arc-me" class:danger=move || { my().pending >= 8 }>
            <div class="arc-plate me">
                <span class=move || badge_class(my_rank())>{move || my_rank().map(|r| r.to_string()).unwrap_or_else(|| "-".into())}</span>
                <span class="arc-name">{my_name}</span>
                <span class="arc-you">"YOU"</span><span class="team-tag">{move || crate::room_controls::team_label(ui,me())}</span><span class="arc-ko">"KO " <b>{move || my().kos}</b></span>
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
                        <div class="arc-kv"><span>"LEVEL"</span><b>{move || my().level + 1}</b></div>
                        <div class="arc-kv"><span>"공격"</span><b>{move || my().sent}</b></div>
                        <div class="arc-kv"><span title="분당 보낸 공격 줄">"APM"</span><b>{move || crate::battle::apm(ui, me())}</b></div>
                        <div class="arc-kv"><span>"상쇄"</span><b>{move || my().cancelled}</b></div>
                    </div>
                </div>
                <div class="meter" class:combat-receiving=move || { incoming.get() > 0 } title="받을 공격">
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
        <crate::combat::CombatPanel ui=ui/>
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
                key=|p: &PlayerInfo| (p.id, p.name.clone(), p.bot)
                children=move |p: PlayerInfo| {
                    let id = p.id;
                    let st = move || stat_of(ui, id);
                    let rank = move || rank_of(ui, id);
                    let in_game = move || ui.players.with(|players| players.iter().any(|p| p.id == id && p.in_game));
                    view! {
                        <div class="arc-opp" class:targeted=move || stat_of(ui, ui.me.get()).target == Some(id) class:out=move || in_game() && !st().alive>
                            <div class="arc-plate">
                                <span class=move || badge_class(rank())>{move || rank().map(|r| r.to_string()).unwrap_or_else(|| "-".into())}</span>
                                <span class="arc-name" title=p.name.clone()>{format!("{} · {}",crate::room_controls::player_tag(id),p.name)}<Show when=move || id == ui.host.get()><small class="lobby-board-host">" ♛"</small></Show></span><span class="team-tag">{move || crate::room_controls::team_label(ui,id)}</span>
                                <span class="arc-ko">"KO " <b>{move || st().kos}</b></span>
                            </div>
                            <div class="arc-opp-board">
                                <crate::combat::TargetPulse ui=ui id=id/>
                                <div id=format!("opp-{id}") class="mini"></div>
                                <Show when=move || in_game() && !st().alive && ui.phase.get() == Phase::Playing>
                                    <div class="arc-overlay ko small">"K.O."</div>
                                </Show>
                            </div>
                            <div class="combat-opp-chain">{move || crate::combat::chain_text(ui,id)}</div>
                            <div class="arc-opp-meta">
                                <span>{move || format!("공격 {} · {} APM", st().sent, crate::battle::apm(ui, id))}</span>
                                <span class:hot=move || { st().pending > 0 }>"대기 " {move || st().pending}</span>
                            </div>
                        </div>
                    }
                }
            />
            {move || {
                (0..empty_slots())
                    .map(|_| view! { <div class="arc-opp empty"><div class="arc-empty">"VS"<br/><small>"도전자를 기다리고 있어요"</small></div></div> })
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
