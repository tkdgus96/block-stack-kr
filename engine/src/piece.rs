//! 블록 종류와 회전 데이터.
//!
//! 회전 상태별 셀 좌표는 컴파일 타임 상수로 계산해 두어 런타임 할당이 없다.

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// 4칸 블록 7종. 셀 코드 1..=7 에 대응한다.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[repr(u8)]
pub enum Kind {
    I = 1,
    O = 2,
    T = 3,
    S = 4,
    Z = 5,
    J = 6,
    L = 7,
}

pub const ALL_KINDS: [Kind; 7] = [Kind::I, Kind::O, Kind::T, Kind::S, Kind::Z, Kind::J, Kind::L];

impl Kind {
    #[inline]
    pub fn code(self) -> u8 {
        self as u8
    }

    pub fn from_code(c: u8) -> Option<Kind> {
        match c {
            1 => Some(Kind::I),
            2 => Some(Kind::O),
            3 => Some(Kind::T),
            4 => Some(Kind::S),
            5 => Some(Kind::Z),
            6 => Some(Kind::J),
            7 => Some(Kind::L),
            _ => None,
        }
    }

    /// 회전 상자 크기
    fn box_size(self) -> i8 {
        match self {
            Kind::I => 4,
            Kind::O => 2,
            _ => 3,
        }
    }

    fn base(self) -> [(i8, i8); 4] {
        match self {
            Kind::I => [(0, 1), (1, 1), (2, 1), (3, 1)],
            Kind::O => [(0, 0), (1, 0), (0, 1), (1, 1)],
            Kind::T => [(1, 0), (0, 1), (1, 1), (2, 1)],
            Kind::S => [(1, 0), (2, 0), (0, 1), (1, 1)],
            Kind::Z => [(0, 0), (1, 0), (1, 1), (2, 1)],
            Kind::J => [(0, 0), (0, 1), (1, 1), (2, 1)],
            Kind::L => [(2, 0), (0, 1), (1, 1), (2, 1)],
        }
    }

    /// 회전 상태 `rot`(0..4)의 셀 오프셋 (x, y). y는 아래 방향이 +.
    pub fn cells(self, rot: u8) -> [(i8, i8); 4] {
        let n = self.box_size();
        let mut c = self.base();
        for _ in 0..(rot & 3) {
            for p in c.iter_mut() {
                // 시계 방향 90도: (x, y) -> (n-1-y, x)
                *p = (n - 1 - p.1, p.0);
            }
        }
        c
    }

    /// 생성 위치 x
    pub fn spawn_x(self) -> i8 {
        match self {
            Kind::O => 4,
            _ => 3,
        }
    }
}

/// SRS transition-specific kick tests; coordinates use y-down. Index 4 is
/// the fifth (last) kick and upgrades a qualifying T-spin Mini to Full.
pub fn kicks(kind: Kind, from: u8, to: u8) -> &'static [(i8, i8)] {
    if kind == Kind::O { return &[(0, 0)]; }
    if kind == Kind::I {
        match (from & 3, to & 3) {
            (0, 1) | (3, 2) => &[(0,0),(-2,0),(1,0),(-2,1),(1,-2)],
            (1, 0) | (2, 3) => &[(0,0),(2,0),(-1,0),(2,-1),(-1,2)],
            (1, 2) | (0, 3) => &[(0,0),(-1,0),(2,0),(-1,-2),(2,1)],
            (2, 1) | (3, 0) => &[(0,0),(1,0),(-2,0),(1,2),(-2,-1)],
            _ => &[],
        }
    } else {
        match (from & 3, to & 3) {
            (0, 1) | (2, 1) => &[(0,0),(-1,0),(-1,-1),(0,2),(-1,2)],
            (1, 0) | (1, 2) => &[(0,0),(1,0),(1,1),(0,-2),(1,-2)],
            (2, 3) | (0, 3) => &[(0,0),(1,0),(1,-1),(0,2),(1,2)],
            (3, 2) | (3, 0) => &[(0,0),(-1,0),(-1,1),(0,-2),(-1,-2)],
            _ => &[],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_rotations_return_to_start() {
        for k in ALL_KINDS {
            let mut a = k.cells(0);
            let mut b = k.cells(4);
            a.sort();
            b.sort();
            assert_eq!(a, b, "{k:?}");
        }
    }

    #[test]
    fn o_piece_does_not_change_on_rotation() {
        let mut a = Kind::O.cells(0);
        let mut b = Kind::O.cells(1);
        a.sort();
        b.sort();
        assert_eq!(a, b);
    }
}
