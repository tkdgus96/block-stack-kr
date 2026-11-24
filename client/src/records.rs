//! Local-only personal history. Aggregate totals survive the bounded recent list.
use bs_engine::protocol::MatchSummary;
use leptos::prelude::*;
use serde::{Deserialize, Serialize};
use crate::runtime::Ui;

const KEY: &str = "blockstack.records.v1";
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Records {
    pub rounds: u32,
    pub battles: u32,
    pub wins: u32,
    pub draws: u32,
    pub total_sent: u64,
    pub best_apm: f64,
    pub recent: Vec<PersonalRound>,
    #[serde(skip)]
    pub available: bool,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct PersonalRound {
    pub key: String,
    pub name: String,
    pub at_ms: f64,
    pub mode: String,
    pub place: u8,
    pub sent: u32,
    pub score: u32,
    pub kos: u32,
    pub active_ticks: u32,
    pub win: bool,
    pub draw: bool,
}
pub(crate) fn rate(sent: u32, ticks: u32) -> f64 {
    if ticks == 0 { 0.0 } else { sent as f64 * 3600.0 / ticks as f64 }
}
impl Records {
    pub(crate) fn load() -> Self {
        let storage = web_sys::window().and_then(|w| w.local_storage().ok().flatten());
        let mut value = storage.as_ref().and_then(|s| s.get_item(KEY).ok().flatten())
            .and_then(|s| serde_json::from_str::<Self>(&s).ok()).unwrap_or_default();
        value.recent.truncate(50);
        value.available = storage.is_some();
        value
    }
    pub(crate) fn record(&mut self, room: &str, me: u8, result: &MatchSummary, at_ms: f64) -> bool {
        let Some(player) = result.players.iter().find(|p| p.id == me) else { return false };
        let key = format!("{room}/{}", result.match_id);
        if self.recent.iter().any(|r| r.key == key) { return false; }
        let battle = result.players.len() >= 2;
        let draw = if result.teams {
            let teams: std::collections::HashSet<_> = result.players.iter().filter(|p| result.winners.contains(&p.id)).map(|p| p.team).collect();
            teams.len() > 1
        } else { result.winners.len() > 1 };
        let draw = draw && result.winners.contains(&me);
        let win = battle && !draw && result.winners.contains(&me);
        self.rounds = self.rounds.saturating_add(1);
        self.battles = self.battles.saturating_add(battle as u32);
        self.wins = self.wins.saturating_add(win as u32);
        self.draws = self.draws.saturating_add((battle && draw && result.winners.contains(&me)) as u32);
        self.total_sent = self.total_sent.saturating_add(player.sent as u64);
        self.best_apm = self.best_apm.max(rate(player.sent, player.active_ticks));
        self.recent.insert(0, PersonalRound {
            key, name: player.name.clone(), at_ms,
            mode: if result.teams { "팀전" } else if battle { "개인전" } else { "연습" }.into(),
            place: player.place, sent: player.sent, score: player.score, kos: player.kos,
            active_ticks: player.active_ticks, win, draw,
        });
        self.recent.truncate(50);
        true
    }
    pub(crate) fn save(&mut self) {
        self.available = web_sys::window().and_then(|w| w.local_storage().ok().flatten())
            .and_then(|s| serde_json::to_string(self).ok().map(|value| s.set_item(KEY, &value).is_ok())).unwrap_or(false);
    }
}
#[component]
pub(crate) fn PersonalRecords(ui: Ui) -> impl IntoView {
    let armed = RwSignal::new(false);
    view! {
        <section class="personal-records feature-card" aria-label="개인 기록">
            <div class="feature-title"><h2>"개인 기록"</h2><span>"이 브라우저에 저장"</span></div>
            <div class="record-metrics">
                <div><small>"완료 경기"</small><b>{move || ui.records.with(|r| r.rounds)}</b></div>
                <div><small>"대결 승률"</small><b>{move || ui.records.with(|r| if r.battles == 0 { "—".into() } else { format!("{:.0}%", r.wins as f64 * 100.0 / r.battles as f64) })}</b></div>
                <div><small>"최고 APM"</small><b>{move || ui.records.with(|r| format!("{:.1}", r.best_apm))}</b></div>
                <div><small>"경기당 공격"</small><b>{move || ui.records.with(|r| if r.rounds == 0 { "—".into() } else { format!("{:.1}", r.total_sent as f64 / r.rounds as f64) })}</b></div>
            </div>
            <Show when=move || ui.records.with(|r| !r.recent.is_empty())>
                <p class="feature-note">"최근 20경기 APM 추이 · 왼쪽이 이전 경기, 오른쪽이 최근 경기 (이 브라우저 기록)"</p>
                <div class="growth-chart" role="img" aria-label="최근 경기 APM 변화">{move || { let r = ui.records.get(); let values: Vec<_> = r.recent.iter().take(20).rev().map(|p| rate(p.sent,p.active_ticks)).collect(); let max = values.iter().copied().fold(1.0,f64::max); values.into_iter().enumerate().map(|(i,v)| view! { <div title=format!("{}번째 경기 APM {v:.1}",i+1)><i style=format!("height:{}%",(v/max*100.0).max(2.0))></i><small>{format!("{v:.0}")}</small></div> }).collect_view() }}</div>
            </Show>
            <p class="feature-note">{move || ui.records.with(|r| if r.available { "로그인 없이 닉네임과 최근 50경기를 보관합니다. 다른 기기와 동기화되지 않으며 브라우저 데이터를 삭제하면 기록도 사라집니다. 연습은 승률에서 제외합니다." } else { "브라우저 저장소를 사용할 수 없어 기록이 저장되지 않을 수 있습니다." })}</p>
            <div class="feature-scroll"><table class="feature-table"><thead><tr><th>"닉네임 / 모드"</th><th>"결과"</th><th>"공격"</th><th>"APM"</th><th>"KO"</th><th>"점수"</th></tr></thead>
                <tbody>{move || ui.records.get().recent.into_iter().map(|r| view! { <tr><th>{format!("{} · {}", r.name, r.mode)}</th><td>{if r.draw { "무승부".into() } else { format!("{}위", r.place) }}</td><td>{r.sent}</td><td>{format!("{:.1}", rate(r.sent,r.active_ticks))}</td><td>{r.kos}</td><td>{r.score}</td></tr> }).collect_view()}</tbody>
            </table></div>
            <Show when=move || ui.records.with(|r| r.rounds == 0)><p class="feature-note">"경기를 완료하면 첫 기록이 여기에 표시됩니다."</p></Show>
            <button class="link" on:click=move |_| {
                if !armed.get_untracked() { armed.set(true); return; }
                let mut records = Records::default(); records.save(); ui.records.set(records); armed.set(false);
            }>{move || if armed.get() { "한 번 더 누르면 기록을 삭제합니다" } else { "개인 기록 초기화" }}</button>
            <Show when=move || armed.get()><button class="link" on:click=move |_| armed.set(false)>"취소"</button></Show>
        </section>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bs_engine::protocol::RoundPlayer;
    fn result(id: u64) -> MatchSummary {
        MatchSummary { match_id:id, winners:vec![1], players: vec![
            RoundPlayer { id:1,name:"me".into(),sent:12,active_ticks:1800,place:1,..Default::default() },
            RoundPlayer { id:2,name:"other".into(),place:2,..Default::default() },
        ], ..Default::default() }
    }
    #[test]
    fn history_deduplicates_and_preserves_totals_beyond_recent_limit() {
        let mut records = Records::default();
        assert!(records.record("ROOM",1,&result(1),0.0));
        assert!(!records.record("ROOM",1,&result(1),1.0));
        assert!(!records.record("ROOM",3,&result(2),1.0));
        for id in 2..=60 { records.record("ROOM",1,&result(id),id as f64); }
        assert_eq!(records.rounds,60); assert_eq!(records.recent.len(),50);
        assert_eq!(records.total_sent,720); assert_eq!(records.wins,60);
        assert_eq!(records.best_apm,24.0);
        let encoded = serde_json::to_string(&records).unwrap();
        let restored: Records = serde_json::from_str(&encoded).unwrap();
        assert_eq!(restored.rounds,60); assert_eq!(restored.recent[0].key,"ROOM/60");
    }
    #[test]
    fn team_winners_are_wins_and_team_ties_are_draws() {
        let mut records = Records::default();
        let mut summary = result(1); summary.teams = true;
        summary.players.push(RoundPlayer { id:3,team:1,..Default::default() });
        summary.players[1].team = 0; summary.winners = vec![1,2];
        records.record("ROOM",1,&summary,0.0);
        assert_eq!((records.wins,records.draws),(1,0));
        summary.match_id = 2; summary.winners.push(3);
        records.record("ROOM",1,&summary,0.0);
        assert_eq!((records.wins,records.draws),(1,1));
        assert!(records.recent[0].draw); assert!(!records.recent[0].win);
    }
    #[test]
    fn eliminated_player_does_not_inherit_other_players_draw() {
        let mut records = Records::default();
        let mut summary = result(1);
        summary.players.push(RoundPlayer { id:3, place:3, ..Default::default() });
        summary.winners = vec![1,2];
        records.record("ROOM",3,&summary,0.0);
        assert_eq!((records.wins,records.draws),(0,0));
        assert!(!records.recent[0].draw);
        assert_eq!(records.recent[0].place,3);
    }
    #[test]
    fn practice_does_not_change_battle_win_rate_and_zero_ticks_are_finite() {
        let mut records = Records::default();
        let mut summary = result(1); summary.players.truncate(1);
        summary.players[0].active_ticks = 0;
        records.record("ROOM",1,&summary,0.0);
        assert_eq!((records.rounds,records.battles,records.wins),(1,0,0));
        assert_eq!(records.best_apm,0.0); assert_eq!(rate(99,0),0.0);
    }
}
