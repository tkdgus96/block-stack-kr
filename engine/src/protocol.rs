//! 클라이언트 ↔ 서버 WebSocket 메시지 (postcard 바이너리 인코딩).

use crate::game::{Game, Input};
use serde::{Deserialize, Serialize};

pub const MAX_PLAYERS: usize = 6;
pub const MAX_NAME_LEN: usize = 16;
pub const MAX_CHAT_LEN: usize = 120;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum ClientMsg {
    /// 방 참가. room이 비어 있으면 새 방을 만든다.
    Join { room: String, name: String },
    /// 방장만: 게임 시작
    Start,
    /// 방장만: 연습 상대(봇) 추가/제거
    AddBot,
    RemoveBot,
    /// 입력. seq는 클라이언트 예측 보정에 사용
    Input { seq: u32, input: Input },
    Ping { t: u32 },
    /// 현재 개설된 방 목록 요청 (방에 들어가 있어도 가능)
    ListRooms,
    /// 같은 방 채팅
    Chat { text: String },
}

/// 채팅 한 줄
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChatLine {
    pub from: u8,
    pub name: String,
    pub text: String,
}

/// 방 목록 한 줄
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RoomSummary {
    pub code: String,
    pub host: String,
    /// 봇 포함 인원
    pub players: u8,
    /// 사람 인원
    pub humans: u8,
    pub bots: u8,
    /// 게임 진행 중(카운트다운 포함)
    pub playing: bool,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Phase {
    Lobby,
    Countdown,
    Playing,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlayerInfo {
    pub id: u8,
    pub name: String,
    pub bot: bool,
    pub wins: u32,
    /// 현재 판에 참가 중인지
    pub in_game: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlayerStat {
    pub id: u8,
    pub score: u32,
    pub lines: u32,
    pub level: u8,
    pub sent: u32,
    pub received: u32,
    pub cancelled: u32,
    pub pending: u32,
    pub alive: bool,
    /// 탈락 순위 (0 = 아직 결정 안 됨)
    pub place: u8,
    /// 현재 공격 대상 id
    pub target: Option<u8>,
    /// 이번 판 KO 수
    pub kos: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum ServerMsg {
    Welcome { you: u8, room: String },
    Lobby { phase: Phase, host: u8, players: Vec<PlayerInfo> },
    Countdown { seconds: u8 },
    Started { seed: u64 },
    /// 내 게임 상태 (권위 있는 상태). ack = 마지막으로 처리된 입력 seq
    YourState { ack: u32, game: Game },
    /// 상대 게임판들: (id, 4비트 압축 셀 100바이트)
    Boards(Vec<(u8, Vec<u8>)>),
    Scores(Vec<PlayerStat>),
    Log(String),
    GameOver { ranking: Vec<u8> },
    Pong { t: u32 },
    Error(String),
    Rooms(Vec<RoomSummary>),
    /// 채팅. 입장 직후에는 최근 기록을 여러 줄 한꺼번에 보낸다
    Chat(Vec<ChatLine>),
}

pub fn encode<T: Serialize>(msg: &T) -> Vec<u8> {
    postcard::to_allocvec(msg).expect("encode")
}

pub fn decode<'a, T: Deserialize<'a>>(bytes: &'a [u8]) -> Option<T> {
    postcard::from_bytes(bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_state() {
        let g = Game::new(9);
        let m = ServerMsg::YourState { ack: 7, game: g.clone() };
        let bytes = encode(&m);
        assert!(bytes.len() < 400, "state size = {}", bytes.len());
        assert_eq!(decode::<ServerMsg>(&bytes), Some(m));
    }

    #[test]
    fn roundtrip_client() {
        let m = ClientMsg::Input { seq: 3, input: Input::HardDrop };
        assert_eq!(decode::<ClientMsg>(&encode(&m)), Some(m));
    }
}
