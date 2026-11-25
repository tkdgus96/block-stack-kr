//! 클라이언트 ↔ 서버 WebSocket 메시지 (postcard 바이너리 인코딩).

use crate::game::{CombatState, Game, Input, SpinKind};
use serde::{Deserialize, Serialize};

pub const MAX_PLAYERS: usize = 6;
pub const MAX_NAME_LEN: usize = 16;
pub const MAX_CHAT_LEN: usize = 120;
/// 게임판 크기처럼 예전 클라이언트가 해석하지 못하는 변경이 있을 때 올린다.
/// 2: 숨김 영역을 10줄로 늘려 게임판이 300칸이 되었다.
pub const PROTOCOL: u16 = 2;

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
    SetRoomConfig(RoomConfig),
    SetTeam { team: u8 },
    Ready { ready: bool },
    /// Opt in to authoritative combat metadata without changing legacy Game bytes.
    EnableCombat,
    AttachProfile { token: String },
    SetCoop { enabled: bool },
    /// 내 공격을 누구에게 보낼지. 새 값은 항상 맨 뒤에 붙인다.
    SetTargetMode { mode: TargetMode },
    /// 빠른 대전 대기열에 들어간다. token 은 프로필 복구 코드(없으면 빈 문자열)로 레이팅을 찾는 데만 쓴다.
    QuickMatch { token: String },
    CancelQuickMatch,
    /// 연결 직후 Join 보다 먼저 보낸다. 보내지 않은 예전 클라이언트는 입장을 거절하고 새로고침을 안내한다.
    Hello { protocol: u16 },
}

/// 서버 순위표가 있는 연습 모드
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum RankedMode {
    Sprint,
    Blitz,
    Marathon,
}

impl RankedMode {
    pub const ALL: [RankedMode; 3] = [RankedMode::Sprint, RankedMode::Blitz, RankedMode::Marathon];
    pub fn practice(self) -> crate::practice::Mode {
        match self {
            RankedMode::Sprint => crate::practice::Mode::Sprint,
            RankedMode::Blitz => crate::practice::Mode::Blitz,
            RankedMode::Marathon => crate::practice::Mode::Marathon,
        }
    }
    pub fn from_practice(mode: crate::practice::Mode) -> Option<RankedMode> {
        RankedMode::ALL.into_iter().find(|m| m.practice() == mode)
    }
    /// URL 에 쓰는 이름
    pub fn slug(self) -> &'static str {
        match self {
            RankedMode::Sprint => "sprint",
            RankedMode::Blitz => "blitz",
            RankedMode::Marathon => "marathon",
        }
    }
}

/// 순위표 등록 요청 (`POST /api/leaderboard`, postcard). 서버가 입력 기록으로 다시 실행해 기록을 확인한다.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct LeaderboardSubmission {
    pub mode: RankedMode,
    pub seed: u64,
    pub end_tick: u32,
    pub name: String,
    pub log: Vec<(u32, Input, bool)>,
}

