# Block Stack — Multiplayer Arena

**2~6인 실시간 블록 퍼즐 대전 게임**. 아케이드 아레나를 기본으로 제공하며, 버튼 하나로 차분한 스프레드시트 화면으로 전환할 수 있습니다. 기존에 저장한 테마 선택은 유지됩니다.

- 게임 로직: 순수 Rust (`engine/`) — 클라이언트와 서버가 같은 코드를 사용
- 프론트엔드: Leptos 0.8 (CSR) + Rust/WASM (`client/`)
- 백엔드: Axum 0.8 + Tokio WebSocket (`server/`)
- WebGL/WebGPU/게임 엔진 미사용. 기본 사운드·애니메이션 OFF

성능 측정 결과와 방법은 [docs/PERFORMANCE.md](docs/PERFORMANCE.md) 참고.

## 공격과 콤보

게임판 아래에는 현재 **COMBO / B2B**가 유지되고, 공격할 때 기술명과 **생성·상쇄·실제 전송** 줄 수가 표시됩니다. 연속 삭제 다음부터 1 COMBO이며 줄을 지우지 못하면 콤보가 끊깁니다. 높은 콤보는 색과 숫자 크기로 구분합니다. 실제 공격을 받는 상대 게임판과 내 공격 게이지를 짧게 강조합니다. 모바일과 업무 화면에서도 같은 상태를 보여줍니다.

T-Spin Full/미니, 테트리스, B2B, 퍼펙트 클리어를 판정합니다. Full 싱글/더블/트리플의 기본 공격은 2/4/6줄이며, 두 번째 연속 특수 줄 삭제부터 B2B +1이 붙습니다. 콤보 공격과 PC +4를 합산한 뒤 대기 중 공격부터 상쇄합니다. 받은 공격은 0.5초가 지나야 게임판에 올라오므로 그 사이에 줄을 지우면 상쇄할 수 있습니다. 3인 이상 방에서는 공격 대상 방식(무작위, 위험한 상대, 반격)을 고를 수 있습니다. 회전은 전이별 SRS 보정을 사용합니다. 상세한 판정·점수·예외는 [docs/ATTACK_RULES.md](docs/ATTACK_RULES.md)를 참고하세요.

## 대결과 성장

경기 종료 후 결과표에서 순위·공격 줄·APM·KO·점수와 직전 경기 대비 변화를 확인합니다. **준비 / 재대결**을 모두 누르면 같은 방에서 다음 판이 자동으로 시작됩니다. 봇은 자동 준비하며, 인원·팀·방 규칙을 바꾸면 준비가 초기화됩니다.

**방 설정 → 규칙 변경**에서 방장이 대기 중에 공개/비공개, 1·2·3분, 최대 인원(2~6명), 봇 난이도를 선택합니다. 기본 대결 시간은 2분입니다. 비공개 방은 목록에 나오지 않으며 링크나 코드를 아는 사람은 참가할 수 있습니다.

**팀전**은 정원 4명 또는 6명을 모두 채운 2:2·3:3으로 시작합니다. 각자 A/B 팀을 고르고 봇은 자동 배정됩니다. 공격은 상대 팀에게만 보내며 한 팀만 생존하면 종료합니다. 시간 종료 시 팀 생존자 수 → 전체 공격 줄 → KO → 점수 합계 순으로 판정합니다. 탈락한 팀원도 팀 승리를 함께 받고, 동점이면 양 팀 무승부입니다.

상단 **개인 기록** 메뉴에는 최고 APM, 평균 공격량, 대결 승률, 최근 50경기를 닉네임과 함께 저장합니다. 누적 합계는 최근 50경기 제한과 별도로 유지됩니다. 기록은 로그인 없이 현재 브라우저에 저장되며 기기 간 동기화되지 않습니다. 연습은 승률에서 제외하고, 관전·재접속 시 과거 결과를 중복 저장하지 않습니다.

**설정 → 모바일 터치 조작**에서 버튼 크기·불투명도·왼손 모드·버튼 행 배치·진동을 바꿀 수 있습니다. 최소 조작 영역은 44px이며 진동 미지원 기기에서는 진동만 건너뜁니다.

## 배포

골프 조인과 같은 **Oracle + systemd/Caddy 서버 → Cloudflare Pages 웹 화면 → 브라우저 검증** 흐름은 [docs/DEPLOYMENT.md](docs/DEPLOYMENT.md)를 참고하세요. 서버는 별도 `127.0.0.1:3010` 서비스를 사용합니다.

## 빠른 시작

