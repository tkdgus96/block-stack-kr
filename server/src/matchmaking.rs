//! 빠른 대전 1:1 대기열.
//!
//! 레이팅 차이가 허용 범위 안인 두 사람을 묶어 비공개 방을 만든다. 허용 범위는 기다린 시간에
//! 따라 넓어지고, `BOT_AFTER` 동안 상대가 없으면 봇과 붙인다. 레이팅은 참고용이다.
use crate::room::{Outbox, Registry, RoomCmd};
use bs_engine::protocol::{encode, ServerMsg};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const BOT_AFTER: Duration = Duration::from_secs(15);

struct Ticket {
    conn: u64,
    rating: f64,
    since: Instant,
    out: Outbox,
}

static QUEUE: Mutex<Vec<Ticket>> = Mutex::new(Vec::new());

/// 기다린 시간에 따른 허용 레이팅 차이
fn window(waited: Duration) -> f64 {
    150.0 + 25.0 * waited.as_secs_f64()
}

fn notify(out: &Outbox, msg: &ServerMsg) {
    let _ = out.try_send(axum::body::Bytes::from(encode(msg)));
}

/// 대기열에 넣고 바로 짝을 찾아 본다.
pub fn enqueue(reg: &Registry, conn: u64, rating: f64, out: Outbox) {
    {
        let mut q = QUEUE.lock().unwrap();
        q.retain(|t| t.conn != conn && !t.out.is_closed());
        q.push(Ticket { conn, rating, since: Instant::now(), out: out.clone() });
    }
    notify(&out, &ServerMsg::QuickMatchWaiting(true));
    sweep(reg, Instant::now());
}

/// 대기열에 없어도 답한다. 재접속 뒤처럼 클라이언트만 기다리는 중이라고 알고 있는 경우가 있다.
pub fn cancel(conn: u64, out: &Outbox) {
    QUEUE.lock().unwrap().retain(|t| t.conn != conn);
    notify(out, &ServerMsg::QuickMatchWaiting(false));
}

/// 연결이 끊긴 경우: 알릴 곳이 없으므로 조용히 뺀다.
pub fn forget(conn: u64) {
    QUEUE.lock().unwrap().retain(|t| t.conn != conn);
}

/// 사람끼리 짝을 짓고, 오래 기다린 사람은 봇과 붙인다. 1초마다, 그리고 새로 들어올 때 부른다.
pub fn sweep(reg: &Registry, now: Instant) {
    let mut found: Vec<(Vec<Outbox>, bool)> = Vec::new();
    {
        let mut q = QUEUE.lock().unwrap();
        q.retain(|t| !t.out.is_closed());
        // 오래 기다린 사람부터 짝을 찾는다.
        q.sort_by_key(|t| t.since);
        let mut i = 0;
        while i < q.len() {
            let wa = now.saturating_duration_since(q[i].since);
            let partner = (i + 1..q.len())
                .filter(|&j| {
                    let wb = now.saturating_duration_since(q[j].since);
                    (q[i].rating - q[j].rating).abs() <= window(wa.max(wb))
                })
                .min_by(|&x, &y| {
                    let dx = (q[i].rating - q[x].rating).abs();
                    let dy = (q[i].rating - q[y].rating).abs();
                    dx.total_cmp(&dy)
                });
            if let Some(j) = partner {
                let b = q.remove(j);
                let a = q.remove(i);
                found.push((vec![a.out, b.out], false));
            } else if wa >= BOT_AFTER {
                let a = q.remove(i);
                found.push((vec![a.out], true));
            } else {
                i += 1;
            }
        }
    }
    for (outs, vs_bot) in found {
        let code = crate::new_room_code(reg);
        let tx = crate::room_sender(reg, &code);
        if tx.try_send(RoomCmd::QuickPreset { vs_bot, humans: outs.len() as u8 }).is_err() {
            // 방을 만들지 못했다. 대기열에서는 이미 뺐으므로 기다리지 않게 알린다.
            for out in &outs {
                notify(out, &ServerMsg::QuickMatchWaiting(false));
            }
            continue;
        }
        for out in &outs {
            notify(out, &ServerMsg::MatchFound { room: code.clone(), vs_bot });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bs_engine::protocol::decode;
    use tokio::sync::mpsc;

    fn drain(rx: &mut mpsc::Receiver<axum::body::Bytes>) -> Vec<ServerMsg> {
        let mut out = Vec::new();
        while let Ok(b) = rx.try_recv() { out.extend(decode::<ServerMsg>(&b)); }
        out
    }

    // QUEUE 는 전역이므로 한 테스트에서 차례로 확인한다.
    #[tokio::test]
    async fn pairs_close_ratings_widens_with_time_and_falls_back_to_bot() {
        let reg: Registry = Default::default();
        let (ta, mut ra) = mpsc::channel(16);
        let (tb, mut rb) = mpsc::channel(16);
        let (tc, mut rc) = mpsc::channel(16);
        enqueue(&reg, 1, 1500.0, ta);
        enqueue(&reg, 2, 2000.0, tb); // 차이가 커서 바로 묶이지 않는다
        assert_eq!(drain(&mut ra), vec![ServerMsg::QuickMatchWaiting(true)]);
        assert_eq!(drain(&mut rb), vec![ServerMsg::QuickMatchWaiting(true)]);
        enqueue(&reg, 3, 1550.0, tc);
        let a = drain(&mut ra);
        let c = drain(&mut rc);
        let room = match a.last() { Some(ServerMsg::MatchFound { room, vs_bot: false }) => room.clone(), other => panic!("{other:?}") };
        assert_eq!(c.last(), Some(&ServerMsg::MatchFound { room: room.clone(), vs_bot: false }));
        assert!(reg.lock().unwrap().contains_key(&room));
        // 2번은 혼자 남았다. 시간이 지나면 봇과 붙는다.
        sweep(&reg, Instant::now() + BOT_AFTER);
        assert!(matches!(drain(&mut rb).last(), Some(ServerMsg::MatchFound { vs_bot: true, .. })));
        assert!(QUEUE.lock().unwrap().is_empty());
        // 취소하면 대기열에서 빠진다.
        let (td, mut rd) = mpsc::channel(16);
        enqueue(&reg, 4, 1500.0, td.clone());
        cancel(4, &td);
        assert_eq!(drain(&mut rd), vec![ServerMsg::QuickMatchWaiting(true), ServerMsg::QuickMatchWaiting(false)]);
        assert!(QUEUE.lock().unwrap().is_empty());
        // 대기열에 없어도 기다리지 않는다고 답한다(재접속 뒤 버튼이 멈추지 않게).
        cancel(4, &td);
        assert_eq!(drain(&mut rd), vec![ServerMsg::QuickMatchWaiting(false)]);
        assert!(window(Duration::from_secs(20)) > 600.0);
    }
}
