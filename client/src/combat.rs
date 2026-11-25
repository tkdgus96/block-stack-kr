//! Authoritative combat feedback; local prediction never announces attacks.
use crate::runtime::Ui;
use bs_engine::game::SpinKind;
use bs_engine::protocol::{CombatEvent, Phase};
use leptos::prelude::*;
use leptos::leptos_dom::helpers::{set_timeout_with_handle, TimeoutHandle};
use std::time::Duration;
use std::collections::VecDeque;

pub(crate) fn technique(e: &CombatEvent) -> &'static str {
    match (e.spin, e.cleared) {
        (SpinKind::Full, 0) => "T-SPIN",
        (SpinKind::Full, 1) => "T-SPIN SINGLE",
        (SpinKind::Full, 2) => "T-SPIN DOUBLE",
        (SpinKind::Full, _) => "T-SPIN TRIPLE",
        (SpinKind::Mini, 0) => "T-SPIN MINI",
        (SpinKind::Mini, _) => "T-SPIN MINI SINGLE",
        (_, 4) => "TETRIS",
        (_, 3) => "TRIPLE",
        (_, 2) => "DOUBLE",
        (_, 1) => "SINGLE",
        _ => "",
    }
}
pub(crate) fn chain_text(ui: Ui, id: u8) -> String {
    if ui.phase.get() != Phase::Playing { return String::new(); }
    ui.combat_states.with(|states| states.iter().find(|s| s.id == id).map(|s| {
        let mut labels = Vec::new();
        if s.combo >= 1 { labels.push(format!("{} COMBO",s.combo)); }
        if s.b2b >= 2 { labels.push(format!("B2B ×{}",s.b2b - 1)); }
        labels.join(" · ")
    }).unwrap_or_default())
}

fn meaningful(e: &CombatEvent) -> bool { e.cleared > 0 || e.spin != SpinKind::None || e.perfect_clear }
fn key(e: &CombatEvent) -> (u64,u8,u32) { (e.match_id,e.player,e.piece) }

/// A one-shot highlight, without a permanent animation or polling loop.
pub(crate) fn attack_pulse(ui: Ui, target: impl Fn() -> u8 + Copy + Send + Sync + 'static) -> RwSignal<u8> {
    let amount = RwSignal::new(0);
    let seen = StoredValue::new(ui.combat_feed.with_untracked(|feed| feed.iter().rev().find(|e| e.target == Some(target()) && e.sent > 0).map(key)));
    let timer = StoredValue::new(None::<TimeoutHandle>);
    Effect::new(move |_| {
        if ui.phase.get() != Phase::Playing {
            amount.set(0); seen.set_value(None);
            if let Some(timer) = timer.get_value() { timer.clear(); }
            return;
        }
        let event = ui.combat_feed.with(|feed| feed.iter().rev().find(|e| e.target == Some(target()) && e.sent > 0).cloned());
        if let Some(event) = event {
            if seen.get_value() == Some(key(&event)) { return; }
            seen.set_value(Some(key(&event)));
            if let Some(timer) = timer.get_value() { timer.clear(); }
            amount.set(event.sent);
            timer.set_value(set_timeout_with_handle(move || { amount.try_set(0); },Duration::from_millis(850)).ok());
        } else if ui.combat_feed.with(VecDeque::is_empty) {
            amount.set(0); seen.set_value(None);
            if let Some(timer) = timer.get_value() { timer.clear(); }
        }
    });
    on_cleanup(move || { if let Some(timer) = timer.get_value() { timer.clear(); } });
    amount
}

#[component]
pub(crate) fn TargetPulse(ui: Ui, id: u8) -> impl IntoView {
    let amount = attack_pulse(ui, move || id);
    view! { <span class="combat-hit-frame" class:hit=move || { amount.get() > 0 } class:big-hit=move || { amount.get() >= 4 } aria-hidden="true"></span> }
}