```bash
# 1회: 도구 설치
rustup target add wasm32-unknown-unknown
# trunk: https://github.com/trunk-rs/trunk/releases 에서 바이너리 또는 `cargo install trunk`

# 빌드
(cd client && trunk build --release)          # → client/dist
cargo build --release -p bs-server

# 실행 (http://localhost:3000)
BS_DIST=client/dist ./target/release/bs-server  # PowerShell: $env:BS_DIST="client/dist"; .\target\release\bs-server.exe
```

브라우저에서 접속하면 방이 자동으로 만들어지고 URL 이 `?room=코드` 로 바뀝니다.
상단 **링크 복사**로 같은 방에 초대합니다. 방장이 **게임 시작**을 누르면 3초 후 시작합니다.
2인 이상 대결(봇 포함)은 **2분 제한**이며 한 명만 생존하면 조기 종료합니다. 시간이 끝나면 생존자를 우선하고 **보낸 공격 줄 → KO → 점수** 순으로 순위를 정합니다. 같은 기록은 공동 순위이며 공동 1위에는 승수를 추가하지 않습니다. 혼자 시작하는 연습은 시간 제한이 없고, **연습봇 추가**로 대결할 수도 있습니다(최대 6명).

게임판에는 이번 경기의 보낸 공격 줄과 **분당 공격 줄(APM)**을 표시하고, 하단 공격 통계에서 개인별 이번 경기·APM·경기당 평균·완료 경기 수를 볼 수 있습니다. APM은 탈락 전 생존 시간 기준이며 경기당 평균은 현재 방에서 완료한 경기의 누적 공격량 ÷ 완료 판수입니다. 재접속하면 통계가 초기화됩니다.
상단 **방 찾기**(스프레드시트 화면에서는 **방 목록**)에서 지금 열려 있는 방(방 코드, 방장, 인원, 대기/게임 중)을 보고 바로 참가할 수 있습니다.
게임 중인 방에 들어가면 다음 판부터 참가합니다. 목록은 탭을 열 때와 "새로고침"을 누를 때 갱신됩니다. **새 방 만들기**, **코드로 참가**도 지원합니다.

방에 들어가면 오른쪽 라운지에서 참가자를 확인하고 채팅할 수 있습니다. 최근 30줄은 입장한 사람에게 전달됩니다. 메시지는 최대 120자이며 3초에 5줄까지 전송할 수 있습니다. 채팅 입력 중에는 게임 키가 작동하지 않고, Esc로 입력창에서 나와 게임을 조작합니다. 한글 조합 중 Enter는 전송하지 않습니다.

| 환경 변수 | 기본값 | 설명 |
|---|---|---|
| `PORT` | 3000 | 서버 포트 |
| `BS_DIST` | `client/dist` | 정적 파일(WASM 클라이언트) 경로 |
| `BS_BIND` | `0.0.0.0:$PORT` | 서버 수신 IP와 포트. 배포 시 `127.0.0.1:3010` |
| `BS_WS_URL` | 현재 주소의 `/ws` | 웹 빌드 시 지정할 별도 게임 서버 주소 (`wss://…/ws`) |

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
cargo test -p bs-engine   # 엔진/프로토콜 35개: 결정성, 회전/벽, 줄 삭제, 공격/상쇄, 방해줄, 고정 지연, 봇
cargo test -p bs-server   # 방 통합 테스트: 입장→봇→시작→탈락→GameOver→승수, 7번째 입장 거부
python3 tools/smoke.py --url http://localhost:3000  # Chrome: 방·채팅·테마·게임 시작 통합 확인 (pip install playwright)
cargo test -p bs-client   # 로컬 기록·모바일 설정
python3 tools/smoke_battle.py --url http://localhost:3000  # 모바일 터치·대결 시간·통계 확인
python3 tools/smoke_evolution.py --url http://localhost:3000  # 결과·재대결·기록·방 설정·팀전
python3 tools/smoke_touch_settings.py --url http://localhost:3000  # 터치 설정·저장·실제 조작
python3 tools/smoke_combat.py --url http://localhost:3000  # 전투 패킷 UI fixture: 기술명·콤보·강조·모바일
python3 tools/smoke_handling.py --url http://localhost:3000  # 180도 회전·즉시 내리기·ARR 0 유지·경쟁용 프리셋
python3 tools/smoke_battle_rules.py --url http://localhost:3000  # 3인 방 공격 대상 선택·곧 올라올 공격 표시
python3 tools/smoke_practice_modes.py --url http://localhost:3000  # 마라톤·Zen·퍼즐·키 입력 실수 표시
python3 tools/smoke_competitive.py --url http://localhost:3000  # 빠른 대전·봇 대체·전체 리플레이 공유·서버 순위표 (약 20초)
python3 tools/smoke_effects.py --url http://localhost:3000  # 줄 삭제 빛·큰 공격 흔들림·애니메이션 끔
python tools/measure.py   # 실제 Edge/Chrome 으로 성능 측정 (서버 실행 필요, pip install playwright psutil)
```

## 구조

```
engine/   순수 Rust 게임 엔진 (의존성: serde + postcard 만)
  game.rs      보드 10×20(+숨김 2줄), 7-bag, 회전 보정, 고정 지연, 줄 삭제, 콤보, 공격/상쇄/방해줄
  bot.rs       연습봇 (쉬움: 회전×열 평가 / 보통: 보관·다음 블록 / 어려움: 밀어 넣기·T스핀 탐색)
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
| 회전 | ↑ / X, 반대 회전 Z, 180도 회전 A |
| 보관 | C |

