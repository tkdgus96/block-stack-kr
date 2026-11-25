//! Lobby configuration, team selection and readiness shared by all themes.
use crate::runtime::{self as core, Conn, Ui};
use bs_engine::protocol::{ClientMsg, Phase};
use leptos::prelude::*;

pub(crate) fn team_label(ui: Ui, id: u8) -> &'static str {
    if !ui.room_config.get().teams { return ""; }
    if ui.members.with(|m| m.iter().find(|m| m.id == id).is_some_and(|m| m.team == 1)) { "B팀" } else { "A팀" }
}
/// IDs are unique within the current room, including players with identical names.
pub(crate) fn player_tag(id: u8) -> String { format!("P{id:02}") }

pub(crate) fn can_start(ui: Ui) -> bool {
    if ui.phase.get() != Phase::Lobby || ui.conn.get() != Conn::Open || ui.host.get() != ui.me.get() { return false; }
    let players = ui.players.get();
    let members = ui.members.get();
    if players.is_empty() || players.iter().any(|p| !p.bot && p.id != ui.host.get() && !members.iter().any(|m| m.id == p.id && m.ready)) { return false; }
    let config = ui.room_config.get();
    !config.teams || (players.len() == config.max_players as usize && (0..=1).all(|team| members.iter().filter(|m| m.team == team).count() == config.max_players as usize / 2))
}

#[component]
pub(crate) fn IdentitySummary(ui: Ui) -> impl IntoView {
    view! {
        <div class="lobby-identity" aria-label="내 참가 정보">
            <span class="lobby-self-label">"나"</span>
            <strong>{move || ui.players.with(|players| players.iter().find(|p| p.id == ui.me.get()).map(|p| p.name.clone())).unwrap_or_else(|| "연결 중".into())}</strong>
            <span class="lobby-player-id">{move || if ui.me.get() == 0 { "—".into() } else { player_tag(ui.me.get()) }}</span>
            <Show when=move || ui.me.get() != 0 && ui.me.get() == ui.host.get()><span class="lobby-host">"♛ 방장"</span></Show>
        </div>
    }
}

