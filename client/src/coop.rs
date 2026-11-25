//! Cooperative survival mode: shared attack objective and periodic garbage waves.
use crate::runtime::{self as core, Ui};
use bs_engine::protocol::{ClientMsg, Phase};
use leptos::prelude::*;
#[component]
pub(crate) fn CoopPanel(ui: Ui) -> impl IntoView {
    view! {
        <section class="feature-card coop-panel" aria-label="협동 생존전">
            <div class="feature-title"><h2>"협동 생존전"</h2><label><input type="checkbox" aria-label="협동 생존전 사용" prop:checked=move || ui.coop.get().enabled disabled=move || ui.host.get() != ui.me.get() || ui.phase.get() != Phase::Lobby on:change=move |ev| core::send(ClientMsg::SetCoop { enabled: event_target_checked(&ev) })/>" 협동 모드"</label></div>
            <Show when=move || ui.coop.get().enabled fallback=|| view! { <p>"방장이 협동 모드를 켜면 서로 공격하지 않고 공동 목표에 도전합니다."</p> }>
                <strong>{move || { let c=ui.coop.get(); format!("공동 공격 {} / {}줄",c.progress,c.goal) }}</strong>
                <progress aria-label="공동 공격 목표" max=move || ui.coop.get().goal.to_string() value=move || ui.coop.get().progress.to_string()></progress>
                <p>"제한 시간 안에 공격 80줄을 함께 만드세요. 10초마다 모든 생존자에게 방해줄이 올라옵니다. 공격 상쇄 후 남은 줄이 공동 목표에 합산됩니다. 대전 승률·일일 미션에는 포함되지 않습니다."</p>
                <b>{move || match ui.coop.get().won { Some(true)=>"협동 성공!",Some(false)=>"도전 종료 · 다시 준비해 도전하세요",None=>"한 명이라도 생존하면 도전을 계속할 수 있습니다." }}</b>
            </Show>
        </section>
    }
}
