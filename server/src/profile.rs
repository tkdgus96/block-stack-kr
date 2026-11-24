//! Durable bearer-code profiles. Only authoritative room results update progression.
use axum::{
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

static LOCK: Mutex<()> = Mutex::new(());
static ROOT: OnceLock<PathBuf> = OnceLock::new();
fn root() -> &'static Path {
    ROOT.get_or_init(|| {
        PathBuf::from(std::env::var("BS_DATA_DIR").unwrap_or_else(|_| "data".into()))
            .join("profiles")
    })
}
pub fn initialize() -> Result<(), String> {
    std::fs::create_dir_all(root()).map_err(|e| e.to_string())?;
    let probe = root().join(".write-test");
    std::fs::write(&probe, b"")
        .and_then(|_| std::fs::remove_file(probe))
        .map_err(|e| e.to_string())
}
pub fn valid_token(token: &str) -> bool {
    token.len() == 64
        && token
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn key(token: &str) -> String {
    format!("{:x}", Sha256::digest(token.as_bytes()))
}
fn path(token: &str) -> PathBuf {
    root().join(format!("{}.json", key(token)))
}
fn day() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        / 86400
}
#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct Profile {
    pub rounds: u64,
    pub sent: u64,
    pub wins: u64,
    pub day: u64,
    pub daily_rounds: u32,
    pub daily_sent: u32,
    pub daily_wins: u32,
    pub completed_missions: u64,
    pub themes: Vec<String>,
    #[serde(default)]
    recent_matches: Vec<String>,
    #[serde(default)]
    daily_claimed: u8,
}
impl Profile {
    fn refresh(&mut self, today: u64) {
        if self.day != today {
            self.day = today;
            self.daily_rounds = 0;
            self.daily_sent = 0;
            self.daily_wins = 0;
            self.daily_claimed = 0;
        }
    }
    fn record(&mut self, match_id: &str, sent: u32, won: bool, today: u64) -> bool {
        if self.recent_matches.iter().any(|m| m == match_id) {
            return false;
        }
        self.refresh(today);
        self.rounds = self.rounds.saturating_add(1);
        self.sent = self.sent.saturating_add(sent as u64);
        self.wins = self.wins.saturating_add(won as u64);
        self.daily_rounds = self.daily_rounds.saturating_add(1);
        self.daily_sent = self.daily_sent.saturating_add(sent);
        self.daily_wins = self.daily_wins.saturating_add(won as u32);
        for (bit, completed) in [
            (1, self.daily_rounds >= 3),
            (2, self.daily_sent >= 20),
            (4, self.daily_wins >= 1),
        ] {
            if completed && self.daily_claimed & bit == 0 {
                self.daily_claimed |= bit;
                self.completed_missions = self.completed_missions.saturating_add(1);
            }
        }
        for (threshold, theme) in [(1, "ocean"), (3, "sunset"), (9, "violet")] {
            if self.completed_missions >= threshold && !self.themes.iter().any(|t| t == theme) {
                self.themes.push(theme.into());
            }
        }
        self.recent_matches.push(match_id.into());
        if self.recent_matches.len() > 128 {
            self.recent_matches.remove(0);
        }
        true
    }
}
fn read_at(file: &Path) -> Result<Profile, String> {
    if std::fs::metadata(file).map_err(|e| e.to_string())?.len() > 64 * 1024 {
        return Err("profile exceeds limit".into());
    }
    serde_json::from_slice(&std::fs::read(file).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}
fn write_at(file: &Path, profile: &Profile) -> Result<(), String> {
    use std::io::Write;
    let parent = file.parent().ok_or("missing parent")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let temp = file.with_extension("tmp");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut out = options.open(&temp).map_err(|e| e.to_string())?;
    out.write_all(&serde_json::to_vec(profile).map_err(|e| e.to_string())?)
        .and_then(|_| out.sync_all())
        .map_err(|e| e.to_string())?;
    std::fs::rename(temp, file).map_err(|e| e.to_string())?;
    std::fs::File::open(parent)
        .and_then(|d| d.sync_all())
        .map_err(|e| e.to_string())
}
pub fn exists(token: &str) -> bool {
    valid_token(token) && path(token).is_file()
}
/// One recovered identity may occupy multiple seats. Merge before persistence so
/// thread scheduling cannot decide its result: best attack and any eligible win.
pub fn merged_results(
    summary: &bs_engine::protocol::MatchSummary,
    profiles: &std::collections::HashMap<u8, String>,
) -> std::collections::HashMap<String, (u32, bool)> {
    let mut results = std::collections::HashMap::<String, (u32, bool)>::new();
    for row in &summary.players {
        if let Some(token) = profiles.get(&row.id) {
            let won = summary.winners.contains(&row.id)
                && if summary.teams {
                    summary
                        .players
                        .iter()
                        .filter(|p| summary.winners.contains(&p.id))
                        .all(|p| p.team == row.team)
                } else {
                    summary.winners.len() == 1
                };
            let entry = results.entry(token.clone()).or_default();
            entry.0 = entry.0.max(row.sent);
            entry.1 |= won;
        }
    }
    results
}
pub fn record_match(token: &str, match_id: &str, sent: u32, won: bool) -> Result<(), String> {
    if !valid_token(token) || match_id.len() > 120 {
        return Err("invalid profile or match id".into());
    }
    let _guard = LOCK.lock().map_err(|_| "profile lock poisoned")?;
    let file = path(token);
    let mut profile = read_at(&file)?;
    if profile.record(match_id, sent, won, day()) {
        write_at(&file, &profile)?;
    }
    Ok(())
}
fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .filter(|t| valid_token(t))
}
fn cors(headers: &HeaderMap, mut response: Response) -> Response {
    if let Some(origin) = headers
        .get(header::ORIGIN)
        .filter(|o| o.as_bytes() == b"https://block-stack-kr.pages.dev")
    {
        response
            .headers_mut()
            .insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin.clone());
    }
    response
        .headers_mut()
        .insert(header::VARY, "Origin".parse().unwrap());
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    response
}
async fn handle(headers: HeaderMap, create: bool) -> Response {
    let response = tokio::task::spawn_blocking({
        let token = bearer(&headers).map(str::to_owned);
        move || {
            let Some(token) = token else {
                return StatusCode::UNAUTHORIZED.into_response();
            };
            let Ok(_guard) = LOCK.lock() else {
                return StatusCode::SERVICE_UNAVAILABLE.into_response();
            };
            let file = path(&token);
            if !file.exists() {
                if !create {
                    return StatusCode::NOT_FOUND.into_response();
                }
                // A hard storage cap prevents unbounded anonymous profile allocation.
                if std::fs::read_dir(root())
                    .map(|entries| entries.filter_map(Result::ok).take(10_001).count() >= 10_000)
                    .unwrap_or(false)
                {
                    return StatusCode::SERVICE_UNAVAILABLE.into_response();
                }
                if write_at(
                    &file,
                    &Profile {
                        day: day(),
                        ..Default::default()
                    },
                )
                .is_err()
                {
                    return StatusCode::SERVICE_UNAVAILABLE.into_response();
                }
            }
            match read_at(&file) {
                Ok(mut p) => {
                    p.refresh(day());
                    Json(p).into_response()
                }
                Err(_) => StatusCode::SERVICE_UNAVAILABLE.into_response(),
            }
        }
    })
    .await
    .unwrap_or_else(|_| StatusCode::SERVICE_UNAVAILABLE.into_response());
    cors(&headers, response)
}
async fn fetch(headers: HeaderMap) -> Response {
    handle(headers, false).await
}
async fn create(headers: HeaderMap) -> Response {
    handle(headers, true).await
}
async fn options(headers: HeaderMap) -> Response {
    let mut response = cors(&headers, StatusCode::NO_CONTENT.into_response());
    response.headers_mut().insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        "GET, POST, OPTIONS".parse().unwrap(),
    );
    response.headers_mut().insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        "Authorization".parse().unwrap(),
    );
    response
}
pub fn router<S: Clone + Send + Sync + 'static>() -> Router<S> {
    Router::new().route("/api/profile", get(fetch).post(create).options(options))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repeated_identity_merges_independent_of_seat_order_and_excludes_draws() {
        use bs_engine::protocol::{MatchSummary, RoundPlayer};
        let mut summary = MatchSummary {
            winners: vec![2],
            players: vec![
                RoundPlayer {
                    id: 1,
                    sent: 45,
                    team: 0,
                    ..Default::default()
                },
                RoundPlayer {
                    id: 2,
                    sent: 20,
                    team: 1,
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let profiles = [(1, "identity".to_string()), (2, "identity".to_string())]
            .into_iter()
            .collect();
        assert_eq!(merged_results(&summary, &profiles)["identity"], (45, true));
        summary.players.reverse();
        assert_eq!(merged_results(&summary, &profiles)["identity"], (45, true));
        summary.winners = vec![1, 2];
        assert_eq!(merged_results(&summary, &profiles)["identity"], (45, false));
        summary.teams = true;
        assert_eq!(merged_results(&summary, &profiles)["identity"], (45, false));
        summary.winners = vec![2];
        assert_eq!(merged_results(&summary, &profiles)["identity"], (45, true));
    }
    #[test]
    fn bearer_auth_requires_exact_secret_and_cors_is_allowlisted() {
        let mut headers = HeaderMap::new();
        assert!(bearer(&headers).is_none());
        headers.insert(header::AUTHORIZATION, "Bearer short".parse().unwrap());
        assert!(bearer(&headers).is_none());
        headers.insert(
            header::AUTHORIZATION,
            format!("Bearer {}", "a".repeat(64)).parse().unwrap(),
        );
        assert!(bearer(&headers).is_some());
        headers.insert(header::ORIGIN, "https://evil.example".parse().unwrap());
        let denied = cors(&headers, StatusCode::OK.into_response());
        assert!(!denied
            .headers()
            .contains_key(header::ACCESS_CONTROL_ALLOW_ORIGIN));
        headers.insert(
            header::ORIGIN,
            "https://block-stack-kr.pages.dev".parse().unwrap(),
        );
        let allowed = cors(&headers, StatusCode::OK.into_response());
        assert_eq!(
            allowed.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN],
            "https://block-stack-kr.pages.dev"
        );
        assert_eq!(allowed.headers()[header::CACHE_CONTROL], "no-store");
    }
    #[test]
    fn missions_reset_dedupe_and_unlock() {
        let mut p = Profile::default();
        assert!(p.record("a", 20, true, 100));
        assert_eq!(p.completed_missions, 2);
        assert!(!p.record("a", 999, true, 100));
        p.record("b", 0, false, 100);
        p.record("c", 0, false, 100);
        assert_eq!(p.completed_missions, 3);
        assert!(p.themes.iter().any(|x| x == "sunset"));
        p.record("d", 0, false, 101);
        assert_eq!((p.daily_rounds, p.daily_sent, p.daily_wins), (1, 0, 0));
        assert_eq!(p.rounds, 4);
    }
    #[test]
    fn tokens_cannot_traverse_or_be_guessed_short() {
        assert!(!valid_token("../etc/passwd"));
        assert!(!valid_token(&"a".repeat(63)));
        assert!(valid_token(&"a".repeat(64)));
        assert!(!valid_token(&"A".repeat(64)));
    }
    #[test]
    fn atomic_roundtrip_and_corruption_is_not_overwritten() {
        let dir = std::env::temp_dir().join(format!(
            "bs-profile-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let file = dir.join("profile.json");
        let mut p = Profile::default();
        p.record("a", 30, true, 1);
        write_at(&file, &p).unwrap();
        assert_eq!(read_at(&file).unwrap().sent, 30);
        std::fs::write(&file, "broken").unwrap();
        assert!(read_at(&file).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