#[component]
pub(crate) fn ReadyButton(ui: Ui) -> impl IntoView {
    let ready = move || ui.members.with(|m| m.iter().find(|m| m.id == ui.me.get()).is_some_and(|m| m.ready));
    view! {
        <button class="arc-btn ready-button" class:active=ready
            disabled=move || ui.phase.get() != Phase::Lobby || ui.conn.get() != Conn::Open
            on:click=move |_| core::send(ClientMsg::Ready { ready: !ready() })>
            {move || if ready() { "준비 취소" } else { "준비하기" }}
        </button>
    }
}
#[component]
pub(crate) fn RoomControls(ui: Ui) -> impl IntoView {
    let draft = RwSignal::new(ui.room_config.get_untracked());
    Effect::new(move |_| draft.set(ui.room_config.get()));
    let locked = move || ui.phase.get() != Phase::Lobby || ui.host.get() != ui.me.get() || ui.conn.get() != Conn::Open;
    let me_team = move || ui.members.with(|m| m.iter().find(|m| m.id == ui.me.get()).map_or(0, |m| m.team));
    view! {
        <section class="room-controls feature-card" aria-label="방 설정">
            <IdentitySummary ui=ui/>
            <p class="lobby-name-hint">"같은 닉네임도 P번호로 구분합니다. 이름은 설정에서 변경할 수 있습니다."</p>
            <div class="feature-title"><h2>"방 설정"</h2><span>{move || if ui.room_config.get().public { "공개 방" } else { "비공개 방 · 링크/코드 입장" }}</span></div>
            <details class="room-rules"><summary>"규칙 변경 · "{move || format!("{} · {}분 · {}명", if ui.coop.get().enabled { "협동전" } else if ui.room_config.get().teams { "팀전" } else { "개인전" },ui.room_config.get().duration_secs / 60,ui.room_config.get().max_players)}</summary>
            <fieldset disabled=locked>
                <label>"대결 방식"
                    <select aria-label="대결 방식" prop:value=move || if draft.get().teams { "teams" } else { "solo" }
                        on:change=move |ev| { let teams = event_target_value(&ev) == "teams"; draft.update(|c| { c.teams = teams; if teams && c.max_players != 4 && c.max_players != 6 { c.max_players = 4; } }); }>
                        <option value="solo">"개인전"</option><option value="teams">"팀전 (2:2 / 3:3)"</option>
                    </select>
                </label>
                <label>"경기 시간"
                    <select aria-label="경기 시간" prop:value=move || draft.get().duration_secs.to_string()
                        on:change=move |ev| draft.update(|c| c.duration_secs = event_target_value(&ev).parse().unwrap_or(120))>
                        <option value="60">"1분"</option><option value="120">"2분"</option><option value="180">"3분"</option>
                    </select>
                </label>
                <label>"최대 인원"
                    <select aria-label="최대 인원" prop:value=move || draft.get().max_players.to_string()
                        on:change=move |ev| draft.update(|c| c.max_players = event_target_value(&ev).parse().unwrap_or(6))>
                        {(2..=6u8).map(|n| view! { <option value=n.to_string() disabled=move || (draft.get().teams && n != 4 && n != 6) || n < ui.players.with(|p| p.len()) as u8>{format!("{n}명")}</option> }).collect_view()}
                    </select>
                </label>
                <label title="쉬움: 지금 블록만 보고 놓습니다. 보통: 보관한 블록과 다음 블록까지 봅니다. 어려움: 보통에 더해 바닥에서 밀어 넣기와 T스핀을 찾고 공격을 우선합니다.">"봇 난이도"
                    <select aria-label="봇 난이도" prop:value=move || draft.get().bot_level.to_string()
                        on:change=move |ev| draft.update(|c| c.bot_level = event_target_value(&ev).parse().unwrap_or(1))>
                        <option value="0">"쉬움"</option><option value="1">"보통"</option><option value="2">"어려움"</option>
                    </select>
                </label>
                <label class="room-private"><input type="checkbox" aria-label="비공개 방" prop:checked=move || !draft.get().public on:change=move |ev| draft.update(|c| c.public = !event_target_checked(&ev))/><span>"비공개 방"</span></label>
                <button class="arc-btn" on:click=move |_| core::send(ClientMsg::SetRoomConfig(draft.get_untracked()))>"방 설정 적용"</button>
            </fieldset>
            <p class="feature-note">{move || if ui.host.get() != ui.me.get() { "방장만 대기 중에 설정을 변경할 수 있습니다." } else { "비공개 방은 목록에서 숨겨집니다. 링크나 코드를 아는 사람은 입장할 수 있습니다. 설정 변경 시 준비가 해제됩니다." }}</p>
            </details>
            <Show when=move || ui.room_config.get().teams>
                <div class="team-selector" aria-label="팀 선택">
                    <button class="arc-btn team-a" class:active=move || me_team() == 0 disabled=move || ui.phase.get() != Phase::Lobby on:click=move |_| core::send(ClientMsg::SetTeam { team: 0 })>"A팀 선택"</button>
                    <button class="arc-btn team-b" class:active=move || me_team() == 1 disabled=move || ui.phase.get() != Phase::Lobby on:click=move |_| core::send(ClientMsg::SetTeam { team: 1 })>"B팀 선택"</button>
                </div>
                <p class="feature-note">"각 팀 2명 또는 3명이 모두 모이면 시작합니다. 봇은 자동 배정되며 아군에게 공격하지 않습니다."</p>
            </Show>
            <div class="lobby-roster-heading"><h3>"참가자"</h3><span aria-live="polite">{move || {
                if ui.phase.get() != Phase::Lobby { return "경기 상태".into(); }
                let players = ui.players.get();
                let ready = players.iter().filter(|p| p.bot || ui.members.with(|m| m.iter().any(|m| m.id == p.id && m.ready))).count();
                format!("준비 {ready} / {}명", players.len())
            }}</span></div>
            <div class="ready-members" aria-label="참가자 및 준비 상태">
                <For each=move || ui.players.get() key=|p| (p.id,p.name.clone()) children=move |p| {
                    let id = p.id;
                    let bot = p.bot;
                    let ready = move || bot || ui.members.with(|m| m.iter().any(|m| m.id == id && m.ready));
                    view! { <div class="arc-roster-player" class:is-me=move || id == ui.me.get() data-player-id=id>
                        <span class="arc-avatar" class:mine=move || id == ui.me.get() data-color=(id % 6).to_string()>{player_tag(id)}</span>
                        <span class="arc-roster-name"><span>{p.name}</span><span class="lobby-role-tags">
                            <Show when=move || id == ui.me.get()><small class="lobby-me">"나"</small></Show>
                            <Show when=move || id == ui.host.get()><small class="lobby-host">"♛ 방장"</small></Show>
                            <Show when=move || bot><small>"AI · 자동 준비"</small></Show>
                            <span class="team-tag">{move || team_label(ui,id)}</span>
                        </span></span>
                        <b class="lobby-status" class:ready=move || ui.phase.get() == Phase::Lobby && ready()
                            class:waiting=move || ui.phase.get() == Phase::Lobby && !ready()>{move || match ui.phase.get() {
                                Phase::Countdown => if ui.players.with(|p| p.iter().any(|p| p.id == id && p.in_game)) { "시작 대기" } else { "관전" },
                                Phase::Playing => if ui.players.with(|p| p.iter().any(|p| p.id == id && p.in_game)) { if ui.scores.with(|s| s.iter().any(|s| s.id == id && !s.alive)) { "탈락" } else { "경기 중" } } else { "관전" },
                                Phase::Lobby => if ready() { "✓ 준비 완료" } else { "○ 준비 안 됨" }
                            }}</b>
                    </div> }
                }/>
            </div>
            <ReadyButton ui=ui/>
            <p class="feature-note lobby-start-note" aria-live="polite">{move || {
                if ui.phase.get() != Phase::Lobby { return "경기 중에는 준비 상태를 변경할 수 없습니다. 중도 입장자는 다음 경기에 참가합니다.".to_string(); }
                let config = ui.room_config.get();
                if config.teams {
                    let members = ui.members.get();
                    let a = members.iter().filter(|m| m.team == 0).count();
                    let b = members.iter().filter(|m| m.team == 1).count();
                    let half = config.max_players as usize / 2;
                    if a != half || b != half { return format!("팀전 시작 조건: A팀 {a}/{half}명 · B팀 {b}/{half}명. 각 팀 정원과 준비를 모두 채워 주세요."); }
                }
                let waiting = ui.players.with(|players| players.iter().filter(|p| !p.bot && p.id != ui.host.get() && !ui.members.with(|m| m.iter().any(|m| m.id == p.id && m.ready))).map(|p| player_tag(p.id)).collect::<Vec<_>>());
                if !waiting.is_empty() { format!("{} 준비를 기다리고 있습니다. 방장은 시작 버튼으로 준비와 시작을 함께 합니다.", waiting.join(", ")) }
                else if ui.summary.with(|s| s.is_some()) { "방장까지 모두 준비하면 자동 재대결합니다. 방장은 시작 버튼으로 바로 재대결할 수도 있습니다.".into() }
                else { "참가자 준비 완료! 방장이 시작 버튼을 누르면 방장도 준비하고 시작합니다.".into() }
            }}</p>
        </section>
    }
}

