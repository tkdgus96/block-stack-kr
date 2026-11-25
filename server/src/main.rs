//! Block Stack 서버: 정적 파일(WASM 클라이언트) 제공 + WebSocket 대전.

mod room;
mod profile;
mod replays;
mod matchmaking;
mod leaderboard;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::http::{header, HeaderValue};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use bs_engine::protocol::{decode, encode, ClientMsg, RoomConfig, RoomSummary, ServerMsg, MAX_NAME_LEN};
use futures_util::{SinkExt, StreamExt};
use room::{spawn_room, Outbox, Registry, RoomCmd, RoomEntry};

use tokio::sync::{mpsc, oneshot};
use tower_http::compression::CompressionLayer;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::set_header::SetResponseHeaderLayer;

#[tokio::main]
async fn main() {
    profile::initialize().expect("profile storage must be writable (BS_DATA_DIR)");
    let port: u16 = std::env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(3000);
    let dist = std::env::var("BS_DIST").unwrap_or_else(|_| "client/dist".into());
    let registry: Registry = Default::default();
    {
        let reg = registry.clone();
        tokio::spawn(async move {
            let mut every = tokio::time::interval(std::time::Duration::from_secs(1));
            loop {
                every.tick().await;
                matchmaking::sweep(&reg, std::time::Instant::now());
            }
        });
    }

    let static_files = ServeDir::new(&dist).fallback(ServeFile::new(format!("{dist}/index.html")));
    let app = Router::new()
        .route("/ws", get(ws_handler))
        .route("/healthz", get(|| async { "ok" }))
        .merge(profile::router())
        .merge(replays::router())
        .merge(leaderboard::router())
        .fallback_service(static_files)
        .layer(CompressionLayer::new())
        .layer(SetResponseHeaderLayer::if_not_present(header::CACHE_CONTROL, HeaderValue::from_static("no-cache")))
        .with_state(registry);

    let addr: std::net::SocketAddr = match std::env::var("BS_BIND") {
        Ok(bind) => bind.parse().expect("BS_BIND must be an IP address and port, e.g. 127.0.0.1:3010"),
        Err(_) => std::net::SocketAddr::from(([0, 0, 0, 0], port)),
    };
    let listener = tokio::net::TcpListener::bind(addr).await.expect("bind");
    println!("Block Stack 서버: http://{addr}  (정적 파일: {dist})");
    axum::serve(listener, app).await.unwrap();
}

async fn ws_handler(ws: WebSocketUpgrade, State(reg): State<Registry>) -> impl IntoResponse {
    ws.max_message_size(4096).on_upgrade(move |s| handle_socket(s, reg))
}

fn sanitize_room(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_alphanumeric()).take(8).collect::<String>().to_ascii_uppercase()
}

fn new_room_code(reg: &Registry) -> String {
    const CH: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut x = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(7);
    loop {
        let mut code = String::new();
        for _ in 0..5 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            code.push(CH[(x % CH.len() as u64) as usize] as char);
        }
        if !reg.lock().unwrap().contains_key(&code) {
            return code;
        }
    }
}

fn room_sender(reg: &Registry, code: &str) -> mpsc::Sender<RoomCmd> {
    let mut map = reg.lock().unwrap();
    match map.get(code) {
        Some(e) if !e.tx.is_closed() => e.tx.clone(),
        _ => {
            let tx = spawn_room(code.to_string(), reg.clone());
            map.insert(code.to_string(), RoomEntry { config: RoomConfig::default(), public: true, coop: false, tx: tx.clone(), summary: RoomSummary { code: code.to_string(), ..Default::default() } });
            tx
        }
    }
}

/// 현재 열린 방 목록: 사람이 있는 방만, 대기 중인 방 → 사람 많은 순
const MAX_LISTED_ROOMS: usize = 50;

fn listed_entries(reg: &Registry) -> Vec<(RoomSummary, RoomConfig)> {
    let map = reg.lock().unwrap();
    let mut entries: Vec<_> = map.values()
        .filter(|e| e.public && !e.tx.is_closed() && e.summary.humans > 0)
        .map(|e| (e.summary.clone(), e.config.clone())).collect();
    entries.sort_by(|(a, _), (b, _)| a.playing.cmp(&b.playing).then(b.humans.cmp(&a.humans)).then(a.code.cmp(&b.code)));
    entries.truncate(MAX_LISTED_ROOMS);
    entries
}

#[cfg(test)]
fn list_rooms(reg: &Registry) -> Vec<RoomSummary> {
    listed_entries(reg).into_iter().map(|(summary, _)| summary).collect()
}

fn room_rules_msg(entries: &[(RoomSummary, RoomConfig)]) -> axum::body::Bytes {
    axum::body::Bytes::from(encode(&ServerMsg::RoomRules(entries.iter().map(|(s, c)| (s.code.clone(), c.clone())).collect())))
}

