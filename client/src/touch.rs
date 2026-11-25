//! Pointer controls shared by the arcade and spreadsheet game views.
use crate::runtime::{self, Conn, Ui};
use bs_engine::game::Input;
use bs_engine::protocol::Phase;
use leptos::prelude::*;
use wasm_bindgen::JsCast;

#[component]
pub(crate) fn TouchControls(ui: Ui) -> impl IntoView {
    let vibration = Signal::derive(move || ui.settings.with(|s| s.touch_vibration));
    let enabled = Signal::derive(move || {
        let me = ui.me.get();
        ui.conn.get() == Conn::Open
            && ui.phase.get() == Phase::Playing
            && ui
                .players
                .with(|players| players.iter().any(|p| p.id == me && p.in_game))
            && ui
                .scores
                .with(|scores| scores.iter().find(|s| s.id == me).is_some_and(|s| s.alive))
    });
    Effect::new(move |_| {
        if !enabled.get() {
            runtime::touch_cancel();
        }
    });
    on_cleanup(runtime::touch_cancel);
    view! {
        <section class="touch-controls" aria-label="터치 게임 조작"
            class:touch-left-handed=move || ui.settings.with(|s| s.touch_left_handed)
            class:touch-movement-first=move || ui.settings.with(|s| s.touch_layout == 1)
            style=move || ui.settings.with(|s| format!("--touch-scale:{};--touch-opacity:{}", s.touch_size.clamp(80, 125) as f32 / 100.0, s.touch_opacity.clamp(40, 100) as f32 / 100.0))>
            <div class="touch-heading"><span>"TOUCH CONTROLS"</span><small>{move || if enabled.get() { "이동 버튼을 길게 누르면 연속 이동" } else { "게임이 시작되면 조작할 수 있어요" }}</small></div>
            <div class="touch-actions">
                <TouchButton action=Input::Hold label="보관" symbol="HOLD" enabled=enabled vibration=vibration/>
                <TouchButton action=Input::RotateCcw label="왼쪽 회전" symbol="↶" enabled=enabled vibration=vibration/>
                <TouchButton action=Input::RotateCw label="오른쪽 회전" symbol="↷" enabled=enabled vibration=vibration/>
                <TouchButton action=Input::HardDrop label="바로 내리기" symbol="⤓" enabled=enabled vibration=vibration/>
            </div>
            <div class="touch-movement">
                <TouchButton action=Input::Left label="왼쪽 이동" symbol="◀" enabled=enabled vibration=vibration/>
                <TouchButton action=Input::SoftDrop label="천천히 내리기" symbol="▼" enabled=enabled vibration=vibration/>
                <TouchButton action=Input::Right label="오른쪽 이동" symbol="▶" enabled=enabled vibration=vibration/>
            </div>
            <GestureControls ui=ui enabled=enabled/>
            <Show when=move || ui.settings.with(|s|s.touch_gestures)><p class="gesture-help">"보드 탭 회전 · 좌우 드래그 이동 · 아래로 길게 스와이프 고정"</p></Show>
        </section>
    }
}

#[component]
fn TouchButton(
    action: Input,
    label: &'static str,
    symbol: &'static str,
    enabled: Signal<bool>,
    vibration: Signal<bool>,
) -> impl IntoView {
    // A separate pointer set for each action lets two thumbs move and rotate
    // together, and keeps a second finger from releasing the first finger's key.
    let pointers = RwSignal::new(Vec::<i32>::new());
    Effect::new(move |_| {
        if !enabled.get() {
            pointers.set(Vec::new());
        }
    });
    let finish = move |event: web_sys::PointerEvent| {
        event.prevent_default();
        let mut held = pointers.get_untracked();
        let previous = held.len();
        held.retain(|id| *id != event.pointer_id());
        if held.len() != previous {
            if held.is_empty() {
                runtime::touch_release(action);
            }
            pointers.set(held);
        }
    };
    view! {
        <button type="button" class="touch-button" class:pressed=move || !pointers.with(Vec::is_empty)
            class:touch-drop=action == Input::HardDrop
            class:touch-hold=action == Input::Hold
            aria-label=label title=label disabled=move || !enabled.get()
            on:contextmenu=move |event| event.prevent_default()
            on:pointerdown=move |event: web_sys::PointerEvent| {
                event.prevent_default();
                if !enabled.get_untracked() || event.button() != 0 { return; }
                let Some(target) = event.current_target().and_then(|target| target.dyn_into::<web_sys::Element>().ok()) else { return; };
                // Without capture an outside release could leave an action held.
                if target.set_pointer_capture(event.pointer_id()).is_err() { return; }
                let mut held = pointers.get_untracked();
                if !held.contains(&event.pointer_id()) {
                    if held.is_empty() {
                        if vibration.get_untracked() {
                            if let Some(window) = web_sys::window() {
                                // Safari and some desktop browsers omit this API entirely.
                                let navigator = window.navigator();
                                if let Ok(vibrate) = js_sys::Reflect::get(navigator.as_ref(), &"vibrate".into())
                                    .and_then(|value| value.dyn_into::<js_sys::Function>())
                                {
                                    let _ = vibrate.call1(navigator.as_ref(), &12.into());
                                }
                            }
                        }
                        runtime::touch_press(action);
                    }
                    held.push(event.pointer_id());
                    pointers.set(held);
                }
            }
            on:pointerup=finish
            on:pointercancel=finish
            on:lostpointercapture=finish
            on:click=move |event: web_sys::MouseEvent| {
                // Pointer actions ran on down; detail=0 is keyboard/AT activation.
                if event.detail() == 0 && enabled.get_untracked() {
                    runtime::touch_press(action);
                    runtime::touch_release(action);
                }
            }
        ><span aria-hidden="true">{symbol}</span><small>{label}</small></button>
    }
}

