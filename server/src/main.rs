//! Block Stack 서버: 정적 파일(WASM 클라이언트) 제공 + WebSocket 대전.

mod room;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::http::{header, HeaderValue};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use bs_engine::protocol::{decode, encode, ClientMsg, RoomSummary, ServerMsg, MAX_NAME_LEN};
use futures_util::{SinkExt, StreamExt};
use room::{spawn_room, Outbox, Registry, RoomCmd, RoomEntry};

use tokio::sync::{mpsc, oneshot};
use tower_http::compression::CompressionLayer;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::set_header::SetResponseHeaderLayer;

#[tokio::main]
async fn main() {
    let port: u16 = std::env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(3000);
    let dist = std::env::var("BS_DIST").unwrap_or_else(|_| "client/dist".into());
    let registry: Registry = Default::default();

    let static_files = ServeDir::new(&dist).fallback(ServeFile::new(format!("{dist}/index.html")));
    let app = Router::new()
        .route("/ws", get(ws_handler))
        .route("/healthz", get(|| async { "ok" }))
        .fallback_service(static_files)
        .layer(CompressionLayer::new())
        .layer(SetResponseHeaderLayer::if_not_present(header::CACHE_CONTROL, HeaderValue::from_static("no-cache")))
        .with_state(registry);

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(addr).await.expect("bind");
    println!("Block Stack 서버: http://localhost:{port}  (정적 파일: {dist})");
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
            map.insert(code.to_string(), RoomEntry { tx: tx.clone(), summary: RoomSummary { code: code.to_string(), ..Default::default() } });
            tx
        }
    }
}

/// 현재 열린 방 목록: 사람이 있는 방만, 대기 중인 방 → 사람 많은 순
const MAX_LISTED_ROOMS: usize = 50;

fn list_rooms(reg: &Registry) -> Vec<RoomSummary> {
    let map = reg.lock().unwrap();
    let mut v: Vec<RoomSummary> =
        map.values().filter(|e| !e.tx.is_closed() && e.summary.humans > 0).map(|e| e.summary.clone()).collect();
    v.sort_by(|a, b| a.playing.cmp(&b.playing).then(b.humans.cmp(&a.humans)).then(a.code.cmp(&b.code)));
    v.truncate(MAX_LISTED_ROOMS);
    v
}

fn rooms_msg(reg: &Registry) -> axum::body::Bytes {
    axum::body::Bytes::from(encode(&ServerMsg::Rooms(list_rooms(reg))))
}

async fn handle_socket(socket: WebSocket, reg: Registry) {
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

    // 첫 메시지는 Join 이어야 한다
    let (room_tx, id) = loop {
        let Some(Ok(msg)) = stream.next().await else {
            writer.abort();
            return;
        };
        let Message::Binary(b) = msg else { continue };
        let (room, name) = match decode::<ClientMsg>(&b) {
            Some(ClientMsg::Join { room, name }) => (room, name),
            Some(ClientMsg::ListRooms) => {
                let _ = out_tx.try_send(rooms_msg(&reg));
                continue;
            }
            _ => continue,
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
                    // 방 목록은 방 태스크를 거치지 않고 레지스트리에서 바로 응답
                    if m == ClientMsg::ListRooms {
                        let _ = out_tx.try_send(rooms_msg(&reg));
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
