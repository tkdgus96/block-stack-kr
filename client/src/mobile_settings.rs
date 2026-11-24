//! Mobile control preferences; all updates start from the latest shared settings.
use crate::runtime::{self, Ui};
use crate::settings::Settings;
use leptos::prelude::*;

fn update(ui: Ui, change: impl FnOnce(&mut Settings)) {
    ui.settings.update(change);
    runtime::apply_settings(ui.settings.get_untracked());
}

#[component]
pub(crate) fn MobileSettings(ui: Ui) -> impl IntoView {
    view! {
        <section class="mobile-settings" aria-labelledby="mobile-settings-heading">
            <h3 id="mobile-settings-heading" class="sheet-h">"모바일 터치 조작"</h3>
            <p class="muted small">"휴대폰 화면과 손에 맞게 조작 버튼을 바꾸세요. 설정은 이 브라우저에 저장됩니다."</p>
            <div class="mobile-settings-grid">
                <label class="mobile-setting">
                    <span>"버튼 크기 "<output>{move || ui.settings.with(|s| format!("{}%", s.touch_size))}</output></span>
                    <input type="range" aria-label="터치 버튼 크기" min="80" max="125" step="5"
                        prop:value=move || ui.settings.with(|s| s.touch_size.to_string())
                        on:input=move |ev| { let value = event_target_value(&ev).parse::<u8>().unwrap_or(100).clamp(80, 125); update(ui, |s| s.touch_size = value); }/>
                </label>
                <label class="mobile-setting">
                    <span>"버튼 불투명도 "<output>{move || ui.settings.with(|s| format!("{}%", s.touch_opacity))}</output></span>
                    <input type="range" aria-label="터치 버튼 불투명도" min="40" max="100" step="5"
                        prop:value=move || ui.settings.with(|s| s.touch_opacity.to_string())
                        on:input=move |ev| { let value = event_target_value(&ev).parse::<u8>().unwrap_or(100).clamp(40, 100); update(ui, |s| s.touch_opacity = value); }/>
                </label>
                <label class="mobile-setting mobile-setting-check">
                    <input type="checkbox" prop:checked=move || ui.settings.with(|s| s.touch_left_handed)
                        on:change=move |ev| { let value = event_target_checked(&ev); update(ui, |s| s.touch_left_handed = value); }/>
                    <span>"왼손 모드"<small>"바로 내리기를 왼쪽에 배치합니다. 이동 방향은 그대로 유지합니다."</small></span>
                </label>
                <label class="mobile-setting">
                    <span>"버튼 배치"</span>
                    <select aria-label="터치 버튼 배치" prop:value=move || ui.settings.with(|s| s.touch_layout.to_string())
                        on:change=move |ev| { let value = event_target_value(&ev).parse::<u8>().unwrap_or(0).min(1); update(ui, |s| s.touch_layout = value); }>
                        <option value="0" selected=move || ui.settings.with(|s| s.touch_layout == 0)>"기본 · 회전과 보관이 위"</option>
                        <option value="1" selected=move || ui.settings.with(|s| s.touch_layout == 1)>"이동 버튼이 위"</option>
                    </select>
                </label>
                <label class="mobile-setting mobile-setting-check">
                    <input type="checkbox" prop:checked=move || ui.settings.with(|s| s.touch_vibration)
                        on:change=move |ev| { let value = event_target_checked(&ev); update(ui, |s| s.touch_vibration = value); }/>
                    <span>"짧은 진동"<small>"버튼을 처음 누를 때만 진동합니다. 지원하는 기기에서 동작합니다."</small></span>
                </label>
                <label class="mobile-setting mobile-setting-check">
                    <input type="checkbox" aria-label="보드 제스처 조작" prop:checked=move || ui.settings.with(|s| s.touch_gestures)
                        on:change=move |ev| { let value=event_target_checked(&ev); update(ui, |s|s.touch_gestures=value); }/>
                    <span>"보드 제스처 조작"<small>"보드 탭: 오른쪽 회전 · 좌우 드래그: 이동 · 아래로 길게 스와이프 후 놓기: 바로 고정. 기본 버튼도 함께 사용할 수 있습니다."</small></span>
                </label>
            </div>
            <div class="mobile-touch-preview" aria-label="터치 버튼 배치 미리보기"
                class:touch-left-handed=move || ui.settings.with(|s| s.touch_left_handed)
                class:touch-movement-first=move || ui.settings.with(|s| s.touch_layout == 1)
                style=move || ui.settings.with(|s| format!("--touch-scale:{};--touch-opacity:{}", s.touch_size.clamp(80, 125) as f32 / 100.0, s.touch_opacity.clamp(40, 100) as f32 / 100.0))>
                <small class="mobile-preview-label">"버튼 미리보기"</small>
                <div class="touch-actions" aria-hidden="true"><span class="touch-hold">"HOLD"</span><span>"↶"</span><span>"↷"</span><span class="touch-drop">"⤓"</span></div>
                <div class="touch-movement" aria-hidden="true"><span>"◀"</span><span>"▼"</span><span>"▶"</span></div>
            </div>
        </section>
    }
}
