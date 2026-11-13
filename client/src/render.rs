//! 게임판 렌더러 두 가지.
//!
//! - `Dom`: 셀 DOM 요소를 처음 한 번만 만들고, 이후에는 값이 바뀐 셀의 className 만 교체
//! - `Canvas`: Canvas 2D 컨텍스트에 바뀐 셀만 fillRect (WebGL/WebGPU 미사용)
//!
//! 두 방식 모두 이전 프레임 셀 값을 보관해 diff 로 최소 갱신만 수행한다.

use crate::settings::{RendererKind, Theme};
use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d, Element, HtmlCanvasElement};

/// 셀 코드별 class (0 빈칸, 1..7 블록, 8 방해줄, 9 고스트)
const CLASSES: [&str; 10] = ["c k0", "c k1", "c k2", "c k3", "c k4", "c k5", "c k6", "c k7", "c k8", "c k9"];

/// Canvas 용 팔레트 (style.css 의 값과 동일하게 유지)
const SHEET_FILL: [&str; 10] =
    ["#ffffff", "#cfe2f3", "#fff2cc", "#d9d2e9", "#d9ead3", "#f4cccc", "#c9daf8", "#fce5cd", "#d9d9d9", "#f3f3f3"];
const HC_FILL: [&str; 10] =
    ["#ffffff", "#0b5394", "#bf9000", "#674ea7", "#38761d", "#a61c00", "#1155cc", "#b45f06", "#434343", "#e6e6e6"];
const SHEET_GRID: &str = "#e2e2e2";
const HC_GRID: &str = "#7f7f7f";

/// 아케이드 모드: 어두운 판 + 선명한 블록. 입체감은 밝은/어두운 띠 fillRect 로만 표현 (그림자·글로우 없음)
const ARCADE_FILL: [&str; 10] =
    ["#121a2e", "#22c3e6", "#f5c518", "#a855f7", "#3ccf5a", "#ef4444", "#3b6cf6", "#f59e0b", "#6b7280", "#2a3552"];
const ARCADE_HI: [&str; 10] =
    ["#121a2e", "#8ae6f7", "#fbe27a", "#d3a6fb", "#9be8a9", "#f8a0a0", "#9db6fa", "#fbcd7b", "#a3a9b3", "#2a3552"];
const ARCADE_LO: [&str; 10] =
    ["#121a2e", "#0e7f99", "#a8860c", "#6b21a8", "#1f7a33", "#991b1b", "#1e3fa8", "#a16207", "#3f444c", "#2a3552"];
const ARCADE_GRID: &str = "#1c2540";
const ARCADE_BG: &str = "#0b1020";

pub struct DomBoard {
    root: Element,
    cells: Vec<Element>,
    prev: Vec<u8>,
}

pub struct CanvasBoard {
    root: Element,
    ctx: CanvasRenderingContext2d,
    prev: Vec<u8>,
    w: usize,
    cell: f64,
    off_x: f64,
    off_y: f64,
    /// 격자선 두께 (내 게임판 1px, 축소판/미리보기 0px)
    gap: f64,
    fill: &'static [&'static str; 10],
    /// 아케이드 모드 입체 테두리 (밝은 색, 어두운 색)
    bevel: Option<(&'static [&'static str; 10], &'static [&'static str; 10])>,
}

pub enum Board {
    Dom(DomBoard),
    Canvas(CanvasBoard),
}

pub struct BoardSpec {
    pub w: usize,
    pub h: usize,
    /// 행 번호 / 열 머리글 표시 (Spreadsheet Mode 의 내 게임판)
    pub headers: bool,
    /// 셀 사이 격자선 (내 게임판)
    pub grid: bool,
    /// Canvas 렌더러용 셀 크기(px)
    pub cell_px: u8,
}

fn doc() -> web_sys::Document {
    web_sys::window().unwrap().document().unwrap()
}

fn col_name(x: usize) -> String {
    ((b'A' + x as u8) as char).to_string()
}

impl Board {
    pub fn new(kind: RendererKind, theme: Theme, container: Element, spec: BoardSpec) -> Board {
        container.set_inner_html("");
        // :has() 선택자 대신 클래스로 레이아웃 전환 (자식 변경 시 선택자 재평가 방지)
        let _ = container.class_list().toggle_with_force("has-canvas", kind == RendererKind::Canvas);
        match kind {
            RendererKind::Dom => Board::Dom(DomBoard::new(container, spec)),
            RendererKind::Canvas => Board::Canvas(CanvasBoard::new(container, spec, theme)),
        }
    }

    pub fn is_connected(&self) -> bool {
        match self {
            Board::Dom(b) => b.root.is_connected(),
            Board::Canvas(b) => b.root.is_connected(),
        }
    }

    /// 바뀐 셀만 갱신하고, 갱신한 셀 수를 돌려준다
    pub fn update(&mut self, cells: &[u8]) -> u32 {
        match self {
            Board::Dom(b) => b.update(cells),
            Board::Canvas(b) => b.update(cells),
        }
    }
}

impl DomBoard {
    fn new(root: Element, spec: BoardSpec) -> DomBoard {
        let d = doc();
        let n = spec.w * spec.h;
        let mut cells = Vec::with_capacity(n);
        let frag = d.create_document_fragment();
        let mk = |cls: &str, text: Option<&str>| {
            let e = d.create_element("div").unwrap();
            e.set_class_name(cls);
            if let Some(t) = text {
                e.set_text_content(Some(t));
            }
            e
        };
        let _ = root.class_list().toggle_with_force("with-hd", spec.headers);
        if spec.headers {
            frag.append_child(&mk("hd corner", None)).unwrap();
            for x in 0..spec.w {
                frag.append_child(&mk("hd col", Some(&col_name(x)))).unwrap();
            }
        }
        for y in 0..spec.h {
            if spec.headers {
                frag.append_child(&mk("hd row", Some(&(y + 1).to_string()))).unwrap();
            }
            for _ in 0..spec.w {
                let e = mk(CLASSES[0], None);
                frag.append_child(&e).unwrap();
                cells.push(e);
            }
        }
        root.append_child(&frag).unwrap();
        DomBoard { root, cells, prev: vec![0; n] }
    }

