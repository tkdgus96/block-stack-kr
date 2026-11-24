//! Self-contained offline lab: never sends room inputs or writes multiplayer records.
use bs_engine::{
    practice::{Mode, Outcome, Session},
    Input, VISIBLE_CELLS, WIDTH,
};
use leptos::{html, prelude::*};
use wasm_bindgen::{closure::Closure, JsCast};

fn now() -> f64 {
    js_sys::Date::now()
}
fn best_key(mode: Mode) -> String {
    format!("blockstack.practice.v1.{}", mode.key())
}
fn best(mode: Mode) -> Option<u32> {
    web_sys::window()?
        .local_storage()
        .ok()??
        .get_item(&best_key(mode))
        .ok()??
        .parse()
        .ok()
}
fn save_best(s: &Session) -> bool {
    let Some(value) = s.record_value() else {
        return true;
    };
    let improve = best(s.mode).is_none_or(|old| {
        if s.mode == Mode::Blitz {
            value > old
        } else {
            value < old
        }
    });
    if !improve {
        return true;
    }
    web_sys::window()
        .and_then(|w| w.local_storage().ok().flatten())
        .is_some_and(|storage| {
            storage
                .set_item(&best_key(s.mode), &value.to_string())
                .is_ok()
        })
}
fn time(ticks: u32) -> String {
    format!(
        "{}:{:02}.{:02}",
        ticks / 3600,
        (ticks / 60) % 60,
        (ticks % 60) * 100 / 60
    )
}
fn draw(canvas: &web_sys::HtmlCanvasElement, session: &Session) {
    let Some(ctx) = canvas
        .get_context("2d")
        .ok()
        .flatten()
        .and_then(|v| v.dyn_into::<web_sys::CanvasRenderingContext2d>().ok())
    else {
        return;
    };
    let mut cells = [0; VISIBLE_CELLS];
    session.game.render_visible(true, &mut cells);
    let palette = [
        "#10182a", "#22c3e6", "#f5c518", "#a855f7", "#3ccf5a", "#ef4444", "#3b6cf6", "#f59e0b",
        "#6b7280", "#303b55",
    ];
    ctx.set_fill_style_str("#080e1d");
    ctx.fill_rect(0., 0., 240., 480.);
    for (i, c) in cells.iter().enumerate() {
        ctx.set_fill_style_str(palette[(*c as usize).min(9)]);
        ctx.fill_rect(
            (i % WIDTH) as f64 * 24. + 1.,
            (i / WIDTH) as f64 * 24. + 1.,
            22.,
            22.,
        );
    }
}
#[component]
pub(crate) fn PracticeLab() -> impl IntoView {
    let session = RwSignal::new(Session::new(Mode::TSpin, 1));
    let started = RwSignal::new(now());
    let running = RwSignal::new(false);
    let saved = RwSignal::new(true);
    let held = RwSignal::new(Vec::<(i32, Input)>::new());
    let repeat_at = RwSignal::new(0.0);
    let canvas = NodeRef::<html::Canvas>::new();
    let root = NodeRef::<html::Section>::new();
    Effect::new(move |_| {
        let s = session.get();
        if let Some(c) = canvas.get() {
            draw(&c, &s);
        }
    });
    Effect::new(move |_| {
        if let Some(el) = root.get() {
            let _ = el.focus();
        }
    });
    let update = move || {
        if !running.get_untracked() {
            return;
        }
        if web_sys::window()
            .and_then(|w| w.document())
            .is_some_and(|d| d.hidden() || !d.has_focus().unwrap_or(true))
        {
            held.set(Vec::new());
        }
        let ticks = ((now() - started.get_untracked()).max(0.) * 0.06) as u32;
        session.update(|s| s.advance_to(ticks));
        if now() >= repeat_at.get_untracked() {
            let actions = held.get_untracked();
            if !actions.is_empty() {
                session.update(|s| {
                    for (_, action) in &actions {
                        s.input(*action);
                    }
                });
                repeat_at.set(now() + 80.);
            }
        }
        let s = session.get_untracked();
        if s.outcome != Outcome::Playing {
            running.set(false);
            held.set(Vec::new());
            saved.set(save_best(&s));
        }
    };
    let callback = Closure::<dyn FnMut()>::new(update);
    let interval = web_sys::window().and_then(|w| {
        w.set_interval_with_callback_and_timeout_and_arguments_0(
            callback.as_ref().unchecked_ref(),
            17,
        )
        .ok()
    });
    // Leptos cleanup requires Send; JS closures remain in the local StoredValue arena.
    let callback = StoredValue::new_local(callback);
    on_cleanup(move || {
        if let (Some(w), Some(id)) = (web_sys::window(), interval) {
            w.clear_interval_with_handle(id);
        }
        callback.dispose();
    });
    let input = move |key: Input| {
        if !running.get_untracked() {
            return;
        }
        update();
        session.update(|s| s.input(key));
        let s = session.get_untracked();
        if s.outcome != Outcome::Playing {
            running.set(false);
            held.set(Vec::new());
            saved.set(save_best(&s));
        }
    };
    let restart = move || {
        held.set(Vec::new());
        let mode = session.with_untracked(|s| s.mode);
        session.set(Session::new(mode, js_sys::Date::now() as u64));
        started.set(now());
        running.set(true);
        saved.set(true);
        if let Some(el) = root.get() {
            let _ = el.focus();
        }
    };
    view! {
        <section id="practice-lab" class="practice-lab feature-card" tabindex="0" node_ref=root aria-label="혼자 연습실"
            on:keydown=move |e:web_sys::KeyboardEvent| {
                e.stop_propagation();
                let action=match e.code().as_str(){"ArrowLeft"=>Some(Input::Left),"ArrowRight"=>Some(Input::Right),"ArrowDown"=>Some(Input::SoftDrop),"ArrowUp"|"KeyX"=>Some(Input::RotateCw),"KeyZ"=>Some(Input::RotateCcw),"Space"=>Some(Input::HardDrop),"KeyC"|"ShiftLeft"=>Some(Input::Hold),_=>None};
                if let Some(action)=action {e.prevent_default();if !e.repeat() || matches!(action,Input::Left|Input::Right|Input::SoftDrop){input(action);}}
            }
            on:keyup=move |e:web_sys::KeyboardEvent| e.stop_propagation()
            on:focusout=move |_|held.set(Vec::new())>
            <div class="feature-title"><h2>"혼자 연습실"</h2><span>"대전 전적에 영향 없음"</span></div>
            <div class="practice-modes">{Mode::ALL.into_iter().map(move |mode|view!{<button type="button" class:selected=move ||session.with(|s|s.mode==mode) on:click=move |_|{running.set(false);held.set(Vec::new());session.set(Session::new(mode,1));saved.set(true);}>{mode.title()}</button>}).collect_view()}</div>
            <h3>{move ||session.with(|s|s.mode.title())}</h3>
            <p class="feature-note">{move ||session.with(|s|s.mode.hint())}</p>
            <div class="practice-arena">
                <div class="practice-playfield">
                <canvas node_ref=canvas width="240" height="480" class="practice-board" aria-label="연습 게임판"></canvas>
            <div class="practice-touch" aria-label="연습 터치 조작">{[(Input::Left,"◀ 이동"),(Input::SoftDrop,"▼ 내리기"),(Input::Right,"이동 ▶"),(Input::RotateCcw,"↶ 회전"),(Input::RotateCw,"회전 ↷"),(Input::Hold,"보관"),(Input::HardDrop,"⤓ 고정")].into_iter().map(move |(action,label)|view!{<button type="button" disabled=move ||!running.get() on:pointerdown=move |e:web_sys::PointerEvent| {
                e.prevent_default();input(action);
                if matches!(action,Input::Left|Input::Right|Input::SoftDrop) && running.get_untracked(){
                    if let Some(el)=e.current_target().and_then(|t|t.dyn_into::<web_sys::Element>().ok()){let _=el.set_pointer_capture(e.pointer_id());}
                    held.update(|v|v.push((e.pointer_id(),action)));repeat_at.set(now()+180.);
                }
            }
            on:pointerup=move |e:web_sys::PointerEvent|held.update(|v|v.retain(|(id,_)|*id!=e.pointer_id()))
            on:pointercancel=move |e:web_sys::PointerEvent|held.update(|v|v.retain(|(id,_)|*id!=e.pointer_id()))
            on:lostpointercapture=move |e:web_sys::PointerEvent|held.update(|v|v.retain(|(id,_)|*id!=e.pointer_id()))
            on:click=move |e:web_sys::MouseEvent|if e.detail()==0{input(action);}>{label}</button>}).collect_view()}</div>
                </div>
                <div class="practice-side">
                    <div class="practice-status" role="status" aria-live="polite">{move || session.with(|s|match s.outcome {Outcome::Success=>"도전 성공!",Outcome::Failed=>"도전 종료 · 다시 시도하세요",Outcome::Playing=>if running.get(){"플레이 중"}else{"시작 버튼을 누르세요"}})}</div>
                    <dl><dt>"시간"</dt><dd>{move ||session.with(|s|if s.mode==Mode::Blitz {time(7200-s.ticks.min(7200))}else{time(s.ticks)})}</dd>
                    <dt>"지운 줄"</dt><dd>{move ||session.with(|s|format!("{}{}",s.game.stats.lines,if s.mode==Mode::Sprint{" / 40"}else{""}))}</dd>
                    <dt>"점수"</dt><dd>{move ||session.with(|s|s.game.stats.score)}</dd>
                    <dt>"상쇄 / 대기 공격"</dt><dd>{move ||session.with(|s|format!("{} / {}",s.game.stats.cancelled,s.game.pending_total()))}</dd>
                    <dt>"보관 / 다음 블록"</dt><dd>{move ||session.with(|s|format!("{} / {}",s.game.hold.map(|k|format!("{k:?}")).unwrap_or_else(||"—".into()),s.game.next.iter().take(5).map(|k|format!("{k:?}")).collect::<Vec<_>>().join(" ")))}</dd>
                    <dt>"개인 최고"</dt><dd>{move ||session.with(|s|best(s.mode).map(|v|if s.mode==Mode::Blitz{format!("{v}점")}else{time(v)}).unwrap_or_else(||"아직 없음".into()))}</dd></dl>
                    <button type="button" class="primary practice-start" on:click=move |_|restart()>{move ||if running.get(){"처음부터 다시"}else{"시작 / 다시 도전"}}</button>
                    <p class="feature-note">"← → 이동 · ↓ 내리기 · ↑/X 오른쪽 회전 · Z 왼쪽 회전 · Space 고정 · C 보관"</p>
                    <p class="feature-note">"연습 과제는 자동 낙하 없이 천천히 조작할 수 있습니다. 최고 기록은 이 브라우저에 저장됩니다."</p>
                    <Show when=move ||!saved.get()><p role="status">"저장소를 사용할 수 없어 이번 기록은 저장되지 않았습니다."</p></Show>
                </div>
            </div>

        </section>
    }
}
