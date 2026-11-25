//! 게임 서버 HTTP API 호출. 웹 화면과 게임 서버 주소가 다를 수 있으므로 BS_WS_URL 에서 주소를 만든다.
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

/// `/api/...` 경로를 게임 서버 주소로 바꾼다.
pub fn url(path: &str) -> String {
    if let Some(ws) = option_env!("BS_WS_URL").filter(|s| !s.is_empty()) {
        let base = ws.replacen("wss://", "https://", 1).replacen("ws://", "http://", 1);
        if let Some(base) = base.strip_suffix("/ws") {
            return format!("{base}{path}");
        }
    }
    path.into()
}

async fn send(path: &str, method: &str, token: Option<&str>, body: Option<&[u8]>) -> Result<web_sys::Response, u16> {
    let options = web_sys::RequestInit::new();
    options.set_method(method);
    if let Some(body) = body {
        options.set_body(&js_sys::Uint8Array::from(body).into());
    }
    let req = web_sys::Request::new_with_str_and_init(&url(path), &options).map_err(|_| 0u16)?;
    if let Some(token) = token {
        req.headers().set("Authorization", &format!("Bearer {token}")).map_err(|_| 0u16)?;
    }
    if body.is_some() {
        req.headers().set("Content-Type", "application/octet-stream").map_err(|_| 0u16)?;
    }
    let window = web_sys::window().ok_or(0u16)?;
    let response: web_sys::Response = JsFuture::from(window.fetch_with_request(&req))
        .await
        .map_err(|_| 0u16)?
        .dyn_into()
        .map_err(|_| 0u16)?;
    if response.ok() { Ok(response) } else { Err(response.status()) }
}

/// 응답 본문을 바이트로 받는다. 실패하면 HTTP 상태 코드(연결 실패는 0).
pub async fn get_bytes(path: &str) -> Result<Vec<u8>, u16> {
    let response = send(path, "GET", None, None).await?;
    let buffer = JsFuture::from(response.array_buffer().map_err(|_| 0u16)?).await.map_err(|_| 0u16)?;
    Ok(js_sys::Uint8Array::new(&buffer).to_vec())
}

pub async fn get_text(path: &str) -> Result<String, u16> {
    let response = send(path, "GET", None, None).await?;
    JsFuture::from(response.text().map_err(|_| 0u16)?).await.map_err(|_| 0u16)?.as_string().ok_or(0)
}

pub async fn post_bytes(path: &str, token: &str, body: &[u8]) -> Result<String, u16> {
    let response = send(path, "POST", Some(token), Some(body)).await?;
    JsFuture::from(response.text().map_err(|_| 0u16)?).await.map_err(|_| 0u16)?.as_string().ok_or(0)
}

/// 현재 주소의 쿼리 값
pub fn query(name: &str) -> Option<String> {
    let search = web_sys::window()?.location().search().ok()?;
    web_sys::UrlSearchParams::new_with_str(&search).ok()?.get(name)
}
