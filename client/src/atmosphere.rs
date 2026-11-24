//! Event-driven arcade atmosphere. Mounted once outside the theme switch.
use crate::{runtime::Ui, settings::Theme, sound};
use bs_engine::protocol::Phase;
use leptos::prelude::*;

/// 내 게임판 둘레에 잠깐 효과 class 를 붙였다 뗀다. 게임판 그리기와는 따로 움직이므로 렌더링 비용이 없다.
fn board_fx(class: &'static str, ms: i32) {
    use wasm_bindgen::{closure::Closure, JsCast};
    let Some(el) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.query_selector(".arc-board-wrap").ok().flatten()) else { return };
    let list = el.class_list();
    // 같은 효과가 이어지면 다시 시작되도록 뗐다가 다음 프레임에 붙인다.
    let _ = list.remove_1(class);
    let add = Closure::once_into_js(move || {
        let _ = el.class_list().add_1(class);
        let remove = Closure::once_into_js(move || { let _ = el.class_list().remove_1(class); });
        if let Some(w) = web_sys::window() {
            let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(remove.unchecked_ref(), ms);
        }
    });
    if let Some(w) = web_sys::window() {
        let _ = w.request_animation_frame(add.unchecked_ref());
    }
}

#[component]
pub(crate) fn Atmosphere(ui: Ui) -> impl IntoView {
    // Seed from current feed so theme remounts never replay an old attack.
    let seen = StoredValue::new(ui.combat_feed.with_untracked(|feed| feed.iter().rev()
        .find(|e| e.player == ui.me.get_untracked()).map(|e| (ui.room.get_untracked(), e.match_id, e.piece))));
    let previous_clock = StoredValue::new(ui.match_info.get_untracked());
    let previous_danger = StoredValue::new(false);
    // 다른 사람이 나를 공격한 마지막 이벤트 (흔들기·효과음 중복 방지)
    let seen_hit = StoredValue::new(ui.combat_feed.with_untracked(|feed| feed.iter().rev()
        .find(|e| e.target == Some(ui.me.get_untracked())).map(|e| (ui.room.get_untracked(), e.match_id, e.player, e.piece))));
    let active = move || ui.phase.get() == Phase::Playing && ui.players.with(|players|
        players.iter().any(|p| p.id == ui.me.get() && p.in_game)) &&
        ui.scores.with(|scores| scores.iter().any(|s| s.id == ui.me.get() && s.alive));
    let urgent = move || active() && { let (left, _, timed) = ui.match_info.get(); timed && left <= 30 };
    let danger = move || active() && ui.scores.with(|scores| scores.iter().any(|s| s.id == ui.me.get() && s.pending >= 8));
    let combo = move || active() && ui.combat_states.with(|states| states.iter().any(|s| s.id == ui.me.get() && s.combo >= 4));
    Effect::new(move |_| {
        let settings = ui.settings.get();
        let audible = settings.sound && settings.theme == Theme::Arcade && active();
        let visual = settings.theme == Theme::Arcade && settings.effective_animations() && active();
        let hit = ui.combat_feed.with(|feed| feed.iter().rev().find(|e| e.target == Some(ui.me.get())).cloned());
        if let Some(hit) = hit {
            let key = (ui.room.get(), hit.match_id, hit.player, hit.piece);
            if seen_hit.get_value().as_ref() != Some(&key) {
                seen_hit.set_value(Some(key));
                if hit.sent >= 4 {
                    if visual { board_fx("fx-shake", 320); }
                    if audible { sound::heavy_hit(); }
                }
            }
        }
        let event = ui.combat_feed.with(|feed| feed.iter().rev().find(|e| e.player == ui.me.get()).cloned());
        if let Some(event) = event {
            let key = (ui.room.get(), event.match_id, event.piece);
            if seen.get_value().as_ref() != Some(&key) {
                seen.set_value(Some(key));
                if audible { sound::combat(&event); }
                if visual && event.cleared > 0 {
                    board_fx(if event.cleared >= 4 || event.spin != bs_engine::SpinKind::None || event.perfect_clear { "fx-big-clear" } else { "fx-clear" }, 280);
                }
            }
        }
        let clock = ui.match_info.get();
        let old = previous_clock.get_value();
        previous_clock.set_value(clock);
        // A crossing only, never every clock tick or when unmuting inside the final 30s.
        if audible && clock.2 && old.2 && old.0 > 30 && clock.0 <= 30 { sound::final_seconds(); }
        let now_danger = danger();
        let was_danger = previous_danger.get_value();
        previous_danger.set_value(now_danger);
        if audible && now_danger && !was_danger { sound::danger(); }
    });
    view! {
        <div class="arc-atmosphere" aria-hidden="true"
            class:enabled=move || ui.settings.with(|s| s.theme == Theme::Arcade && s.effective_animations()) && active()
            class:urgent=urgent class:danger=danger class:combo=combo></div>
    }
}