#[derive(Clone, Copy)]
struct Gesture {
    id: i32,
    x: f64,
    y: f64,
    steps: i32,
    max_x: f64,
    max_y: f64,
    started: f64,
    cell: f64,
}
fn gesture_action(action: Input) {
    runtime::touch_press(action);
    runtime::touch_release(action);
}
/// Window listeners survive renderer replacement, but only capture the local board.
#[component]
fn GestureControls(ui: Ui, enabled: Signal<bool>) -> impl IntoView {
    use wasm_bindgen::closure::Closure;
    let active = RwSignal::new(None::<Gesture>);
    let allowed = Signal::derive(move || enabled.get() && ui.settings.with(|s| s.touch_gestures));
    Effect::new(move |_| {
        let on = allowed.get();
        if !on {
            active.set(None);
        }
        if let Some(board) = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.get_element_by_id("my-board"))
        {
            let _ = board.class_list().toggle_with_force("gesture-active", on);
        }
    });
    let down = Closure::<dyn FnMut(web_sys::PointerEvent)>::new(move |e: web_sys::PointerEvent| {
        if !allowed.get_untracked() || e.button() != 0 {
            return;
        }
        let Some(board) = e
            .target()
            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
            .and_then(|t| t.closest("#my-board").ok().flatten())
        else {
            return;
        };
        e.prevent_default();
        if active.get_untracked().is_some() {
            active.set(None);
            return;
        }
        let _ = board.set_pointer_capture(e.pointer_id());
        active.set(Some(Gesture {
            id: e.pointer_id(),
            x: e.client_x() as f64,
            y: e.client_y() as f64,
            steps: 0,
            max_x: 0.,
            max_y: 0.,
            started: js_sys::Date::now(),
            cell: (board.client_width() as f64 / 10.).max(14.),
        }));
    });
    let movement =
        Closure::<dyn FnMut(web_sys::PointerEvent)>::new(move |e: web_sys::PointerEvent| {
            let Some(mut g) = active.get_untracked() else {
                return;
            };
            if !allowed.get_untracked() {
                active.set(None);
                return;
            }
            if e.pointer_id() != g.id {
                return;
            }
            e.prevent_default();
            let dx = e.client_x() as f64 - g.x;
            g.max_x = g.max_x.max(dx.abs());
            g.max_y = g.max_y.max((e.client_y() as f64 - g.y).abs());
            let steps = (dx / g.cell).trunc() as i32;
            let delta = (steps - g.steps).clamp(-10, 10);
            for _ in 0..delta.unsigned_abs() {
                gesture_action(if delta > 0 { Input::Right } else { Input::Left });
            }
            g.steps = steps;
            active.set(Some(g));
        });
    let up = Closure::<dyn FnMut(web_sys::PointerEvent)>::new(move |e: web_sys::PointerEvent| {
        let Some(g) = active.get_untracked() else {
            return;
        };
        if e.pointer_id() != g.id {
            return;
        }
        active.set(None);
        if !allowed.get_untracked() {
            return;
        }
        e.prevent_default();
        let dy = e.client_y() as f64 - g.y;
        let max_x = g.max_x.max((e.client_x() as f64 - g.x).abs());
        if dy >= (g.cell * 2.5).max(56.) && dy > max_x * 1.4 {
            gesture_action(Input::HardDrop);
        } else if dy.abs() < 10.
            && max_x < 10.
            && g.max_y < 10.
            && js_sys::Date::now() - g.started < 450.
        {
            gesture_action(Input::RotateCw);
        }
    });
    let cancel = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| active.set(None));
    let listeners = StoredValue::new_local((down, movement, up, cancel));
    listeners.with_value(|(down, movement, up, cancel)| {
        if let Some(w) = web_sys::window() {
            let _ =
                w.add_event_listener_with_callback("pointerdown", down.as_ref().unchecked_ref());
            let _ = w
                .add_event_listener_with_callback("pointermove", movement.as_ref().unchecked_ref());
            let _ = w.add_event_listener_with_callback("pointerup", up.as_ref().unchecked_ref());
            for event in [
                "pointercancel",
                "lostpointercapture",
                "blur",
                "visibilitychange",
            ] {
                let _ = w.add_event_listener_with_callback(event, cancel.as_ref().unchecked_ref());
            }
        }
    });
    on_cleanup(move || {
        listeners.with_value(|(down, movement, up, cancel)| {
            if let Some(w) = web_sys::window() {
                let _ = w.remove_event_listener_with_callback(
                    "pointerdown",
                    down.as_ref().unchecked_ref(),
                );
                let _ = w.remove_event_listener_with_callback(
                    "pointermove",
                    movement.as_ref().unchecked_ref(),
                );
                let _ =
                    w.remove_event_listener_with_callback("pointerup", up.as_ref().unchecked_ref());
                for event in [
                    "pointercancel",
                    "lostpointercapture",
                    "blur",
                    "visibilitychange",
                ] {
                    let _ = w.remove_event_listener_with_callback(
                        event,
                        cancel.as_ref().unchecked_ref(),
                    );
                }
                if let Some(board) = w.document().and_then(|d| d.get_element_by_id("my-board")) {
                    let _ = board.class_list().remove_1("gesture-active");
                }
            }
        });
        listeners.dispose();
    });
    ()
}
