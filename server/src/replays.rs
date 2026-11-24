//! 끝난 경기의 전체 리플레이 저장과 제공.
//!
//! 파일은 `BS_DATA_DIR/replays/<id>.bin` (postcard) 이고, 가장 최근 `KEEP` 개만 남긴다.
//! 리플레이에는 닉네임과 입력만 있고 복구 코드는 없다.
use axum::{
    extract::Path as UrlPath,
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use bs_engine::protocol::{decode, encode};
use bs_engine::replay::MatchReplay;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

const KEEP: usize = 500;
/// 6명 × 3분 경기도 수십 KB 를 넘지 않는다. 그보다 크면 저장하지 않는다.
const MAX_BYTES: usize = 2 * 1024 * 1024;

static LOCK: Mutex<()> = Mutex::new(());
static ROOT: OnceLock<Option<PathBuf>> = OnceLock::new();

fn root() -> Option<&'static Path> {
    ROOT.get_or_init(|| {
        let dir = PathBuf::from(std::env::var("BS_DATA_DIR").unwrap_or_else(|_| "data".into())).join("replays");
        std::fs::create_dir_all(&dir).ok().map(|_| dir)
    })
    .as_deref()
}

/// 저장 폴더를 만들 수 없으면 리플레이 기능을 끈다(경기는 그대로 진행한다).
pub fn enabled() -> bool {
    root().is_some()
}

fn valid_id(id: &str) -> bool {
    id.len() == 16 && id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub fn save(replay: &MatchReplay) -> Result<(), String> {
    let dir = root().ok_or("replay storage unavailable")?;
    if !valid_id(&replay.id) {
        return Err("invalid replay id".into());
    }
    let bytes = encode(replay);
    if bytes.len() > MAX_BYTES {
        return Err("replay too large".into());
    }
    let _guard = LOCK.lock().map_err(|_| "replay lock poisoned")?;
    let file = dir.join(format!("{}.bin", replay.id));
    let temp = file.with_extension("tmp");
    std::fs::write(&temp, &bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&temp, &file).map_err(|e| e.to_string())?;
    prune(dir);
    Ok(())
}

fn prune(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut files: Vec<_> = entries
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|x| x == "bin"))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    if files.len() <= KEEP {
        return;
    }
    files.sort();
    for (_, path) in files.iter().take(files.len() - KEEP) {
        let _ = std::fs::remove_file(path);
    }
}

pub fn load(id: &str) -> Option<MatchReplay> {
    if !valid_id(id) {
        return None;
    }
    let bytes = std::fs::read(root()?.join(format!("{id}.bin"))).ok()?;
    decode(&bytes)
}

async fn fetch(headers: HeaderMap, UrlPath(id): UrlPath<String>) -> Response {
    let found = tokio::task::spawn_blocking(move || load(&id).map(|r| encode(&r))).await.ok().flatten();
    let mut response = match found {
        Some(bytes) => ([(header::CONTENT_TYPE, "application/octet-stream")], bytes).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    };
    if let Some(origin) = headers.get(header::ORIGIN).filter(|o| o.as_bytes() == b"https://block-stack-kr.pages.dev") {
        response.headers_mut().insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin.clone());
    }
    response.headers_mut().insert(header::VARY, "Origin".parse().unwrap());
    // 리플레이는 저장 뒤 바뀌지 않는다.
    response.headers_mut().insert(header::CACHE_CONTROL, "public, max-age=86400".parse().unwrap());
    response
}

pub fn router<S: Clone + Send + Sync + 'static>() -> Router<S> {
    Router::new().route("/api/replay/{id}", get(fetch))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_cannot_escape_the_folder() {
        for bad in ["../../etc/passwd", "ABCDEF0123456789", "0123", "0123456789abcdef0"] {
            assert!(!valid_id(bad), "{bad}");
            assert!(load(bad).is_none());
        }
        assert!(valid_id("0123456789abcdef"));
    }
}