/// Compact roster above the boards, especially useful before scrolling on mobile.
#[component]
pub(crate) fn LobbyPresence(ui: Ui) -> impl IntoView {
    view! {
        <section class="lobby-presence" aria-label="대전 참가 현황">
            <For each=move || ui.players.get() key=|p| (p.id,p.name.clone()) children=move |p| {
                let id = p.id; let bot = p.bot;
                let ready = move || bot || ui.members.with(|m| m.iter().any(|m| m.id == id && m.ready));
                view! { <div class="presence-player" class:mine=move || id == ui.me.get()>
                    <span class="lobby-player-id">{player_tag(id)}</span>
                    <strong>{p.name}</strong>
                    <Show when=move || id == ui.me.get()><span class="lobby-me">"나"</span></Show>
                    <Show when=move || id == ui.host.get()><span class="lobby-host">"♛ 방장"</span></Show>
                    <span class="presence-state" class:ready=move || ui.phase.get() == Phase::Lobby && ready()>{move || {
                        if ui.phase.get() == Phase::Lobby { if ready() { "✓ 준비" } else { "○ 미준비" } }
                        else if ui.players.with(|p| p.iter().find(|p| p.id == id).is_none_or(|p| !p.in_game)) { "관전" }
                        else if ui.scores.with(|s| s.iter().any(|s| s.id == id && !s.alive)) { "탈락" } else { "경기 중" }
                    }}</span>
                </div> }
            }/>
        </section>
    }
}