작은 화면(900px 이하)이나 터치 기기에는 게임판 아래 **터치 조작 버튼**이 나타납니다. 이동·소프트드롭은 길게 누를 수 있고, 이동과 회전을 동시에 누를 수 있습니다. 보관, 양방향 회전, 하드드롭을 지원하며 대기/관전/탈락 중에는 비활성화됩니다.

## 설정

아케이드 모드(기본) / Spreadsheet Mode / 고대비 모드, 저사양 모드, 애니메이션, 사운드, 고스트 블록, 60fps, 게임판 크기(16/22/28px),
렌더러(Canvas 2D / HTML 셀), DAS·ARR·내리기 반복, 키 변경. 브라우저 localStorage 에 저장됩니다.

## 디자인 메모

**아케이드 모드**: 처음 접속하면 아케이드 모드로 열립니다. "업무 화면"으로 전환한 뒤 메뉴의 "아케이드 모드" 버튼(또는 설정 → 테마)으로 돌아오면 색뿐 아니라 화면 구조 전체가 바뀝니다
(`client/src/arcade.rs`). 블록 퍼즐 대전 장르의 일반적인 배치를 따랐습니다.

- 큰 내 게임판, 왼쪽 HOLD + SCORE/LINES/LEVEL/공격/상쇄, 오른쪽 NEXT 5개 + 받을 공격. 창 크기에 따라 셀 크기를 조정
- 게임판 옆 받을 공격 게이지, 이름표·순위 배지(금/은/동)·KO 수
- 1:1 이면 큰 상대 판 하나, 다인이면 상대 판 슬롯 5개(빈 자리 표시)
- 게임판 위 카운트다운 / K.O. / WIN!·순위 표시, 하단 순위 칩
- 방 라운지의 봇 추가·게임 시작·참가자·채팅, 상단 방 찾기
- 줄 삭제·공격·상쇄·KO 시 1.4초 피드백 표시, 공격 대상 강조
- 서버가 KO 를 집계합니다 (탈락한 플레이어에게 마지막으로 공격을 보낸 사람)

Tetris Battle, 한게임 테트리스, 넷마블 테트리스의 대전판·상대 슬롯·방 라운지 구성을 참고했습니다. 조사 자료와 적용 내용은 [docs/UI_REFERENCES.md](docs/UI_REFERENCES.md)에 정리했습니다.
게임판은 변경된 셀만 갱신하고, 피드백 애니메이션은 설정에서 켠 경우에만 짧게 재생합니다. 저사양·동작 줄이기 설정을 존중합니다.
기존 성능 보고서는 이전 UI의 측정값이며, 이번 UI의 전체 성능 벤치마크는 다시 측정하지 않았습니다.
"업무 화면" 버튼 한 번으로 Spreadsheet Mode 로 돌아갑니다.

일반적인 스프레드시트 UI 패턴(행 번호·열 머리글, 이름 상자, 수식 입력줄, 시트 탭, 상태 표시줄)만 사용했고,
특정 제품의 로고·아이콘·그래픽 에셋은 사용하지 않습니다. 이미지 파일이 없고 아이콘은 텍스트 문자(▦)입니다.

## 연습과 성장

연습실(T스핀·콤보·상쇄·40줄·2분 도전·마라톤·Zen·퍼즐 10개, 키 입력 효율 표시), 빠른 대전 1:1(참고용 레이팅), 전체 경기 리플레이와 공유 링크, 서버 순위표, 최근 경기 리플레이와 APM 추이, 서버 프로필·일일 미션·해금 테마, 선택형 모바일 제스처, 협동 생존전을 제공합니다. 대기실에서는 내 P번호·방장·준비 상태를 구분합니다. [기능과 저장 범위](docs/GROWTH_FEATURES.md)를 참고하세요.