#[component]
pub(crate) fn CombatPanel(ui: Ui) -> impl IntoView {
    let state = move || {
        let idle = bs_engine::protocol::CombatStatus { id:0, combo:-1, b2b:0 };
        if ui.phase.get() != Phase::Playing { return idle; }
        ui.combat_states.with(|s| s.iter().find(|s| s.id == ui.me.get()).cloned()).unwrap_or(idle)
    };
    let latest = RwSignal::new(None::<CombatEvent>);
    let seen = StoredValue::new(ui.combat_feed.with_untracked(|feed| feed.iter().rev().find(|e| e.player == ui.me.get_untracked() && meaningful(e)).map(key)));
    let timer = StoredValue::new(None::<TimeoutHandle>);
    Effect::new(move |_| {
        if ui.phase.get() != Phase::Playing {
            latest.set(None); seen.set_value(None);
            if let Some(timer) = timer.get_value() { timer.clear(); }
            return;
        }
        // Keep significant feedback even if a later no-clear lock lands in the
        // same network batch; status counters still reflect the final lock.
        let event = ui.combat_feed.with(|f| f.iter().rev().find(|e| e.player == ui.me.get() && meaningful(e)).cloned());
        if let Some(event) = event {
            if seen.get_value() == Some(key(&event)) { return; }
            seen.set_value(Some(key(&event)));
            if let Some(timer) = timer.get_value() { timer.clear(); }
            latest.set(Some(event));
            timer.set_value(set_timeout_with_handle(move || { latest.try_set(None); },Duration::from_millis(2000)).ok());
        } else if ui.combat_feed.with(VecDeque::is_empty) {
            latest.set(None); seen.set_value(None);
            if let Some(timer) = timer.get_value() { timer.clear(); }
        }
    });
    on_cleanup(move || { if let Some(timer) = timer.get_value() { timer.clear(); } });
    view! {
        <section class="combat-panel" aria-label="콤보와 공격 피드백"
            class:combo-active=move || { state().combo >= 1 }
            class:combo-hot=move || { state().combo >= 4 }
            class:combo-max=move || { state().combo >= 7 }>
            <div class="combat-counter combo-counter" title="첫 줄 삭제 다음부터 연속 삭제마다 1 COMBO. 줄을 지우지 못하면 종료.">
                <small>"COMBO"</small><b>{move || if state().combo >= 1 { state().combo.to_string() } else { "—".into() }}</b>
            </div>
            <div class="combat-counter b2b-counter" class:active=move || { state().b2b >= 2 } title="테트리스/T스핀 줄 삭제 연속. 일반 1~3줄 삭제 시 끊기고, 줄 삭제 없는 고정은 유지.">
                <small>"B2B"</small><b>{move || if state().b2b >= 2 { format!("×{}",state().b2b - 1) } else if state().b2b == 1 { "READY".into() } else { "—".into() }}</b>
            </div>
            <div class="combat-feedback" aria-live="polite" aria-atomic="true">
                <For each=move || { latest.get().into_iter().collect::<Vec<_>>() } key=key children=move |event: CombatEvent| {
                    view! { <div class="combat-card" class:special={event.spin != SpinKind::None || event.perfect_clear} class:big={event.sent >= 4}>
                        <strong class="combat-tech">{technique(&event)}</strong>
                        <span class="combat-bonuses">{if event.perfect_clear { "PERFECT CLEAR" } else { "" }}{if event.back_to_back { " · BACK-TO-BACK +1" } else { "" }}</span>
                        <span class="combat-amount">{format!("전송 +{}줄",event.sent)}
                            <small>{format!(" · 생성 {} · 상쇄 {}",event.generated,event.cancelled)}</small>
                        </span>
                    </div> }
                }/>
                <Show when=move || latest.with(Option::is_none)>
                    <div class="combat-idle"><b>{move || if ui.phase.get() == Phase::Playing { "연속 삭제로 콤보를 이어가세요" } else { "COMBO / T-SPIN / BACK-TO-BACK" }}</b><span>"공격 생성 → 상쇄 → 실제 전송"</span></div>
                </Show>
            </div>
        </section>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn distinct_skill_names_and_zero_clear_events() {
        let mut e = CombatEvent { match_id:1,player:1,piece:1,cleared:0,spin:SpinKind::None,combo:-1,b2b:0,back_to_back:false,perfect_clear:false,generated:0,cancelled:0,sent:0,target:None };
        assert!(!meaningful(&e));
        e.spin = SpinKind::Full;
        for (lines,label) in [(0,"T-SPIN"),(1,"T-SPIN SINGLE"),(2,"T-SPIN DOUBLE"),(3,"T-SPIN TRIPLE")] {
            e.cleared=lines;assert_eq!(technique(&e),label);assert!(meaningful(&e));
        }
        e.spin=SpinKind::Mini;e.cleared=1;assert_eq!(technique(&e),"T-SPIN MINI SINGLE");
        e.spin=SpinKind::None;e.cleared=4;assert_eq!(technique(&e),"TETRIS");
    }
}
