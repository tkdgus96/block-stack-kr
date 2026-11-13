# Block Stack — Spreadsheet Mode

업무용 스프레드시트처럼 차분하고 가벼운 **2~6인 실시간 블록 퍼즐 대전 게임**.

- 게임 로직: 순수 Rust (`engine/`) — 클라이언트와 서버가 같은 코드를 사용
- 프론트엔드: Leptos 0.8 (CSR) + Rust/WASM (`client/`)
- 백엔드: Axum 0.8 + Tokio WebSocket (`server/`)
- WebGL/WebGPU/게임 엔진 미사용. 기본 사운드·애니메이션 OFF

성능 측정 결과와 방법은 [docs/PERFORMANCE.md](docs/PERFORMANCE.md) 참고.

## 빠른 시작

```bash
# 1회: 도구 설치
rustup target add wasm32-unknown-unknown
# trunk: https://github.com/trunk-rs/trunk/releases 에서 바이너리 또는 `cargo install trunk`

# 빌드
(cd client && trunk build --release)          # → client/dist (WASM 원본 약 434KB, brotli 전송 약 171KB)
cargo build --release -p bs-server

# 실행 (http://localhost:3000)
BS_DIST=client/dist ./target/release/bs-server  # PowerShell: $env:BS_DIST="client/dist"; .\target\release\bs-server.exe
```

브라우저에서 접속하면 방이 자동으로 만들어지고 URL 이 `?room=코드` 로 바뀝니다.
상단 **링크 복사**로 같은 방에 초대합니다. 방장이 **게임 시작**을 누르면 3초 후 시작합니다.
혼자 연습할 때는 **연습봇 추가**(최대 6명까지)를 사용하세요.
상단 **방 목록**(또는 하단 "방 목록" 탭)에서 지금 열려 있는 방(방 코드, 방장, 인원, 대기/게임 중)을 보고 바로 참가할 수 있습니다.
게임 중인 방에 들어가면 다음 판부터 참가합니다. 목록은 탭을 열 때와 "새로고침"을 누를 때 갱신됩니다.

| 환경 변수 | 기본값 | 설명 |
|---|---|---|
| `PORT` | 3000 | 서버 포트 |
| `BS_DIST` | `client/dist` | 정적 파일(WASM 클라이언트) 경로 |

개발 중에는 `cargo run -p bs-server` 와 `cd client && trunk serve` (8080, `/ws` 를 3000 으로 프록시)를 함께 실행해도 됩니다.

### Windows + 한글 사용자 이름 경로

GNU 툴체인(`x86_64-pc-windows-gnu`)의 MinGW `ld`/`dlltool` 은 `C:\Users\한글이름\...` 같은 비ASCII 경로를
열지 못합니다. 이 저장소는 다음 구성으로 빌드·검증했습니다.

- Rust: `RUSTUP_HOME=C:\rust\rustup`, `CARGO_HOME=C:\rust\cargo`
- MinGW-w64 (WinLibs): `C:\rust\mingw64\bin` 을 PATH 에 추가 (`dlltool` 이 `as` 를 필요로 함)
- `CARGO_TARGET_DIR=C:\bs-target`, `TEMP=C:\rust\tmp`

`. scripts/env.sh` (Git Bash) 또는 `. .\scripts\env.ps1` (PowerShell) 로 한 번에 설정됩니다.
이 경우 산출물은 `C:\bs-target\release\bs-server.exe` 에 생깁니다.
Visual Studio Build Tools(MSVC 툴체인)를 쓰면 이 설정은 필요 없습니다.

## 테스트

```bash
cargo test -p bs-engine   # 엔진 20개: 결정성, 회전/벽, 줄 삭제, 공격/상쇄, 방해줄, 고정 지연, 봇
cargo test -p bs-server   # 방 통합 테스트: 입장→봇→시작→탈락→GameOver→승수, 7번째 입장 거부
python tools/measure.py   # 실제 Edge/Chrome 으로 성능 측정 (서버 실행 필요, pip install playwright psutil)
```

## 구조

