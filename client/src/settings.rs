//! 사용자 설정. localStorage 에 "키=값" 줄 형식으로 저장한다 (JSON 의존성 없이 가볍게).

use bs_engine::game::Input;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Theme {
    /// 기본: Spreadsheet Mode
    Sheet,
    HighContrast,
    /// 게임 느낌의 어두운 테마 (독자 디자인)
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

pub const ACTIONS: [(Input, &str); 7] = [
    (Input::Left, "왼쪽 이동"),
    (Input::Right, "오른쪽 이동"),
    (Input::SoftDrop, "천천히 내리기"),
    (Input::HardDrop, "바로 내리기"),
    (Input::RotateCw, "시계 방향 회전"),
    (Input::RotateCcw, "반시계 방향 회전"),
    (Input::Hold, "보관"),
];

pub const DEFAULT_KEYS: [&str; 7] = ["ArrowLeft", "ArrowRight", "ArrowDown", "Space", "ArrowUp", "KeyZ", "KeyC"];

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
    pub keys: [String; 7],
    pub name: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            theme: Theme::Sheet,
            low_spec: false,
            animations: false,
            sound: false,
            cell: 22,
            renderer: RendererKind::Canvas,
            fps60: false,
            ghost: true,
            das: 150,
            arr: 40,
            sdr: 40,
            keys: DEFAULT_KEYS.map(String::from),
            name: String::new(),
        }
    }
}

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
        self.keys.iter().position(|k| k == code).map(|i| ACTIONS[i].0)
    }

    /// 저사양 모드를 반영한 실제 값
    pub fn effective_animations(&self) -> bool {
        self.animations && !self.low_spec
    }

    pub fn effective_fps60(&self) -> bool {
        self.fps60 && !self.low_spec
    }

    pub fn load() -> Settings {
        let mut s = Settings::default();
        let Some(raw) = storage().and_then(|st| st.get_item(STORAGE_KEY).ok().flatten()) else { return s };
        for line in raw.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
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
                "renderer" => s.renderer = if v == "dom" { RendererKind::Dom } else { RendererKind::Canvas },
                "fps60" => s.fps60 = b,
                "ghost" => s.ghost = b,
                "das" => s.das = v.parse().unwrap_or(150).min(500),
                "arr" => s.arr = v.parse().unwrap_or(40).min(200),
                "sdr" => s.sdr = v.parse().unwrap_or(40).min(200),
                "name" => s.name = v.chars().take(16).collect(),
                _ => {
                    if let Some(i) = k.strip_prefix("key").and_then(|n| n.parse::<usize>().ok()) {
                        if i < 7 && !v.is_empty() {
                            s.keys[i] = v.to_string();
                        }
                    }
                }
            }
        }
        s
    }

    pub fn save(&self) {
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
        for (i, k) in self.keys.iter().enumerate() {
            out.push_str(&format!("key{i}={k}\n"));
        }
        if let Some(st) = storage() {
            let _ = st.set_item(STORAGE_KEY, &out);
        }
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