/// 공격 대상 선택 방식
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum TargetMode {
    /// 무작위로 고른 상대를 그 상대가 탈락할 때까지 유지 (기존 동작)
    #[default]
    Random,
    /// 쌓인 높이와 받을 공격의 합이 가장 큰 상대
    Danger,
    /// 나를 마지막으로 공격한 상대, 없으면 나를 노리는 상대, 그것도 없으면 무작위
    Counter,
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

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RoomConfig {
    pub public: bool,
    pub duration_secs: u16,
    pub max_players: u8,
    pub bot_level: u8,
    pub teams: bool,
}
impl Default for RoomConfig {
    fn default() -> Self { Self { public: true, duration_secs: 120, max_players: 6, bot_level: 1, teams: false } }
}
impl RoomConfig {
    pub fn valid(&self) -> bool {
        matches!(self.duration_secs, 60 | 120 | 180) && (2..=6).contains(&self.max_players)
            && self.bot_level <= 2 && (!self.teams || matches!(self.max_players, 4 | 6))
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemberState {
    pub id: u8,
    /// A팀=0, B팀=1. 개인전에서는 무시한다.
    pub team: u8,
    pub ready: bool,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RoundPlayer {
    pub id: u8,
    pub name: String,
    pub team: u8,
    pub place: u8,
    pub score: u32,
    pub sent: u32,
    pub active_ticks: u32,
    pub kos: u32,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MatchSummary {
    pub match_id: u64,
    pub elapsed_ticks: u32,
    pub timed_out: bool,
    pub teams: bool,
    /// 팀전은 우승팀 전원. 무승부는 두 팀 모두 포함한다.
    pub winners: Vec<u8>,
    /// 중도 퇴장자를 포함한 이번 판 전체 참가자.
    pub players: Vec<RoundPlayer>,
}

/// 방에 머무는 동안의 대전 통계. 완료된 판만 rounds/total_sent에 누적한다.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct BattleStat {
    pub id: u8,
    /// 이번 판에서 실제로 살아서 진행한 tick 수 (탈락 후 고정).
    pub active_ticks: u32,
    pub rounds: u32,
    pub total_sent: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CoopState { pub enabled: bool, pub goal: u32, pub progress: u32, pub won: Option<bool> }

/// One authoritative piece lock, including zero-line locks that reset a combo.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CombatEvent {
    pub match_id: u64,
    pub player: u8,
    pub piece: u32,
    pub cleared: u8,
    pub spin: SpinKind,
    pub combo: i8,
    pub b2b: u16,
    pub back_to_back: bool,
    pub perfect_clear: bool,
    pub generated: u8,
    pub cancelled: u8,
    /// Lines actually delivered to an eligible opponent, after cancellation.
    pub sent: u8,
    pub target: Option<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CombatStatus {
    pub id: u8,
    pub combo: i8,
    pub b2b: u16,
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
    /// 기존 postcard variant 번호를 보존하려면 새 메시지는 끝에만 추가한다.
    MatchInfo { remaining_secs: u16, elapsed_ticks: u32, timed: bool },
    BattleStats(Vec<BattleStat>),
    /// 공동 1위는 winners에 모두 포함되며 승수는 증가하지 않는다.
    BattleResult { timed_out: bool, winners: Vec<u8> },
    RoomState { config: RoomConfig, members: Vec<MemberState> },
    MatchSummary(MatchSummary),
    RoomRules(Vec<(String, RoomConfig)>),
    YourStateV2 { ack: u32, game: Game, combat: CombatState },
    CombatEvent(CombatEvent),
    CombatStatus(Vec<CombatStatus>),
    CoopState(CoopState),
    CoopRooms(Vec<String>),
    /// 끝난 경기의 전체 리플레이를 `/api/replay/{id}` 에서 받을 수 있다.
    ReplayReady { id: String },
    /// 빠른 대전 대기 상태. false 면 대기열에서 빠졌다.
    QuickMatchWaiting(bool),
    /// 상대를 찾았다. 이 방 코드로 들어가면 바로 시작한다. vs_bot 이면 사람 대신 봇과 붙는다.
    MatchFound { room: String, vs_bot: bool },
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
    fn room_rules_appends_without_changing_legacy_rooms_payload() {
        let message = ServerMsg::RoomRules(vec![("ROOM".into(), RoomConfig { max_players: 4, teams: true, ..Default::default() })]);
        assert_eq!(encode(&message)[0], 18);
        assert_eq!(decode::<ServerMsg>(&encode(&message)), Some(message));
        assert_eq!(encode(&ServerMsg::Rooms(vec![])), vec![11, 0]);
        assert_eq!(encode(&ServerMsg::RoomRules(vec![])), vec![18, 0]);
    }

    #[test]
    fn room_contract_roundtrips_append_only_and_validates_configuration() {
        let config = RoomConfig::default();
        assert!(config.valid());
        for duration_secs in [60,120,180] { assert!(RoomConfig { duration_secs, ..config.clone() }.valid()); }
        assert!(!RoomConfig { teams: true, max_players: 5, ..config.clone() }.valid());
        assert!(!RoomConfig { duration_secs: 0, ..config.clone() }.valid());
        assert!(!RoomConfig { bot_level: 3, ..config.clone() }.valid());
        let state = ServerMsg::RoomState { config: config.clone(), members: vec![MemberState { id: 1, team: 0, ready: true }] };
        assert_eq!(encode(&state)[0], 16);
        assert_eq!(decode::<ServerMsg>(&encode(&state)), Some(state));
        let summary = ServerMsg::MatchSummary(MatchSummary { match_id: 123, players: vec![RoundPlayer { id: 1, name: "player".into(), ..Default::default() }], ..Default::default() });
        assert_eq!(encode(&summary)[0], 17);
        assert_eq!(decode::<ServerMsg>(&encode(&summary)), Some(summary));
        for (index, msg) in [ClientMsg::SetRoomConfig(config), ClientMsg::SetTeam { team: 1 }, ClientMsg::Ready { ready: true }].into_iter().enumerate() {
            let bytes = encode(&msg);
            assert_eq!(bytes[0], 8 + index as u8);
            assert_eq!(decode::<ClientMsg>(&bytes), Some(msg));
        }
    }

    #[test]
    fn battle_messages_roundtrip_and_existing_variant_indices_stay_unchanged() {
        assert_eq!(encode(&ServerMsg::Chat(vec![])), vec![12, 0]);
        assert_eq!(encode(&ServerMsg::GameOver { ranking: vec![1, 2] }), vec![8, 2, 1, 2]);
        let messages = [
            ServerMsg::MatchInfo { remaining_secs: 120, elapsed_ticks: 0, timed: true },
            ServerMsg::BattleStats(vec![BattleStat { id: 1, active_ticks: 3600, rounds: 2, total_sent: 80 }]),
            ServerMsg::BattleResult { timed_out: true, winners: vec![1, 2] },
        ];
        for (index, msg) in messages.iter().enumerate() {
            let encoded = encode(msg);
            assert_eq!(encoded[0], 13 + index as u8);
            assert_eq!(decode::<ServerMsg>(&encoded).as_ref(), Some(msg));
        }
    }

    #[test]
    fn roundtrip_state() {
        let g = Game::new(9);
        let m = ServerMsg::YourState { ack: 7, game: g.clone() };
        let bytes = encode(&m);
        assert!(bytes.len() < 400, "state size = {}", bytes.len());
        assert_eq!(decode::<ServerMsg>(&bytes), Some(m));
    }

    #[test]
    fn combat_extensions_append_and_keep_legacy_state_layout() {
        assert_eq!(encode(&ClientMsg::EnableCombat), vec![11]);
        let mut game = Game::new(9);
        let legacy = encode(&ServerMsg::YourState { ack: 7, game: game.clone() });
        game.combat.back_to_back = 4;
        game.combat.last_rotation = Some(crate::game::RotationInfo { kick_index: 3 });
        assert_eq!(encode(&ServerMsg::YourState { ack: 7, game: game.clone() }), legacy);
        let Some(ServerMsg::YourState { game: old, .. }) = decode::<ServerMsg>(&legacy) else { panic!("legacy state decode") };
        assert_eq!(old.combat, CombatState::default());
        let combat = game.combat.clone();
        let v2 = ServerMsg::YourStateV2 { ack: 7, game, combat: combat.clone() };
        assert_eq!(encode(&v2)[0], 19);
        let Some(ServerMsg::YourStateV2 { ack, game, combat: hydrated }) = decode::<ServerMsg>(&encode(&v2)) else { panic!("v2 decode") };
        assert_eq!(ack, 7);
        assert_eq!(hydrated, combat);
        assert_eq!(game.combat, CombatState::default(), "client hydrates skipped metadata from the separate field");
        let event = CombatEvent { match_id: 12, player: 1, piece: 3, cleared: 2, spin: SpinKind::Full,
            combo: 2, b2b: 3, back_to_back: true, perfect_clear: false, generated: 6, cancelled: 2, sent: 4, target: Some(2) };
        let message = ServerMsg::CombatEvent(event);
        assert_eq!(encode(&message)[0], 20);
        assert_eq!(decode::<ServerMsg>(&encode(&message)), Some(message));
        let message = ServerMsg::CombatStatus(vec![CombatStatus { id: 1, combo: -1, b2b: 0 }]);
        assert_eq!(encode(&message)[0], 21);
        assert_eq!(decode::<ServerMsg>(&encode(&message)), Some(message));
        assert_eq!(legacy[0], 4);
    }

    #[test]
    fn growth_extensions_keep_existing_discriminants_and_roundtrip() {
        assert_eq!(encode(&ClientMsg::AttachProfile { token: "a".repeat(64) })[0],12);
        assert_eq!(encode(&ClientMsg::SetCoop { enabled:true }),vec![13,1]);
        let state = ServerMsg::CoopState(CoopState { enabled:true, goal:80, progress:4, won:None });
        assert_eq!(encode(&state)[0],22); assert_eq!(decode::<ServerMsg>(&encode(&state)),Some(state));
        let rooms = ServerMsg::CoopRooms(vec!["ABC".into()]);
        assert_eq!(encode(&rooms)[0],23); assert_eq!(decode::<ServerMsg>(&encode(&rooms)),Some(rooms));
        assert_eq!(encode(&ServerMsg::CombatStatus(vec![])),vec![21,0]);
        assert_eq!(encode(&ClientMsg::SetTargetMode { mode: TargetMode::Counter }),vec![14,2]);
        assert_eq!(encode(&ClientMsg::CancelQuickMatch),vec![16]);
        assert_eq!(encode(&ClientMsg::Hello { protocol: PROTOCOL }),vec![17,PROTOCOL as u8]);
    }

    #[test]
    fn roundtrip_client() {
        let m = ClientMsg::Input { seq: 3, input: Input::HardDrop };
        assert_eq!(decode::<ClientMsg>(&encode(&m)), Some(m));
    }

    #[test]
    fn input_discriminants_stay_stable_when_rotate_180_is_appended() {
        let inputs = [Input::Left, Input::Right, Input::RotateCw, Input::RotateCcw, Input::SoftDrop, Input::HardDrop, Input::Hold, Input::Rotate180];
        for (n, input) in inputs.into_iter().enumerate() {
            let bytes = encode(&ClientMsg::Input { seq: 1, input });
            assert_eq!(*bytes.last().unwrap(), n as u8, "{input:?}");
            assert_eq!(decode::<ClientMsg>(&bytes), Some(ClientMsg::Input { seq: 1, input }));
        }
    }
}
