//! 연습 모드(40줄, 2분 도전, 마라톤) 서버 순위표.
//!
//! 클라이언트는 시드와 입력 기록만 보낸다. 서버가 같은 엔진으로 처음부터 다시 실행해 나온
//! 기록만 저장하므로, 화면에 표시된 숫자를 고쳐 보내는 방법으로는 순위에 오를 수 없다.
//! 프로필(복구 코드)이 있는 사람만 등록할 수 있고, 프로필마다 가장 좋은 기록 하나만 남긴다.
use axum::{
    body::Bytes,
    extract::Path as UrlPath,
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use bs_engine::practice::Session;
use bs_engine::protocol::{decode, LeaderboardSubmission, RankedMode, MAX_NAME_LEN};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::Mutex;

const KEEP: usize = 100;
const SHOW: usize = 20;
const MAX_BODY: usize = 1024 * 1024;

static LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Entry {
    pub name: String,
    /// 40줄: 걸린 tick (작을수록 좋음), 그 외: 점수 (클수록 좋음)
    pub value: u32,
    #[serde(skip_serializing, default)]
    owner: String,
}

fn file(mode: RankedMode) -> PathBuf {
    PathBuf::from(std::env::var("BS_DATA_DIR").unwrap_or_else(|_| "data".into()))
        .join("leaderboard")
        .join(format!("{}.json", mode.slug()))
}

fn better(mode: RankedMode, a: u32, b: u32) -> bool {
    if mode == RankedMode::Sprint { a < b } else { a > b }
}

fn read(mode: RankedMode) -> Vec<Entry> {
    std::fs::read(file(mode)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

/// 기록을 넣고 정렬한다. 새 기록이 목록에 남았으면 true. 자기 최고 기록보다 못하거나 순위 밖이면 false.
fn insert(mode: RankedMode, entries: &mut Vec<Entry>, entry: Entry) -> bool {
    let (owner, value) = (entry.owner.clone(), entry.value);
    if let Some(old) = entries.iter_mut().find(|e| e.owner == entry.owner) {
        if !better(mode, entry.value, old.value) {
            return false;
        }
        *old = entry;
    } else {
        entries.push(entry);
    }
    entries.sort_by(|a, b| if mode == RankedMode::Sprint { a.value.cmp(&b.value) } else { b.value.cmp(&a.value) });
    entries.truncate(KEEP);
    entries.iter().any(|e| e.owner == owner && e.value == value)
}

/// 다시 실행해 확인한 기록. 끝까지 하지 않았거나 실패한 기록은 `None`.
pub fn verified_value(sub: &LeaderboardSubmission) -> Option<u32> {
    let session = Session::verify(sub.mode.practice(), sub.seed, &sub.log, sub.end_tick)?;
    session.record_value()
}

fn clean_name(name: &str) -> String {
    let name: String = name.trim().chars().filter(|c| !c.is_control()).take(MAX_NAME_LEN).collect();
    if name.is_empty() { "플레이어".into() } else { name }
}

/// 확인한 기록 값과, 그 기록이 순위표에 올라갔는지
fn submit(token: &str, sub: LeaderboardSubmission) -> Result<(u32, bool), StatusCode> {
    if !crate::profile::exists(token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let value = verified_value(&sub).ok_or(StatusCode::UNPROCESSABLE_ENTITY)?;
    let owner = format!("{:x}", Sha256::digest(token.as_bytes()));
    let _guard = LOCK.lock().map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    let mut entries = read(sub.mode);
    let stored = insert(sub.mode, &mut entries, Entry { name: clean_name(&sub.name), value, owner });
    if stored {
        let path = file(sub.mode);
        std::fs::create_dir_all(path.parent().unwrap()).map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        let temp = path.with_extension("tmp");
        let bytes = serde_json::to_vec(&entries.iter().map(|e| StoredEntry::from(e.clone())).collect::<Vec<_>>())
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        std::fs::write(&temp, bytes).and_then(|_| std::fs::rename(&temp, &path)).map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    }
    Ok((value, stored))
}

/// 파일에는 owner 까지 저장한다 (응답에는 넣지 않는다).
#[derive(Serialize)]
struct StoredEntry {
    name: String,
    value: u32,
    owner: String,
}
impl From<Entry> for StoredEntry {
    fn from(e: Entry) -> Self {
        StoredEntry { name: e.name, value: e.value, owner: e.owner }
    }
}

fn mode_of(slug: &str) -> Option<RankedMode> {
    RankedMode::ALL.into_iter().find(|m| m.slug() == slug)
}

fn cors(headers: &HeaderMap, mut response: Response) -> Response {
    if let Some(origin) = headers.get(header::ORIGIN).filter(|o| o.as_bytes() == b"https://block-stack-kr.pages.dev") {
        response.headers_mut().insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin.clone());
    }
    response.headers_mut().insert(header::VARY, "Origin".parse().unwrap());
    response.headers_mut().insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    response
}

async fn list(headers: HeaderMap, UrlPath(slug): UrlPath<String>) -> Response {
    let Some(mode) = mode_of(&slug) else { return cors(&headers, StatusCode::NOT_FOUND.into_response()) };
    let entries = tokio::task::spawn_blocking(move || {
        let _guard = LOCK.lock();
        read(mode).into_iter().take(SHOW).collect::<Vec<_>>()
    })
    .await
    .unwrap_or_default();
    cors(&headers, Json(entries).into_response())
}

async fn create(headers: HeaderMap, body: Bytes) -> Response {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .filter(|t| crate::profile::valid_token(t))
        .map(str::to_owned);
    let response = match (token, decode::<LeaderboardSubmission>(&body)) {
        (None, _) => StatusCode::UNAUTHORIZED.into_response(),
        (_, None) => StatusCode::BAD_REQUEST.into_response(),
        (Some(token), Some(sub)) => {
            match tokio::task::spawn_blocking(move || submit(&token, sub)).await {
                Ok(Ok((value, stored))) => Json(serde_json::json!({ "value": value, "stored": stored })).into_response(),
                Ok(Err(code)) => code.into_response(),
                Err(_) => StatusCode::SERVICE_UNAVAILABLE.into_response(),
            }
        }
    };
    cors(&headers, response)
}

async fn options(headers: HeaderMap) -> Response {
    let mut response = cors(&headers, StatusCode::NO_CONTENT.into_response());
    response.headers_mut().insert(header::ACCESS_CONTROL_ALLOW_METHODS, "GET, POST, OPTIONS".parse().unwrap());
    response.headers_mut().insert(header::ACCESS_CONTROL_ALLOW_HEADERS, "Authorization, Content-Type".parse().unwrap());
    response
}

pub fn router<S: Clone + Send + Sync + 'static>() -> Router<S> {
    Router::new()
        .route("/api/leaderboard", post(create).options(options))
        .route("/api/leaderboard/{mode}", get(list))
        .layer(axum::extract::DefaultBodyLimit::max(MAX_BODY))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bs_engine::practice::{Mode, Outcome};
    use bs_engine::Input;

    fn entry(owner: &str, value: u32) -> Entry {
        Entry { name: owner.into(), value, owner: owner.into() }
    }

    #[test]
    fn keeps_one_best_entry_per_profile_in_mode_order() {
        let mut sprint = Vec::new();
        assert!(insert(RankedMode::Sprint, &mut sprint, entry("a", 3000)));
        assert!(insert(RankedMode::Sprint, &mut sprint, entry("b", 2000)));
        assert!(!insert(RankedMode::Sprint, &mut sprint, entry("a", 3500)), "slower run does not replace");
        assert!(insert(RankedMode::Sprint, &mut sprint, entry("a", 1500)));
        assert_eq!(sprint.iter().map(|e| (e.owner.as_str(), e.value)).collect::<Vec<_>>(), vec![("a", 1500), ("b", 2000)]);
        let mut blitz = Vec::new();
        insert(RankedMode::Blitz, &mut blitz, entry("a", 100));
        insert(RankedMode::Blitz, &mut blitz, entry("b", 900));
        assert_eq!(blitz[0].owner, "b");
        // 목록이 가득 찼을 때 순위 밖으로 밀려나는 기록은 올라가지 않았다고 알린다.
        let mut full: Vec<Entry> = (0..KEEP).map(|i| entry(&format!("p{i}"), 1000 + i as u32)).collect();
        assert!(!insert(RankedMode::Sprint, &mut full, entry("late", 9000)));
        assert!(insert(RankedMode::Sprint, &mut full, entry("fast", 10)));
        // 응답 JSON 에는 owner 가 없다.
        assert!(!serde_json::to_string(&blitz).unwrap().contains("owner"));
    }

    #[test]
    fn only_replayed_finished_runs_count() {
        // 2분 도전을 끝까지 버틴 기록: 연습봇의 계획대로 2초마다 블록 하나를 놓는다.
        let mut s = Session::new(Mode::Blitz, 7);
        let mut t = 0;
        while s.outcome == Outcome::Playing && t < 7200 {
            s.advance_to(t);
            for input in bs_engine::bot::plan_with(&s.game, &bs_engine::bot::Style::DEFAULT) {
                s.press(input);
            }
            t += 120;
        }
        s.advance_to(7200);
        assert_eq!(s.outcome, Outcome::Success);
        assert!(s.log.iter().any(|&(_, i, _)| i != Input::HardDrop));
        let sub = LeaderboardSubmission { mode: RankedMode::Blitz, seed: 7, end_tick: 7200, name: "x".into(), log: s.log.clone() };
        assert_eq!(verified_value(&sub), s.record_value());
        assert!(verified_value(&sub).is_some(), "survived 2 minutes");
        // 끝나기 전에 보낸 기록, 다른 시드는 인정하지 않거나 다른 값이 나온다.
        assert_eq!(verified_value(&LeaderboardSubmission { end_tick: 7000, ..sub.clone() }), None);
        let mut sprint = LeaderboardSubmission { mode: RankedMode::Sprint, ..sub };
        sprint.end_tick = 100;
        assert_eq!(verified_value(&sprint), None, "40 lines not cleared");
    }

    #[test]
    fn names_are_cleaned_and_modes_parse() {
        assert_eq!(clean_name("  \u{7}abc\n "), "abc");
        assert_eq!(clean_name(""), "플레이어");
        assert_eq!(mode_of("marathon"), Some(RankedMode::Marathon));
        assert_eq!(mode_of("../x"), None);
    }
}
