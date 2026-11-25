//! 사용자 설정. localStorage 에 "키=값" 줄 형식으로 저장한다 (JSON 의존성 없이 가볍게).

use bs_engine::game::Input;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Theme {
    /// 업무용 Spreadsheet Mode
    Sheet,
    HighContrast,
    /// 기본: 게임 느낌의 어두운 테마 (독자 디자인)
    Arcade,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RendererKind {
    /// HTML + CSS Grid, 변경된 셀만 class 갱신
    Dom,
    /// Canvas 2D, 변경된 셀만 fillRect (WebGL 미사용).
    /// 측정 결과(docs/PERFORMANCE.md) 소프트웨어 렌더링에서 CPU 사용량이 더 낮아 기본값으로 선택
    Canvas,
}

pub const ACTIONS: [(Input, &str); 8] = [
    (Input::Left, "왼쪽 이동"),
    (Input::Right, "오른쪽 이동"),
    (Input::SoftDrop, "천천히 내리기"),
    (Input::HardDrop, "바로 내리기"),
    (Input::RotateCw, "시계 방향 회전"),
    (Input::RotateCcw, "반시계 방향 회전"),
    (Input::Hold, "보관"),
    (Input::Rotate180, "180도 회전"),
];

pub const DEFAULT_KEYS: [&str; 8] = [
    "ArrowLeft",
    "ArrowRight",
    "ArrowDown",
    "Space",
    "ArrowUp",
    "KeyZ",
    "KeyC",
    "KeyA",
];

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub theme: Theme,
    pub low_spec: bool,
    pub animations: bool,
    pub sound: bool,
    /// 셀 한 칸 크기(px): 게임판 크기
    pub cell: u8,
    pub renderer: RendererKind,
    /// false = 화면 갱신 최대 30fps (기본), true = 60fps
    pub fps60: bool,
    pub ghost: bool,
    pub das: u16,
    pub arr: u16,
    pub sdr: u16,
    pub keys: [String; 8],
    pub name: String,
    pub touch_size: u8,
    pub touch_opacity: u8,
    pub touch_left_handed: bool,
    /// 0: actions first, 1: movement first.
    pub touch_layout: u8,
    pub touch_vibration: bool,
    pub touch_gestures: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            theme: Theme::Arcade,
            low_spec: false,
            animations: false,
            sound: false,
            cell: 28,
            renderer: RendererKind::Canvas,
            fps60: false,
            ghost: true,
            das: 150,
            arr: 40,
            sdr: 40,
            keys: DEFAULT_KEYS.map(String::from),
            name: String::new(),
            touch_size: 100,
            touch_opacity: 100,
            touch_left_handed: false,
            touch_layout: 0,
            touch_vibration: false,
            touch_gestures: false,
        }
    }
}

/// 경쟁용 프리셋의 DAS(ms). ARR 과 내리기 반복은 0(즉시)이다.
pub const COMPETITIVE_DAS: u16 = 100;

const STORAGE_KEY: &str = "blockstack.settings.v1";

fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

impl Settings {
    pub fn action_for_code(&self, code: &str) -> Option<Input> {
        // 회전은 보조 키(KeyX)도 허용
        if code == "KeyX" && !self.keys.iter().any(|k| k == "KeyX") {
            return Some(Input::RotateCw);
        }
        self.keys
            .iter()
            .position(|k| k == code)
            .map(|i| ACTIONS[i].0)
    }

    /// 빠른 조작을 원하는 사용자를 위한 값. 키 배치와 화면 설정은 그대로 둔다.
    pub fn apply_competitive_preset(&mut self) {
        self.das = COMPETITIVE_DAS;
        self.arr = 0;
        self.sdr = 0;
    }

    /// 저사양 모드를 반영한 실제 값
    pub fn effective_animations(&self) -> bool {
        self.animations && !self.low_spec
    }

    pub fn effective_fps60(&self) -> bool {
        self.fps60 && !self.low_spec
    }

    pub fn load() -> Settings {
        storage()
            .and_then(|st| st.get_item(STORAGE_KEY).ok().flatten())
            .map(|raw| Self::from_storage(&raw))
            .unwrap_or_default()
    }

