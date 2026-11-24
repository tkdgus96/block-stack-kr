//! Bounded recording of authoritative own-board snapshots, never predicted inputs.
use bs_engine::protocol::CombatEvent;
use bs_engine::{Game, SpinKind};
use leptos::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

#[derive(Clone, Default, Serialize, Deserialize)]
pub(crate) struct Replay {
    pub frames: Vec<Frame>,
    pub events: Vec<TimedEvent>,
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Frame {
    pub tick: u32,
    pub cells: Vec<u8>,
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct TimedEvent {
    pub tick: u32,
    pub event: CombatEvent,
}
impl std::ops::Deref for TimedEvent {
    type Target = CombatEvent;
    fn deref(&self) -> &CombatEvent {
        &self.event
    }
}
#[derive(Default)]
pub(crate) struct Recorder {
    frames: VecDeque<Frame>,
    events: Vec<TimedEvent>,
}
impl Recorder {
    pub fn frame(&mut self, game: &Game) {
        if self
            .frames
            .back()
            .is_some_and(|f| game.elapsed < f.tick + 6 && !game.dead)
        {
            return;
        }
        let mut cells = vec![0; 200];
        game.render_visible(true, &mut cells);
        self.frames.push_back(Frame {
            tick: game.elapsed,
            cells,
        });
        while self
            .frames
            .front()
            .is_some_and(|f| f.tick + 900 < game.elapsed)
            || self.frames.len() > 152
        {
            self.frames.pop_front();
        }
    }
    pub fn event(&mut self, tick: u32, event: CombatEvent) {
        if self.events.len() < 4096 {
            self.events.push(TimedEvent { tick, event });
        }
    }
    pub fn finish(&self) -> Replay {
        Replay {
            frames: self.frames.iter().cloned().collect(),
            events: self.events.clone(),
        }
    }
}
impl Replay {
    pub fn load() -> Option<Self> {
        let raw = web_sys::window()?
            .local_storage()
            .ok()??
            .get_item("bs-last-replay")
            .ok()??;
        if raw.len() > 2_000_000 {
            return None;
        }
        let r: Self = serde_json::from_str(&raw).ok()?;
        (r.frames.len() <= 152
            && r.events.len() <= 4096
            && r.frames
                .iter()
                .all(|f| f.cells.len() == 200 && f.cells.iter().all(|c| *c <= 15))
            && r.frames.windows(2).all(|w| w[0].tick <= w[1].tick))
        .then_some(r)
    }
    pub fn save(&self) {
        if let (Some(s), Ok(v)) = (
            web_sys::window().and_then(|w| w.local_storage().ok().flatten()),
            serde_json::to_string(self),
        ) {
            let _ = s.set_item("bs-last-replay", &v);
        }
    }
}

#[component]
pub(crate) fn ReplayPanel(ui: crate::runtime::Ui) -> impl IntoView {
    let index = RwSignal::new(0usize);
    let playing = RwSignal::new(false);
    Effect::new(move |_| {
        let _ = ui.replay.get();
        index.set(0);
        playing.set(false);
    });
    let origin = StoredValue::new(0.0f64);
    let timer = wasm_bindgen::closure::Closure::<dyn FnMut()>::new(move || {
        if !playing.get_untracked() {
            return;
        }
        let target = ((js_sys::Date::now() - origin.get_value()).max(0.0) * 0.06) as u32;
        ui.replay.with_untracked(|r| {
            if let Some(r) = r {
                index.set(r.frames.iter().rposition(|f| f.tick <= target).unwrap_or(0));
                if r.frames.last().is_none_or(|f| target >= f.tick) { playing.set(false); }
            } else { playing.set(false); }
        });
    });
    use wasm_bindgen::JsCast;
    let timer = StoredValue::new_local(timer);
    let handle = StoredValue::new(None::<i32>);
    Effect::new(move |_| {
        if let (Some(w), Some(h)) = (web_sys::window(), handle.get_value()) {
            w.clear_interval_with_handle(h);
            handle.set_value(None);
        }
        if playing.get() {
            let tick = ui.replay.with_untracked(|r| r.as_ref().and_then(|r| r.frames.get(index.get_untracked())).map_or(0, |f| f.tick));
            origin.set_value(js_sys::Date::now() - tick as f64 / 0.06);
            timer.with_value(|timer| {
                handle.set_value(web_sys::window().and_then(|w| {
                    w.set_interval_with_callback_and_timeout_and_arguments_0(
                        timer.as_ref().unchecked_ref(),
                        100,
                    )
                    .ok()
                }));
            });
        }
    });
    on_cleanup(move || {
        if let (Some(w), Some(h)) = (web_sys::window(), handle.get_value()) {
            w.clear_interval_with_handle(h);
        }
        timer.dispose();
    });
    view! {
        <section class="feature-card replay-panel" aria-label="리플레이와 경기 분석">
            <h2>"리플레이 · 경기 분석"</h2>
            <p>"최근 참가 경기의 마지막 15초를 다시 봅니다. 이 브라우저에 마지막 한 경기를 보관합니다."</p>
            <Show when=move || ui.replay.with(|r| r.as_ref().is_some_and(|r| !r.frames.is_empty())) fallback=|| view! { <p>"대전을 완료하면 내 보드와 기술 기록이 여기에 표시됩니다."</p> }>
                <div class="replay-layout">
                    <div class="review-board" aria-label="내 보드 다시 보기">{(0..200usize).map(|cell| view! { <i class=move || ui.replay.with(|r| format!("cell c{}",r.as_ref().and_then(|r| r.frames.get(index.get())).and_then(|f| f.cells.get(cell)).copied().unwrap_or(0)))></i> }).collect_view()}</div>
                    <div class="review-analysis">
                        <strong>{move || ui.replay.with(|r| r.as_ref().map(|r| { let generated: u32 = r.events.iter().map(|e| e.generated as u32).sum(); let cancelled: u32 = r.events.iter().map(|e| e.cancelled as u32).sum(); let sent: u32 = r.events.iter().map(|e| e.sent as u32).sum(); format!("공격 발생 {generated}줄 · 상쇄 {cancelled}줄 · 전달 {sent}줄") }).unwrap_or_default())}</strong>
                        <p>{move || ui.replay.with(|r| r.as_ref().map(|r| format!("최대 {} COMBO · T스핀 {}회 · 퍼펙트 클리어 {}회",r.events.iter().map(|e| e.combo.max(0)).max().unwrap_or(0), r.events.iter().filter(|e| e.spin != SpinKind::None).count(),r.events.iter().filter(|e| e.perfect_clear).count())).unwrap_or_default())}</p>
                        <p>"기술을 누르면 마지막 15초에 포함된 해당 순간으로 이동합니다."</p>
                        <div class="review-events">{move || ui.replay.get().map(|r| { let first = r.frames.first().map_or(0, |f| f.tick);
                            r.events.iter().filter(|e| e.generated > 0 || e.spin != SpinKind::None).rev().take(20).map(|e| { let tick = e.tick; let in_clip = tick >= first && tick <= r.frames.last().map_or(0, |f| f.tick); let target = r.frames.iter().position(|f| f.tick >= tick).unwrap_or(r.frames.len().saturating_sub(1)); view! { <button class="arc-btn" disabled=!in_clip on:click=move |_| { playing.set(false); index.set(target); }>{format!("{}:{:02} · {} · {}줄",tick/3600,(tick/60)%60,if e.spin != SpinKind::None { "T-SPIN" } else if e.perfect_clear { "PC" } else { "CLEAR" },e.generated)}</button> } }).collect_view()
                        })}</div>
                    </div>
                </div>
                <div class="replay-controls"><button class="arc-btn" on:click=move |_| { let n = ui.replay.with_untracked(|r| r.as_ref().map_or(0, |r| r.frames.len())); if index.get_untracked() + 1 >= n { index.set(0); } playing.update(|p| *p = !*p); }>{move || if playing.get() { "일시 정지" } else { "재생" }}</button>
                    <input type="range" aria-label="리플레이 위치" min="0" max=move || ui.replay.with(|r| r.as_ref().map_or(0, |r| r.frames.len().saturating_sub(1))).to_string() prop:value=move || index.get().to_string() on:input=move |ev| { playing.set(false); index.set(event_target_value(&ev).parse().unwrap_or(0)); }/>
                    <span>{move || ui.replay.with(|r| r.as_ref().and_then(|r| r.frames.get(index.get())).map(|f| format!("{}:{:02}.{}",f.tick/3600,(f.tick/60)%60,(f.tick%60)/6)).unwrap_or_default())}</span>
                </div>
            </Show>
        </section>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recording_keeps_last_fifteen_seconds_and_captures_death() {
        let mut recorder = Recorder::default();
        let mut g = Game::new(2);
        for t in 0..2000 {
            g.elapsed = t;
            recorder.frame(&g);
        }
        g.dead = true;
        recorder.frame(&g);
        let replay = recorder.finish();
        assert!(replay.frames.len() <= 152);
        assert_eq!(replay.frames.last().unwrap().tick, 1999);
        assert!(replay.frames.first().unwrap().tick >= 1099);
    }
}
