//! Server-authoritative clock and per-player attack statistics, shared by both themes.
use crate::runtime::Ui;
use bs_engine::protocol::{ClientMsg, Phase, TargetMode};
use leptos::prelude::*;

pub(crate) fn apm(ui: Ui, id: u8) -> String {
    let sent = ui.scores.with(|s| s.iter().find(|s| s.id == id).map_or(0, |s| s.sent));
    let ticks = ui.battle_stats.with(|s| s.iter().find(|s| s.id == id).map_or(0, |s| s.active_ticks));
    format!("{:.1}", if ticks == 0 { 0.0 } else { sent as f64 * 3600.0 / ticks as f64 })
}

pub(crate) fn is_draw(ui: Ui) -> bool {
    if let Some(summary) = ui.summary.get() {
        if !summary.teams { return summary.winners.len() > 1; }
        let teams: std::collections::HashSet<_> = summary.players.iter().filter(|p| summary.winners.contains(&p.id)).map(|p| p.team).collect();
        return teams.len() > 1;
    }
    ui.battle_result.with(|r| r.as_ref().is_some_and(|(_, winners)| {
        if !ui.room_config.get().teams { return winners.len() > 1; }
        let teams: std::collections::HashSet<_> = ui.members.with(|members| members.iter().filter(|p| winners.contains(&p.id)).map(|p| p.team).collect());
        teams.len() > 1
    }))
}

#[component]
pub(crate) fn MatchClock(ui: Ui) -> impl IntoView {
    view! {
        <div class="match-clock" class:urgent=move || { let (left, _, timed) = ui.match_info.get(); timed && left <= 20 && ui.phase.get() == Phase::Playing }
            aria-label="경기 시간" title="방 설정의 제한 시간을 적용합니다. 개인전: 생존 우선, 공격 줄 → KO → 점수. 팀전: 팀 생존자 수 → 공격 줄 → KO → 점수.">
            <small>{move || if ui.phase.get() == Phase::Lobby && ui.ranking.with(|r| r.is_empty()) { "대결 제한" } else if ui.match_info.get().2 { "남은 시간" } else { "1인 연습" }}</small>
            <strong>{move || {
                let (remaining, elapsed, timed) = ui.match_info.get();
                let secs = if ui.phase.get() == Phase::Lobby && ui.ranking.with(|r| r.is_empty()) { ui.room_config.get().duration_secs as u32 } else if timed { remaining as u32 } else { elapsed / 60 };
                format!("{}:{:02}", secs / 60, secs % 60)
            }}</strong>
        </div>
    }
}

#[component]
pub(crate) fn AttackStats(ui: Ui) -> impl IntoView {
    view! {
        <section class="battle-stats" aria-label="플레이어 공격 통계">
            <div class="battle-stats-heading"><b>"공격 통계"</b><span>"보낸 공격 줄 기준"</span></div>
            <div class="battle-stats-scroll">
                <table><thead><tr><th>"플레이어"</th><th>"이번 경기"</th><th>"분당 (APM)"</th><th>"경기당 평균"</th><th>"완료 경기"</th></tr></thead>
                    <tbody><For each=move || ui.players.get() key=|p| (p.id, p.name.clone()) children=move |p| {
                        let id = p.id;
                        let history = move || ui.battle_stats.with(|s| s.iter().find(|s| s.id == id).cloned()).unwrap_or_default();
                        view! { <tr class:mine=move || ui.me.get() == id><th>{p.name}</th>
                            <td>{move || ui.scores.with(|s| s.iter().find(|s| s.id == id).map(|s| s.sent.to_string())).unwrap_or_else(|| "—".into())}</td>
                            <td>{move || apm(ui, id)}</td>
                            <td>{move || { let h = history(); if h.rounds == 0 { "—".into() } else { format!("{:.1}", h.total_sent as f64 / h.rounds as f64) } }}</td>
                            <td>{move || history().rounds}</td>
                        </tr> }
                    }/></tbody>
                </table>
            </div>
            <p>"APM = 이번 경기 보낸 줄 ÷ 생존 플레이 시간(분). 경기당 평균은 이 방에서 완료한 경기 기준이며 재접속 시 초기화됩니다."</p>
        </section>
    }
}