    /// Pure parser keeps older saved settings compatible and bounds new options.
    fn from_storage(raw: &str) -> Settings {
        let mut s = Self::default();
        for line in raw.lines() {
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let b = v == "1";
            match k {
                "theme" => {
                    s.theme = match v {
                        "hc" => Theme::HighContrast,
                        "arcade" => Theme::Arcade,
                        _ => Theme::Sheet,
                    }
                }
                "low" => s.low_spec = b,
                "anim" => s.animations = b,
                "sound" => s.sound = b,
                "cell" => s.cell = v.parse().unwrap_or(22).clamp(14, 34),
                "renderer" => {
                    s.renderer = if v == "dom" {
                        RendererKind::Dom
                    } else {
                        RendererKind::Canvas
                    }
                }
                "fps60" => s.fps60 = b,
                "ghost" => s.ghost = b,
                "das" => s.das = v.parse().unwrap_or(150).min(500),
                "arr" => s.arr = v.parse().unwrap_or(40).min(200),
                "sdr" => s.sdr = v.parse().unwrap_or(40).min(200),
                "name" => s.name = v.chars().take(16).collect(),
                "touch_size" => s.touch_size = v.parse::<u16>().unwrap_or(100).clamp(80, 125) as u8,
                "touch_opacity" => {
                    s.touch_opacity = v.parse::<u16>().unwrap_or(100).clamp(40, 100) as u8
                }
                "touch_left_handed" => s.touch_left_handed = b,
                "touch_layout" => s.touch_layout = v.parse::<u8>().unwrap_or(0).min(1),
                "touch_vibration" => s.touch_vibration = b,
                "touch_gestures" => s.touch_gestures = b,
                _ => {
                    if let Some(i) = k.strip_prefix("key").and_then(|n| n.parse::<usize>().ok()) {
                        if i < ACTIONS.len() && !v.is_empty() {
                            s.keys[i] = v.to_string();
                        }
                    }
                }
            }
        }
        // 180도 회전 이전에 저장한 설정에는 key7 이 없다. 기본 키를 이미 다른 동작에 쓰고 있으면
        // 같은 키에 두 동작이 겹쳐 180도 회전을 누를 수 없으므로 비어 있는 키를 대신 쓴다.
        let rotate_180 = ACTIONS.len() - 1;
        let saved_180 = raw.lines().any(|l| l.starts_with(&format!("key{rotate_180}=")));
        if !saved_180 && s.keys[..rotate_180].contains(&s.keys[rotate_180]) {
            s.keys[rotate_180] = ["KeyQ", "KeyE", "KeyW", "KeyD", "KeyS"]
                .into_iter()
                .find(|k| !s.keys[..rotate_180].iter().any(|used| used == k))
                .unwrap_or_default()
                .to_string();
        }
        s
    }

    pub fn save(&self) {
        if let Some(st) = storage() {
            let _ = st.set_item(STORAGE_KEY, &self.to_storage());
        }
    }

    fn to_storage(&self) -> String {
        let b = |x: bool| if x { "1" } else { "0" };
        let mut out = format!(
            "theme={}\nlow={}\nanim={}\nsound={}\ncell={}\nrenderer={}\nfps60={}\nghost={}\ndas={}\narr={}\nsdr={}\nname={}\n",
            match self.theme {
                Theme::HighContrast => "hc",
                Theme::Arcade => "arcade",
                Theme::Sheet => "sheet",
            },
            b(self.low_spec),
            b(self.animations),
            b(self.sound),
            self.cell,
            if self.renderer == RendererKind::Canvas { "canvas" } else { "dom" },
            b(self.fps60),
            b(self.ghost),
            self.das,
            self.arr,
            self.sdr,
            self.name.replace('\n', " "),
        );
        out.push_str(&format!(
            "touch_size={}\ntouch_opacity={}\ntouch_left_handed={}\ntouch_layout={}\ntouch_vibration={}\ntouch_gestures={}\n",
            self.touch_size.clamp(80, 125), self.touch_opacity.clamp(40, 100),
            b(self.touch_left_handed), self.touch_layout.min(1), b(self.touch_vibration), b(self.touch_gestures),
        ));
        for (i, k) in self.keys.iter().enumerate() {
            out.push_str(&format!("key{i}={k}\n"));
        }
        out
    }
}