```
engine/   순수 Rust 게임 엔진 (의존성: serde + postcard 만)
  game.rs      보드 10×20(+숨김 2줄), 7-bag, 회전 보정, 고정 지연, 줄 삭제, 콤보, 공격/상쇄/방해줄
  bot.rs       연습봇 (회전×열 전수 평가, 봇마다 다른 가중치)
  protocol.rs  WebSocket 메시지 (postcard 바이너리)
server/   Axum + Tokio
  main.rs      정적 파일(br/gzip 압축) + /ws
  room.rs      방 = Tokio 태스크 1개. 60Hz 고정 tick 으로 모든 플레이어 게임을 권위 있게 진행
client/   Leptos CSR → WASM
  app.rs       Spreadsheet Mode UI (제목/메뉴/수식 입력줄/시트 탭/상태 표시줄, 점수표)
  runtime.rs   네트워크, 입력 예측·보정, DAS/ARR, 렌더 스케줄링, 성능 계측
  render.rs    게임판 렌더러: Canvas 2D(기본) / HTML 셀(CSS Grid), 둘 다 변경된 셀만 갱신
tools/measure.py  Playwright 기반 성능 측정
docs/PERFORMANCE.md  측정 결과·방법·한계·수동 테스트 절차
```

### 실시간 동기화

1. 서버 방 태스크가 60Hz 로 모든 게임을 진행(중력·고정·공격 분배·탈락 판정). 브라우저 탭이 비활성이어도 게임은 서버에서 계속된다.
2. 클라이언트는 입력을 `seq` 와 함께 비동기로 보내고(응답을 기다리지 않음), 같은 엔진으로 **즉시 로컬 예측**해서 그린다.
3. 서버는 상태가 바뀐 tick 에만 `YourState{ack, game}` 를 보낸다. 클라이언트는 서버 상태 위에 아직 처리되지 않은 입력(`seq > ack`)을 다시 적용해 보정한다.
4. 상대 게임판은 10Hz, 4비트 압축(100바이트)으로만 전송된다. 점수표는 2Hz.

### 렌더링 원칙

- 지속적인 `requestAnimationFrame` 루프 없음. 상태가 바뀔 때만 1회 예약
- 키 입력은 핸들러 안에서 바로 바뀐 셀만 그린다 (레이아웃 읽기 없음 → 강제 레이아웃 없음)
- 서버/상대 판 갱신은 기본 30fps 상한(설정에서 60fps), 상대 판은 10Hz(저사양 4Hz)
- 게임판은 Leptos 반응형 시스템 밖에서 명령형으로 갱신 → 게임판 변경이 페이지 재렌더링으로 이어지지 않음
- Leptos 시그널은 점수표·로그·연결 상태처럼 드물게 바뀌는 UI 에만 사용

## 조작 (설정 시트에서 변경 가능)

| 동작 | 기본 키 |
|---|---|
| 이동 | ← → (DAS 150ms, ARR 40ms) |
| 천천히 내리기 | ↓ |
| 바로 내리기 | Space |
| 회전 | ↑ / X, 반대 회전 Z |
| 보관 | C |

## 설정

Spreadsheet Mode(기본) / 고대비 모드 / 아케이드 모드, 저사양 모드, 애니메이션, 사운드, 고스트 블록, 60fps, 게임판 크기(16/22/28px),
렌더러(Canvas 2D / HTML 셀), DAS·ARR·내리기 반복, 키 변경. 브라우저 localStorage 에 저장됩니다.

## 디자인 메모

**아케이드 모드**: 메뉴의 "아케이드 모드" 버튼(또는 설정 → 테마)으로 전환하면 색뿐 아니라 화면 구조 전체가 바뀝니다
(`client/src/arcade.rs`). 블록 퍼즐 대전 장르의 일반적인 배치를 따랐습니다.

- 가운데 큰 내 게임판, 왼쪽 HOLD + SCORE/LINES/LEVEL/공격/상쇄, 오른쪽 NEXT 5개 + 받을 공격
- 게임판 옆 받을 공격 게이지, 이름표·순위 배지(금/은/동)·KO 수
- 1:1 이면 큰 상대 판 하나, 다인이면 상대 판 슬롯 5개(빈 자리 표시)
- 게임판 위 카운트다운 / K.O. / WIN!·순위 표시, 하단 순위 칩
- 서버가 KO 를 집계합니다 (탈락한 플레이어에게 마지막으로 공격을 보낸 사람)

특정 게임의 이름·로고·캐릭터·그래픽을 따라 하지 않은 독자 디자인이며, 모든 표시는 정지 상태입니다
(블러·글로우·그림자·애니메이션 없음).
"업무 화면" 버튼 한 번으로 Spreadsheet Mode 로 돌아갑니다.

일반적인 스프레드시트 UI 패턴(행 번호·열 머리글, 이름 상자, 수식 입력줄, 시트 탭, 상태 표시줄)만 사용했고,
특정 제품의 로고·아이콘·그래픽 에셋은 사용하지 않습니다. 이미지 파일이 없고 아이콘은 텍스트 문자(▦)입니다.
