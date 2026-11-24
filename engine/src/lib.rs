//! Block Stack 게임 엔진.
//!
//! - 렌더링·네트워크·시계에 의존하지 않는 순수 Rust 로직
//! - 서버(권위 있는 시뮬레이션)와 클라이언트(입력 예측)가 같은 코드를 사용
//! - 결정적: 같은 시드 + 같은 입력 순서 = 같은 결과

pub mod bot;
pub mod finesse;
pub mod puzzles;
pub mod practice;
pub mod game;
pub mod piece;
#[cfg(feature = "serde")]
pub mod protocol;
#[cfg(feature = "serde")]
pub mod replay;
pub mod rng;

pub use game::*;
pub use piece::Kind;