#[component]
pub(crate) fn MatchResults(ui: Ui) -> impl IntoView {
    view! {
        <Show when=move || ui.summary.with(|s| s.is_some()) && ui.phase.get() == Phase::Lobby>
            <section class="match-results feature-card" aria-label="경기 결과">
                <div class="feature-title"><h2>"경기 결과"</h2><span>{move || ui.summary.with(|s| s.as_ref().map(|s| if s.timed_out { "제한 시간 종료" } else { "경기 종료" }).unwrap_or(""))}</span></div>
                <h3 class="result-headline">{move || ui.summary.with(|s| s.as_ref().map(|s| {
                    if ui.coop.get().enabled { return if ui.coop.get().won == Some(true) { "협동 성공!".into() } else { "협동 도전 종료".into() }; }
                    if s.players.len() == 1 { return "연습 완료".into(); }
                    if s.teams {
                        let teams: std::collections::HashSet<_> = s.players.iter().filter(|p| s.winners.contains(&p.id)).map(|p| p.team).collect();
                        if teams.len() > 1 { "무승부".into() }
                        else if let Some(team) = teams.iter().next() { format!("{}팀 승리!", if *team == 0 { "A" } else { "B" }) }
                        else { "대결 종료".into() }
                    } else if s.winners.len() > 1 { "공동 1위 · 무승부".into() }
                    else { s.players.iter().find(|p| s.winners.contains(&p.id)).map(|p| format!("{} 승리!",p.name)).unwrap_or_else(|| "대결 종료".into()) }
                }).unwrap_or_default())}</h3>
                <p class="result-delta">{move || {
                    let records = ui.records.get();
                    let summary = ui.summary.get();
                    match (records.recent.first(), records.recent.get(1), summary) {
                        (Some(current), Some(previous), Some(summary)) if current.key == format!("{}/{}",ui.room.get(),summary.match_id) =>
                            format!("직전 경기 대비 공격 {:+}줄 · APM {:+.1}", current.sent as i64 - previous.sent as i64, crate::records::rate(current.sent,current.active_ticks) - crate::records::rate(previous.sent,previous.active_ticks)),
                        _ => "다음 경기에서 자신의 기록에 도전하세요.".into(),
                    }
                }}</p>
                <div class="feature-scroll"><table class="feature-table"><thead><tr><th>"순위 / 플레이어"</th><th>"공격"</th><th>"APM"</th><th>"KO"</th><th>"점수"</th></tr></thead><tbody>
                    {move || ui.summary.get().map(|s| {
                        let mut players = s.players; players.sort_by_key(|p| (p.place,p.id));
                        players.into_iter().map(|p| view! {
                            <tr class:mine=move || ui.me.get() == p.id><th>{format!("{} · {}{}",if ui.coop.get().enabled { "협동 참가".into() } else { format!("{}위",p.place) }, if s.teams { if p.team == 0 { "[A] " } else { "[B] " } } else { "" }, p.name)}</th>
                                <td>{p.sent}</td><td>{format!("{:.1}",crate::records::rate(p.sent,p.active_ticks))}</td><td>{p.kos}</td><td>{p.score}</td></tr>
                        }).collect_view()
                    })}
                </tbody></table></div>
                <div class="result-rematch"><crate::room_controls::ReadyButton ui=ui/><span>"모두 준비하면 같은 멤버로 다시 시작합니다."</span></div>
            </section>
        </Show>
    }
}

const TARGET_MODES: [(TargetMode, &str, &str); 3] = [
    (TargetMode::Random, "무작위", "무작위로 고른 상대를 그 상대가 탈락할 때까지 공격합니다."),
    (TargetMode::Danger, "위험한 상대", "쌓인 높이와 받을 공격이 가장 많은 상대를 공격합니다."),
    (TargetMode::Counter, "반격", "나를 마지막으로 공격한 상대를 공격합니다. 없으면 나를 노리는 상대, 그것도 없으면 무작위입니다."),
];

/// 3인 이상 개인전·팀전에서만 보이는 공격 대상 선택
#[component]
pub(crate) fn TargetModeSelect(ui: Ui) -> impl IntoView {
    let visible = move || ui.players.with(|p| p.len() >= 3) && !ui.coop.get().enabled;
    let hint = move || TARGET_MODES.iter().find(|m| m.0 == ui.target_mode.get()).map_or("", |m| m.2);
    view! {
        <Show when=visible>
            <label class="target-mode" title=hint>
                <span>"공격 대상"</span>
                <select aria-label="공격 대상 선택" on:change=move |ev| {
                    let i: usize = event_target_value(&ev).parse().unwrap_or(0);
                    let mode = TARGET_MODES.get(i).map_or(TargetMode::Random, |m| m.0);
                    ui.target_mode.set(mode);
                    crate::runtime::send(ClientMsg::SetTargetMode { mode });
                }>
                    {TARGET_MODES.iter().enumerate().map(|(i, (mode, label, _))| {
                        let mode = *mode;
                        view! { <option value=i.to_string() selected=move || ui.target_mode.get() == mode>{*label}</option> }
                    }).collect_view()}
                </select>
            </label>
        </Show>
    }
}

/// 1:1 빠른 대전. 비슷한 레이팅의 상대를 찾고, 15초 안에 없으면 봇과 붙는다.
#[component]
pub(crate) fn QuickMatchButton(ui: Ui) -> impl IntoView {
    let lobby = move || ui.phase.get() == Phase::Lobby;
    view! {
        <button class="arc-btn quick-match" class:waiting=move || ui.quick_waiting.get() disabled=move || !lobby() && !ui.quick_waiting.get()
            title="비슷한 실력의 상대와 1:1 대전을 바로 시작합니다. 15초 안에 상대가 없으면 봇과 대전합니다."
            on:click=move |_| {
                if ui.quick_waiting.get_untracked() {
                    crate::runtime::send(ClientMsg::CancelQuickMatch);
                } else {
                    crate::runtime::send(ClientMsg::QuickMatch { token: crate::profile::token().unwrap_or_default() });
                }
            }>
            {move || if ui.quick_waiting.get() { "상대 찾는 중… (취소)" } else { "빠른 대전 1:1" }}
        </button>
    }
}
