//! 전체 경기 리플레이 보기. 서버가 저장한 시드와 입력 기록을 같은 엔진으로 다시 실행해 모든 게임판을 보여 준다.
use crate::runtime::Ui;
use bs_engine::protocol::decode;
use bs_engine::replay::{MatchReplay, Player};
use bs_engine::{Game, TICKS_PER_SECOND, VISIBLE_CELLS, WIDTH};
use leptos::prelude::*;
use wasm_bindgen::{closure::Closure, JsCast};

const CELL: f64 = 12.0;
const PALETTE: [&str; 10] = [
    "#10182a", "#22c3e6", "#f5c518", "#a855f7", "#3ccf5a", "#ef4444", "#3b6cf6", "#f59e0b", "#6b7280", "#303b55",
];

fn draw(id: &str, game: &Game) {
    let Some(canvas) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id(id))
        .and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok())
    else {
        return;
    };
    let Some(ctx) = canvas
        .get_context("2d")
        .ok()
        .flatten()
        .and_then(|c| c.dyn_into::<web_sys::CanvasRenderingContext2d>().ok())
    else {
        return;
    };
    let mut cells = [0u8; VISIBLE_CELLS];
    game.render_visible(false, &mut cells);
    ctx.set_fill_style_str("#080e1d");
    ctx.fill_rect(0., 0., CELL * WIDTH as f64, CELL * 20.);
    for (i, c) in cells.iter().enumerate() {
        ctx.set_fill_style_str(PALETTE[(*c as usize).min(9)]);
        ctx.fill_rect((i % WIDTH) as f64 * CELL + 1., (i / WIDTH) as f64 * CELL + 1., CELL - 2., CELL - 2.);
    }
}

fn clock(tick: u32) -> String {
    format!("{}:{:02}", tick / 3600, (tick / 60) % 60)
}

#[component]
pub(crate) fn FullReplayPanel(ui: Ui) -> impl IntoView {
    let replay = RwSignal::new(None::<MatchReplay>);
    let status = RwSignal::new(String::new());
    let tick = RwSignal::new(0u32);
    let playing = RwSignal::new(false);
    let runner = StoredValue::new(None::<Player>);

    // 리플레이 id 가 바뀌면 서버에서 받아 온다.
    Effect::new(move |_| {
        let Some(id) = ui.full_replay_id.get() else { return };
        status.set("리플레이를 불러오는 중…".into());
        playing.set(false);
        wasm_bindgen_futures::spawn_local(async move {
            let result = crate::api::get_bytes(&format!("/api/replay/{id}")).await;
            if replay.is_disposed() { return; }
            match result.ok().and_then(|b| decode::<MatchReplay>(&b)) {
                Some(r) => {
                    runner.set_value(Some(Player::new(&r)));
                    tick.set(0);
                    status.set(format!("{} · {}명 · {}", clock(r.end_tick), r.players.len(), r.id));
                    replay.set(Some(r));
                }
                None => status.set("리플레이를 찾을 수 없습니다. 서버는 최근 500경기만 보관합니다.".into()),
            }
        });
    });

    // tick 이 바뀌면 진행하고 다시 그린다. 뒤로 가면 처음부터 다시 실행한다.
    Effect::new(move |_| {
        let t = tick.get();
        replay.with(|r| {
            let Some(r) = r else { return };
            runner.update_value(|p| {
                if p.as_ref().is_none_or(|p| p.tick > t) {
                    *p = Some(Player::new(r));
                }
                if let Some(p) = p {
                    p.advance_to(r, t);
                    for (i, (_, game, _)) in p.games.iter().enumerate() {
                        draw(&format!("full-replay-{i}"), game);
                    }
                }
            });
        });
    });

    let timer = Closure::<dyn FnMut()>::new(move || {
        if !playing.get_untracked() { return; }
        let end = replay.with_untracked(|r| r.as_ref().map_or(0, |r| r.end_tick));
        let next = (tick.get_untracked() + TICKS_PER_SECOND / 20).min(end);
        tick.set(next);
        if next >= end { playing.set(false); }
    });
    let interval = web_sys::window().and_then(|w| {
        w.set_interval_with_callback_and_timeout_and_arguments_0(timer.as_ref().unchecked_ref(), 50).ok()
    });
    let timer = StoredValue::new_local(timer);
    on_cleanup(move || {
        if let (Some(w), Some(id)) = (web_sys::window(), interval) {
            w.clear_interval_with_handle(id);
        }
        timer.dispose();
    });

    let copied = RwSignal::new(false);
    let share = move |_| {
        let Some(id) = ui.full_replay_id.get_untracked() else { return };
        let Some(w) = web_sys::window() else { return };
        let loc = w.location();
        let link = format!("{}{}?replay={id}", loc.origin().unwrap_or_default(), loc.pathname().unwrap_or_default());
        let _ = w.navigator().clipboard().write_text(&link);
        copied.set(true);
    };

    view! {
        <section class="feature-card full-replay" aria-label="전체 경기 리플레이">
            <h2>"전체 경기 다시 보기"</h2>
            <p>"끝난 대전의 모든 게임판을 처음부터 다시 봅니다. 서버가 입력 기록만 저장하고 같은 엔진으로 다시 실행합니다."</p>
            <Show when=move || ui.full_replay_id.with(|id| id.is_some()) fallback=|| view! { <p>"대전을 마치면 여기에서 다시 볼 수 있습니다."</p> }>
                <p class="full-replay-status" role="status">{move || status.get()}</p>
                <div class="full-replay-boards">
                    {move || replay.with(|r| r.as_ref().map(|r| r.players.iter().enumerate().map(|(i, p)| view! {
                        <figure>
                            <canvas id=format!("full-replay-{i}") width=(CELL * WIDTH as f64).to_string() height=(CELL * 20.).to_string()></canvas>
                            <figcaption>{format!("{}{}", p.name, if p.bot { " (봇)" } else { "" })}</figcaption>
                        </figure>
                    }).collect_view()))}
                </div>
                <div class="replay-controls">
                    <button class="arc-btn" on:click=move |_| {
                        let end = replay.with_untracked(|r| r.as_ref().map_or(0, |r| r.end_tick));
                        if tick.get_untracked() >= end { tick.set(0); }
                        playing.update(|p| *p = !*p);
                    }>{move || if playing.get() { "일시 정지" } else { "재생" }}</button>
                    <input type="range" aria-label="전체 리플레이 위치" min="0"
                        max=move || replay.with(|r| r.as_ref().map_or(0, |r| r.end_tick)).to_string()
                        prop:value=move || tick.get().to_string()
                        on:input=move |ev| { playing.set(false); tick.set(event_target_value(&ev).parse().unwrap_or(0)); }/>
                    <span>{move || clock(tick.get())}</span>
                    <button class="arc-btn" on:click=share>{move || if copied.get() { "링크 복사됨" } else { "공유 링크 복사" }}</button>
                </div>
            </Show>
        </section>
    }
}
