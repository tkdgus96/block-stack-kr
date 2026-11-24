//! Optional anonymous server profile, recovered with a secret bearer code.
use bs_engine::protocol::ClientMsg;
use leptos::prelude::*;
use serde::Deserialize;
use wasm_bindgen::{closure::Closure, JsCast};
thread_local! { static SESSION_TOKEN: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) }; }
const KEY: &str = "bs-profile-token-v1";
const THEME: &str = "bs-profile-theme-v1";
#[derive(Clone, Default, Deserialize)]
struct Profile {
    rounds: u64,
    sent: u64,
    wins: u64,
    daily_rounds: u32,
    daily_sent: u32,
    daily_wins: u32,
    completed_missions: u64,
    themes: Vec<String>,
}
pub fn token() -> Option<String> {
    SESSION_TOKEN.with(|s| s.borrow().clone()).or_else(|| {
        web_sys::window()?
            .local_storage()
            .ok()??
            .get_item(KEY)
            .ok()?
            .filter(|t| valid(t))
    })
}
fn remember_token(value: &str) -> bool {
    SESSION_TOKEN.with(|s| *s.borrow_mut() = Some(value.into()));
    save(KEY, value)
}
fn valid(t: &str) -> bool {
    t.len() == 64
        && t.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn save(key: &str, value: &str) -> bool {
    web_sys::window()
        .and_then(|w| w.local_storage().ok().flatten())
        .is_some_and(|s| s.set_item(key, value).is_ok())
}
fn generate() -> Option<String> {
    let mut bytes = [0u8; 32];
    web_sys::window()?
        .crypto()
        .ok()?
        .get_random_values_with_u8_array(&mut bytes)
        .ok()?;
    Some(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
fn endpoint() -> String {
    if let Some(ws) = option_env!("BS_WS_URL").filter(|s| !s.is_empty()) {
        let url = ws
            .replacen("wss://", "https://", 1)
            .replacen("ws://", "http://", 1);
        if let Some(base) = url.strip_suffix("/ws") {
            return format!("{base}/api/profile");
        }
    }
    "/api/profile".into()
}
async fn request(t: &str, create: bool) -> Result<Profile, String> {
    let options = web_sys::RequestInit::new();
    options.set_method(if create { "POST" } else { "GET" });
    let req = web_sys::Request::new_with_str_and_init(&endpoint(), &options)
        .map_err(|_| "요청을 만들지 못했습니다")?;
    req.headers()
        .set("Authorization", &format!("Bearer {t}"))
        .map_err(|_| "인증 요청 오류")?;
    let response = wasm_bindgen_futures::JsFuture::from(
        web_sys::window()
            .ok_or("브라우저 없음")?
            .fetch_with_request(&req),
    )
    .await
    .map_err(|_| "프로필 서버에 연결하지 못했습니다")?
    .dyn_into::<web_sys::Response>()
    .map_err(|_| "응답 오류")?;
    if !response.ok() {
        return Err(if response.status() == 404 {
            "복구 코드를 찾을 수 없습니다"
        } else {
            "프로필을 불러오지 못했습니다. 잠시 후 다시 시도해 주세요"
        }
        .into());
    }
    let text = wasm_bindgen_futures::JsFuture::from(response.text().map_err(|_| "응답 오류")?)
        .await
        .map_err(|_| "응답 오류")?
        .as_string()
        .ok_or("응답 오류")?;
    serde_json::from_str(&text).map_err(|_| "프로필 형식 오류".into())
}
fn apply_theme(theme: &str) {
    if let Some(body) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.body())
    {
        let _ = body.set_attribute("data-profile-theme", theme);
    }
}
pub fn init_theme() {
    let theme = web_sys::window()
        .and_then(|w| w.local_storage().ok().flatten())
        .and_then(|s| s.get_item(THEME).ok().flatten())
        .unwrap_or_default();
    if ["", "ocean", "sunset", "violet"].contains(&theme.as_str()) {
        apply_theme(&theme);
    }
}
pub fn refresh() {
    // Room persistence runs off the simulation thread; allow the atomic write to finish.
    for delay in [500, 2000] {
        set_timeout(
            || {
                if let (Some(w), Ok(event)) =
                    (web_sys::window(), web_sys::Event::new("bs-profile-refresh"))
                {
                    let _ = w.dispatch_event(&event);
                }
            },
            std::time::Duration::from_millis(delay),
        );
    }
}
#[component]
pub fn ProfilePanel() -> impl IntoView {
    let profile = RwSignal::new(None::<Profile>);
    let status = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let reveal = RwSignal::new(false);
    let recovery = RwSignal::new(String::new());
    let load = move |import: Option<String>| {
        if import.is_some() && crate::runtime::in_match() {
            status.set(
                "경기 중에는 프로필을 바꿀 수 없습니다. 경기 종료 후 다시 시도해 주세요.".into(),
            );
            return;
        }
        if busy.get_untracked() {
            return;
        }
        let current = token();
        let importing = import.is_some();
        let Some(candidate) = import.or_else(|| current.clone()).or_else(generate) else {
            status.set("안전한 복구 코드를 생성하지 못했습니다".into());
            return;
        };
        if !valid(&candidate) {
            status.set("복구 코드는 소문자 영문·숫자 64자리입니다".into());
            return;
        }
        busy.set(true);
        status.set("프로필을 불러오는 중…".into());
        wasm_bindgen_futures::spawn_local(async move {
            let result = request(&candidate, !importing && current.is_none()).await;
            if profile.is_disposed() { return; }
            match result {
                Ok(p) => {
                    let persisted = remember_token(&candidate);
                    crate::runtime::send(ClientMsg::AttachProfile { token: candidate });
                    profile.set(Some(p));
                    status.set(if persisted {"서버 저장 완료 · 대전 결과는 경기 종료 후 반영됩니다"} else {"서버 저장 완료 · 브라우저 저장이 차단되어 이 탭에서만 유지됩니다. 아래 복구 코드를 꼭 보관하세요."}.into());
                    recovery.set(String::new());
                    reveal.set(false);
                }
                Err(e) => status.set(e),
            }
            busy.set(false);
        });
    };
    Effect::new(move |_| {
        init_theme();
        load(None);
    });
    let listener = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| load(None));
    if let Some(w) = web_sys::window() {
        let _ = w.add_event_listener_with_callback(
            "bs-profile-refresh",
            listener.as_ref().unchecked_ref(),
        );
    }
    let listener = StoredValue::new_local(listener);
    on_cleanup(move || {
        listener.with_value(|callback| {
            if let Some(w) = web_sys::window() {
                let _ = w.remove_event_listener_with_callback(
                    "bs-profile-refresh",
                    callback.as_ref().unchecked_ref(),
                );
            }
        });
        listener.dispose();
    });
    view! {
        <section class="profile-panel" aria-label="서버 프로필과 오늘의 미션">
            <h3>"서버 프로필 · 오늘의 미션"</h3>
            <p>"게스트로 바로 시작합니다. 복구 코드를 보관하면 다른 기기에서 같은 전적을 이어갈 수 있습니다. 정식 계정 로그인은 아닙니다."</p>
            <p class="profile-status" role="status">{move||status.get()}</p>
            <button on:click=move |_|load(None) disabled=move||busy.get()>"전적 새로고침"</button>
            {move||profile.get().map(|p|view!{
                <div class="profile-totals"><strong>{format!("{}경기 · {}승 · 공격 {}줄",p.rounds,p.wins,p.sent)}</strong></div>
                <p>"공개/비공개 대전만 집계합니다. 혼자 연습과 협동전은 제외됩니다. 미션은 매일 UTC 00시(한국 오전 9시)에 초기화됩니다."</p>
                <ul class="mission-list">
                    <li class:done={p.daily_rounds>=3}>{format!("{} 대전 3경기 완료 {}/3",if p.daily_rounds>=3 {"✓"}else{"○"},p.daily_rounds.min(3))}</li>
                    <li class:done={p.daily_sent>=20}>{format!("{} 공격 20줄 보내기 {}/20",if p.daily_sent>=20 {"✓"}else{"○"},p.daily_sent.min(20))}</li>
                    <li class:done={p.daily_wins>=1}>{format!("{} 대전 1승 {}/1",if p.daily_wins>=1 {"✓"}else{"○"},p.daily_wins.min(1))}</li>
                </ul>
                <p>{format!("누적 미션 {}개 · 미션 1/3/9개로 테마 해금",p.completed_missions)}</p>
                <div class="profile-themes"><button on:click=move |_|{save(THEME,"");apply_theme("");}>"기본 테마"</button>
                    {[ ("ocean","오션",1), ("sunset","선셋",3), ("violet","바이올렛",9) ].into_iter().map(|(id,label,need)| {
                        let unlocked=p.themes.iter().any(|x|x==id);
                        view!{<button disabled=!unlocked on:click=move |_|{save(THEME,id);apply_theme(id);}>{format!("{} {}",label,if unlocked {"✓".to_string()}else{format!("🔒 {need}개")})}</button>}
                    }).collect_view()}
                </div>
            })}
            <details class="profile-recovery"><summary>"기기 변경 · 복구 코드 관리"</summary>
                <p>"이 코드를 아는 사람은 전적을 사용할 수 있습니다. 공개 채팅이나 스크린샷에 공유하지 마세요. 분실 시 복구할 수 없습니다."</p>
                <button on:click=move |_|reveal.update(|r|*r=!*r)>{move||if reveal.get(){"코드 숨기기"}else{"내 복구 코드 표시"}}</button>
                <Show when=move||reveal.get()><code class="profile-secret">{move||token().unwrap_or_default()}</code></Show>
                <label>"다른 프로필 복구 코드"<input type="password" autocomplete="off" maxlength="64" prop:value=move||recovery.get() on:input=move|e|recovery.set(event_target_value(&e)) placeholder="64자리 복구 코드"/></label>
                <button disabled=move||busy.get() on:click=move|_|load(Some(recovery.get_untracked().trim().into()))>"이 프로필로 전환"</button>
            </details>
        </section>
    }
}