fn rooms_msg(entries: &[(RoomSummary, RoomConfig)]) -> axum::body::Bytes {
    axum::body::Bytes::from(encode(&ServerMsg::Rooms(entries.iter().map(|(s, _)| s.clone()).collect())))
}

fn send_room_list(reg: &Registry, out: &Outbox) {
    let entries = listed_entries(reg);
    let coop: Vec<_> = { let map = reg.lock().unwrap(); entries.iter().filter(|(s,_)| map.get(&s.code).is_some_and(|e| e.coop)).map(|(s,_)| s.code.clone()).collect() };
    // Reserve all slots so a slow client receives one complete snapshot.
    if let Ok(mut permits) = out.try_reserve_many(3) {
        permits.next().unwrap().send(axum::body::Bytes::from(encode(&ServerMsg::CoopRooms(coop))));
        permits.next().unwrap().send(room_rules_msg(&entries));
        permits.next().unwrap().send(rooms_msg(&entries));
    }
}

static NEXT_CONN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// 방을 거치지 않는 메시지(방 목록, 빠른 대전)를 처리했으면 true
async fn handle_lobby_msg(reg: &Registry, conn: u64, out: &Outbox, msg: &ClientMsg) -> bool {
    match msg {
        ClientMsg::ListRooms => send_room_list(reg, out),
        ClientMsg::QuickMatch { token } => {
            let token = token.clone();
            let rating = tokio::task::spawn_blocking(move || profile::rating_of(&token)).await.unwrap_or(1500.0);
            matchmaking::enqueue(reg, conn, rating, out.clone());
        }
        ClientMsg::CancelQuickMatch => matchmaking::cancel(conn, out),
        // 입장 전에만 의미가 있다. 입장 뒤에 오면 버린다.
        ClientMsg::Hello { .. } => {}
        _ => return false,
    }
    true
}

