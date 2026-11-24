//! Event-driven arcade atmosphere. Mounted once outside the theme switch.
use crate::{runtime::Ui, settings::Theme, sound};
use bs_engine::protocol::Phase;
use leptos::prelude::*;

#[component]
pub(crate) fn Atmosphere(ui: Ui) -> impl IntoView {
    // Seed from current feed so theme remounts never replay an old attack.
    let seen = StoredValue::new(ui.combat_feed.with_untracked(|feed| feed.iter().rev()
        .find(|e| e.player == ui.me.get_untracked()).map(|e| (ui.room.get_untracked(), e.match_id, e.piece))));
    let previous_clock = StoredValue::new(ui.match_info.get_untracked());
    let previous_danger = StoredValue::new(false);
    let active = move || ui.phase.get() == Phase::Playing && ui.players.with(|players|
        players.iter().any(|p| p.id == ui.me.get() && p.in_game)) &&
        ui.scores.with(|scores| scores.iter().any(|s| s.id == ui.me.get() && s.alive));
    let urgent = move || active() && { let (left, _, timed) = ui.match_info.get(); timed && left <= 30 };
    let danger = move || active() && ui.scores.with(|scores| scores.iter().any(|s| s.id == ui.me.get() && s.pending >= 8));
    let combo = move || active() && ui.combat_states.with(|states| states.iter().any(|s| s.id == ui.me.get() && s.combo >= 4));
    Effect::new(move |_| {
        let settings = ui.settings.get();
        let audible = settings.sound && settings.theme == Theme::Arcade && active();
        let event = ui.combat_feed.with(|feed| feed.iter().rev().find(|e| e.player == ui.me.get()).cloned());
        if let Some(event) = event {
            let key = (ui.room.get(), event.match_id, event.piece);
            if seen.get_value().as_ref() != Some(&key) {
                seen.set_value(Some(key));
                if audible { sound::combat(&event); }
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