    fn update(&mut self, cells: &[u8]) -> u32 {
        let mut changed = 0;
        for (i, (&v, p)) in cells.iter().zip(self.prev.iter_mut()).enumerate() {
            if v != *p {
                *p = v;
                self.cells[i].set_class_name(CLASSES[(v as usize).min(9)]);
                changed += 1;
            }
        }
        changed
    }
}

impl CanvasBoard {
    fn new(root: Element, spec: BoardSpec, theme: Theme) -> CanvasBoard {
        let d = doc();
        let canvas: HtmlCanvasElement = d.create_element("canvas").unwrap().unchecked_into();
        let cell = spec.cell_px as f64;
        let (off_x, off_y) = if spec.headers { (28.0, cell) } else { (0.0, 0.0) };
        let edge = if spec.grid { 1.0 } else { 0.0 };
        let css_w = off_x + cell * spec.w as f64 + edge;
        let css_h = off_y + cell * spec.h as f64 + edge;
        let dpr = web_sys::window().unwrap().device_pixel_ratio().clamp(1.0, 2.0);
        canvas.set_width((css_w * dpr) as u32);
        canvas.set_height((css_h * dpr) as u32);
        let _ = canvas.set_attribute("style", &format!("width:{css_w}px;height:{css_h}px;display:block"));
        canvas.set_class_name("cv");
        root.append_child(&canvas).unwrap();
        // alpha:false → 합성 비용 감소. WebGL 은 사용하지 않는다.
        let opts = js_sys::Object::new();
        let _ = js_sys::Reflect::set(&opts, &"alpha".into(), &false.into());
        let ctx: CanvasRenderingContext2d =
            canvas.get_context_with_context_options("2d", &opts).unwrap().unwrap().unchecked_into();
        let _ = ctx.scale(dpr, dpr);
        let (fill, grid, bevel, bg) = match theme {
            Theme::Sheet => (&SHEET_FILL, SHEET_GRID, None, if spec.headers { "#f8f9fa" } else { "#ffffff" }),
            Theme::HighContrast => (&HC_FILL, HC_GRID, None, if spec.headers { "#f8f9fa" } else { "#ffffff" }),
            Theme::Arcade => (&ARCADE_FILL, ARCADE_GRID, Some((&ARCADE_HI, &ARCADE_LO)), ARCADE_BG),
        };
        // 배경 + 격자 1회 그리기 (격자는 내 게임판에만)
        ctx.set_fill_style_str(bg);
        ctx.fill_rect(0.0, 0.0, css_w, css_h);
        let gap = if spec.grid { 1.0 } else { 0.0 };
        if spec.grid {
            ctx.set_fill_style_str(grid);
            ctx.fill_rect(off_x, off_y, cell * spec.w as f64 + 1.0, cell * spec.h as f64 + 1.0);
        }
        if spec.headers {
            ctx.set_font("11px system-ui, sans-serif");
            ctx.set_text_align("center");
            ctx.set_text_baseline("middle");
            ctx.set_fill_style_str("#5f6368");
            for x in 0..spec.w {
                let _ = ctx.fill_text(&col_name(x), off_x + cell * (x as f64 + 0.5), off_y / 2.0);
            }
            for y in 0..spec.h {
                let _ = ctx.fill_text(&(y + 1).to_string(), off_x / 2.0, off_y + cell * (y as f64 + 0.5));
            }
        }
        let mut b = CanvasBoard { root, ctx, prev: vec![u8::MAX; spec.w * spec.h], w: spec.w, cell, off_x, off_y, gap, fill, bevel };
        let zeros = vec![0u8; spec.w * spec.h];
        b.update(&zeros);
        b
    }

    fn update(&mut self, cells: &[u8]) -> u32 {
        let mut changed = 0;
        let mut last_color = usize::MAX;
        for (i, (&v, p)) in cells.iter().zip(self.prev.iter_mut()).enumerate() {
            if v == *p {
                continue;
            }
            *p = v;
            let ci = (v as usize).min(9);
            if ci != last_color {
                self.ctx.set_fill_style_str(self.fill[ci]);
                last_color = ci;
            }
            let x = (i % self.w) as f64;
            let y = (i / self.w) as f64;
            let g = self.gap;
            let (px, py, sz) = (self.off_x + x * self.cell + g, self.off_y + y * self.cell + g, self.cell - g);
            self.ctx.fill_rect(px, py, sz, sz);
            if let Some((hi, lo)) = self.bevel {
                if (1..=8).contains(&ci) {
                    // 위·왼쪽 밝은 띠, 아래·오른쪽 어두운 띠 → 단순 사각형 4개로 입체감
                    let b = (sz / 7.0).max(1.0).floor();
                    self.ctx.set_fill_style_str(hi[ci]);
                    self.ctx.fill_rect(px, py, sz, b);
                    self.ctx.fill_rect(px, py, b, sz);
                    self.ctx.set_fill_style_str(lo[ci]);
                    self.ctx.fill_rect(px, py + sz - b, sz, b);
                    self.ctx.fill_rect(px + sz - b, py, b, sz);
                    last_color = usize::MAX;
                }
            }
            changed += 1;
        }
        changed
    }
}