async fn handle_socket(socket: WebSocket, reg: Registry) {
    let conn = NEXT_CONN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let (mut sink, mut stream) = socket.split();
    let (out_tx, mut out_rx): (Outbox, _) = mpsc::channel(256);

    // 송신 전용 태스크
    let writer = tokio::spawn(async move {
        while let Some(bytes) = out_rx.recv().await {
            if sink.send(Message::Binary(bytes)).await.is_err() {
                break;
            }
        }
    });

    // 첫 메시지는 Join 이어야 한다. 그 전에 Hello 로 프로토콜 버전을 받는다.
    let mut protocol = 0u16;
    let (room_tx, id) = loop {
        let Some(Ok(msg)) = stream.next().await else {
            matchmaking::forget(conn);
            writer.abort();
            return;
        };
        let Message::Binary(b) = msg else { continue };
        let (room, name) = match decode::<ClientMsg>(&b) {
            Some(ClientMsg::Hello { protocol: p }) => {
                protocol = p;
                continue;
            }
            Some(ClientMsg::Join { .. }) if protocol < bs_engine::protocol::PROTOCOL => {
                // 예전 클라이언트는 게임판 크기가 달라 상태를 받으면 멈춘다. 입장시키지 않는다.
                let msg = ServerMsg::Error("새 버전이 나왔습니다. 페이지를 새로고침해 주세요.".into());
                let _ = out_tx.send(axum::body::Bytes::from(encode(&msg))).await;
                continue;
            }
            Some(ClientMsg::Join { room, name }) => (room, name),
            Some(other) => {
                handle_lobby_msg(&reg, conn, &out_tx, &other).await;
                continue;
            }
            None => continue,
        };
        let mut name: String = name.trim().chars().filter(|c| !c.is_control()).take(MAX_NAME_LEN).collect();
        if name.is_empty() {
            name = "플레이어".into();
        }
        let mut code = sanitize_room(&room);
        if code.is_empty() {
            code = new_room_code(&reg);
        }
        let mut joined = None;
        for _ in 0..2 {
            let tx = room_sender(&reg, &code);
            let (rtx, rrx) = oneshot::channel();
            if tx.send(RoomCmd::Join { name: name.clone(), tx: out_tx.clone(), reply: rtx }).await.is_err() {
                continue; // 방이 막 닫힌 경우 → 새로 만든다
            }
            match rrx.await {
                Ok(Ok(id)) => {
                    joined = Some((tx, id));
                    break;
                }
                Ok(Err(e)) => {
                    let _ = out_tx.send(axum::body::Bytes::from(encode(&ServerMsg::Error(e)))).await;
                    break;
                }
                Err(_) => continue,
            }
        }
        match joined {
            Some(j) => break j,
            None => continue,
        }
    };

    while let Some(Ok(msg)) = stream.next().await {
        match msg {
            Message::Binary(b) => {
                if let Some(m) = decode::<ClientMsg>(&b) {
                    // 방 목록과 빠른 대전은 방 태스크를 거치지 않는다
                    if handle_lobby_msg(&reg, conn, &out_tx, &m).await {
                        continue;
                    }
                    if room_tx.send(RoomCmd::Msg { id, msg: m }).await.is_err() {
                        break;
                    }
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }
    matchmaking::forget(conn);
    let _ = room_tx.send(RoomCmd::Leave { id }).await;
    writer.abort();
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn join(reg: &Registry, code: &str, name: &str) -> (mpsc::Receiver<axum::body::Bytes>, u8) {
        let (tx, rx) = mpsc::channel(256);
        let (rtx, rrx) = oneshot::channel();
        room_sender(reg, code).send(RoomCmd::Join { name: name.into(), tx, reply: rtx }).await.unwrap();
        (rx, rrx.await.unwrap().unwrap())
    }

    #[tokio::test(start_paused = true)]
    async fn rules_and_rooms_share_selection_privacy_order_and_clear_empty_snapshots() {
        use bs_engine::protocol::decode;
        let reg: Registry = Default::default();
        let (_host, id) = join(&reg, "PUBLIC", "host").await;
        let config = RoomConfig { max_players: 4, teams: true, duration_secs: 60, ..Default::default() };
        room_sender(&reg, "PUBLIC").send(RoomCmd::Msg { id, msg: ClientMsg::SetRoomConfig(config.clone()) }).await.unwrap();
        let (_hidden, hidden_id) = join(&reg, "HIDDEN", "hidden").await;
        room_sender(&reg, "HIDDEN").send(RoomCmd::Msg { id: hidden_id, msg: ClientMsg::SetRoomConfig(RoomConfig { public: false, ..Default::default() }) }).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let (out, mut rx) = mpsc::channel(8);
        send_room_list(&reg, &out);
        assert_eq!(decode::<ServerMsg>(&rx.recv().await.unwrap()), Some(ServerMsg::CoopRooms(vec![])));
        assert_eq!(decode::<ServerMsg>(&rx.recv().await.unwrap()), Some(ServerMsg::RoomRules(vec![("PUBLIC".into(), config)])));
        assert!(matches!(decode::<ServerMsg>(&rx.recv().await.unwrap()), Some(ServerMsg::Rooms(v)) if v.len() == 1 && v[0].code == "PUBLIC"));
        let empty: Registry = Default::default();
        send_room_list(&empty, &out);
        assert_eq!(decode::<ServerMsg>(&rx.recv().await.unwrap()), Some(ServerMsg::CoopRooms(vec![])));
        assert_eq!(decode::<ServerMsg>(&rx.recv().await.unwrap()), Some(ServerMsg::RoomRules(vec![])));
        assert_eq!(decode::<ServerMsg>(&rx.recv().await.unwrap()), Some(ServerMsg::Rooms(vec![])));
    }

    #[tokio::test(start_paused = true)]
    async fn private_room_is_hidden_but_code_join_remains_available() {
        use bs_engine::protocol::RoomConfig;
        let reg: Registry = Default::default();
        let (_host, id) = join(&reg, "PRIVATE", "host").await;
        room_sender(&reg, "PRIVATE").send(RoomCmd::Msg { id, msg: ClientMsg::SetRoomConfig(RoomConfig { public: false, ..Default::default() }) }).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert!(list_rooms(&reg).is_empty());
        let (_guest, _) = join(&reg, "PRIVATE", "guest").await;
        assert_eq!(reg.lock().unwrap().get("PRIVATE").unwrap().summary.humans, 2);
        assert!(list_rooms(&reg).is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn room_list_shows_open_rooms_and_hides_closed_ones() {
        let reg: Registry = Default::default();
        let (_a, host_a) = join(&reg, "AAAA1", "철수").await;
        let (_b1, _) = join(&reg, "BBBB2", "영희").await;
        let (b2, _) = join(&reg, "BBBB2", "민수").await;
        room_sender(&reg, "AAAA1").send(RoomCmd::Msg { id: host_a, msg: ClientMsg::AddBot }).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let list = list_rooms(&reg);
        assert_eq!(list.len(), 2);
        // 사람 많은 방이 먼저
        assert_eq!(list[0].code, "BBBB2");
        assert_eq!((list[0].humans, list[0].host.as_str()), (2, "영희"));
        assert_eq!((list[1].players, list[1].bots), (2, 1));
        assert!(!list[1].playing);

        // 게임을 시작하면 진행 중으로 바뀌고 대기 중인 방 뒤로 간다
        room_sender(&reg, "AAAA1").send(RoomCmd::Msg { id: host_a, msg: ClientMsg::Start }).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let list = list_rooms(&reg);
        assert!(list.iter().find(|r| r.code == "AAAA1").unwrap().playing);
        assert_eq!(list.last().unwrap().code, "AAAA1");

        // 사람이 모두 나가면 목록에서 사라진다
        drop(b2);
        for code in ["BBBB2"] {
            let tx = room_sender(&reg, code);
            tx.send(RoomCmd::Leave { id: 1 }).await.unwrap();
            tx.send(RoomCmd::Leave { id: 2 }).await.unwrap();
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert!(list_rooms(&reg).iter().all(|r| r.code != "BBBB2"));
    }
}