/// 키 코드를 사람이 읽기 쉬운 이름으로
pub fn key_label(code: &str) -> String {
    match code {
        "ArrowLeft" => "←".into(),
        "ArrowRight" => "→".into(),
        "ArrowUp" => "↑".into(),
        "ArrowDown" => "↓".into(),
        "Space" => "Space".into(),
        "ShiftLeft" | "ShiftRight" => "Shift".into(),
        "ControlLeft" | "ControlRight" => "Ctrl".into(),
        c if c.starts_with("Key") => c[3..].into(),
        c if c.starts_with("Digit") => c[5..].into(),
        c => c.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_saved_settings_keep_mobile_defaults() {
        let s = Settings::from_storage(
            "theme=sheet\nname=기존 사용자\ndas=210\nkey0=KeyA\nunknown=value",
        );
        assert_eq!(s.theme, Theme::Sheet);
        assert_eq!(s.name, "기존 사용자");
        assert_eq!(s.das, 210);
        assert_eq!(s.keys[0], "KeyA");
        assert_eq!(
            (s.touch_size, s.touch_opacity, s.touch_layout),
            (100, 100, 0)
        );
        assert!(!s.touch_left_handed && !s.touch_vibration && !s.touch_gestures);
    }

    #[test]
    fn rotate_180_key_has_default_and_round_trips() {
        let s = Settings::default();
        assert_eq!(s.action_for_code("KeyA"), Some(Input::Rotate180));
        // 180도 회전이 생기기 전 저장된 설정에는 key7 이 없으므로 기본 키를 쓴다.
        let old = Settings::from_storage("key6=KeyC");
        assert_eq!(old.keys[7], "KeyA");
        // 예전 설정에서 A를 이미 다른 동작에 썼다면 겹치지 않는 키를 쓴다.
        let clash = Settings::from_storage("key0=KeyA");
        assert_eq!(clash.action_for_code("KeyA"), Some(ACTIONS[0].0));
        assert_eq!(clash.keys[7], "KeyQ");
        assert_eq!(clash.action_for_code("KeyQ"), Some(Input::Rotate180));
        let mut s = Settings::default();
        s.keys[7] = "KeyQ".into();
        let loaded = Settings::from_storage(&s.to_storage());
        assert_eq!(loaded.action_for_code("KeyQ"), Some(Input::Rotate180));
        assert_eq!(Settings::from_storage("key8=KeyP").keys, Settings::default().keys, "unknown key slot is ignored");
    }

    #[test]
    fn competitive_preset_only_changes_handling() {
        let mut s = Settings::default();
        s.name = "테스트".into();
        s.keys[0] = "KeyJ".into();
        let before = s.clone();
        s.apply_competitive_preset();
        assert_eq!((s.das, s.arr, s.sdr), (COMPETITIVE_DAS, 0, 0));
        assert_eq!(Settings { das: before.das, arr: before.arr, sdr: before.sdr, ..s.clone() }, before);
        assert_eq!(Settings::from_storage(&s.to_storage()), s);
    }

    #[test]
    fn mobile_values_are_bounded_and_malformed_values_use_defaults() {
        let s = Settings::from_storage("touch_size=999\ntouch_opacity=0\ntouch_layout=9");
        assert_eq!(
            (s.touch_size, s.touch_opacity, s.touch_layout),
            (125, 40, 1)
        );
        let s = Settings::from_storage("touch_size=0\ntouch_opacity=999");
        assert_eq!((s.touch_size, s.touch_opacity), (80, 100));
        let s = Settings::from_storage("touch_size=bad\ntouch_opacity=-1\ntouch_layout=bad");
        assert_eq!(
            (s.touch_size, s.touch_opacity, s.touch_layout),
            (100, 100, 0)
        );
    }

    #[test]
    fn mobile_settings_round_trip_without_losing_other_preferences() {
        let mut s = Settings::default();
        s.sound = true;
        s.name = "테스트".into();
        s.touch_size = 115;
        s.touch_opacity = 60;
        s.touch_left_handed = true;
        s.touch_layout = 1;
        s.touch_vibration = true;
        s.touch_gestures = true;
        assert_eq!(Settings::from_storage(&s.to_storage()), s);
    }
}
