# 성능 측정 보고서

측정 일자: 2026-11-13 · 원본 데이터: [`docs/perf-results/`](perf-results/) · 재현: `python tools/measure.py`

## 요약

| 예산 / 목표 | 결과 | 판정 |
|---|---|---|
| WASM 초기 다운로드 압축 크기 ≤ 1MB | **124.9KB** (brotli, 원본 297.4KB) · 아케이드 화면 추가 후 **171.2KB** (원본 434.2KB) | 충족 |
| 초기 화면 전체 전송량 | **135.5KB** (HTML+JS+CSS+WASM, 외부 폰트·이미지·CDN 없음) | — |
| 첫 게임판 표시 (소프트웨어 렌더링) | Edge 91ms · CPU 4배 감속 345ms · Chrome 370ms | — |
| 게임판 변경 시 전체 DOM 재생성 안 함 | 셀 DOM/캔버스는 1회 생성 후 바뀐 셀만 갱신 | 충족 |
| 입력 처리 중 동기식 네트워크 요청 없음 | WebSocket 비동기 전송 + 로컬 예측 | 충족 |
| 저사양 모드에서 지속 애니메이션 루프 없음 | 대기실 유휴: 렌더 0회/초, 메인 스레드 0.06% | 충족 |
| 키 입력 → 화면(DOM) 반영 | 6인 대전 소프트웨어 렌더링 p50 **0.5ms** / p95 1.0ms | — |
| 6인 대전 시 클라이언트 메인 스레드 | 플레이어당 **2.6~3.1%** (CPU 4배 감속 시 12.8~14.4%) | — |
| 6인 대전 서버 CPU / 메모리 | 1코어의 **1.7%** / RSS 8.4MB | — |

예산을 넘긴 항목은 없습니다. 측정하지 못한 항목은 [측정하지 못한 것](#측정하지-못한-것)에 적었습니다.

## 측정 환경

| 항목 | 값 |
|---|---|
| CPU / RAM / GPU | Intel Core i7-1165G7 (4코어 8스레드) / 16GB / Intel Iris Xe (내장) |
| OS | Windows 11 Home 10.0.26200 |
| 브라우저 | Microsoft Edge 157.0.4341.46, Google Chrome 152.0.7977.83 (Playwright headless) |
| 서버 | `bs-server` release 빌드, 같은 PC (localhost, RTT 약 1ms) |
| 빌드 | `trunk build --release` (opt-level=z, LTO, wasm-opt -Oz) |

### 하드웨어 가속 ON / OFF 확인 방법

각 시나리오마다 CDP `SystemInfo.getInfo` 로 실제 상태를 기록했습니다 (원본 JSON 의 `gpu` 필드).

| 구성 | 실행 인자 | 2D Canvas | 합성 | 래스터화 | 장치 |
|---|---|---|---|---|---|
| GPU ON | (기본값) | enabled | enabled | enabled | Intel Iris Xe |
| GPU OFF | `--disable-gpu` | unavailable_software | disabled_software | disabled_software | SwiftShader (CPU) |

"CPU 4배 감속"은 저사양 사무용 PC 근사치로, DevTools 와 같은 `Emulation.setCPUThrottlingRate(4)` 를 사용했습니다.

## 1. 초기 로딩 (Edge, localhost)

| 파일 | 전송(압축) | 원본 |
|---|---|---|
| `bs-client_bg.wasm` | 124,851 B | 297,388 B |
| `bs-client.js` (wasm-bindgen 접착 코드) | 7,245 B | 26,962 B |
| `style.css` | 2,398 B | 6,724 B |
| **합계 (HTML 포함)** | **135,530 B** | |

| 구성 | DOMContentLoaded | load | 첫 게임판 표시 (`bs-board-ready`) |
|---|---|---|---|
| Edge GPU ON | 106ms | 107ms | 177ms |
| Edge GPU OFF | 45ms | 45ms | 91ms |
| Edge GPU OFF + CPU 4배 감속 | 91ms | 93ms | 345ms |
| Chrome GPU OFF | 330ms | 330ms | 370ms |
| Chrome GPU OFF + CPU 4배 감속 | 378ms | 379ms | 531ms |

최종 빌드(cargo `strip` 대신 wasm-opt 로 디버그 정보 제거)는 WASM 원본 298,547B / brotli 124,811B 로 측정 빌드와 0.1KB 이내 차이입니다.
localhost 기준이므로 네트워크 전송 시간은 거의 0 입니다. 135KB 는 10Mbps 회선에서 약 0.1초 분량입니다.

## 2. 렌더러 비교: HTML 셀(CSS Grid) vs Canvas 2D

요구사항에 따라 HTML + CSS Grid 방식을 먼저 구현하고, Canvas 2D(소프트웨어 래스터화 가능, WebGL 미사용)와
같은 부하로 비교했습니다. 두 렌더러 모두 이전 셀 값을 보관해 **바뀐 셀만** 갱신합니다.

부하(`?bench=dom|canvas&opps=5`, 서버 없이 페이지 안에서 재현): 내 게임 60Hz tick, 초당 입력 10회,
상대 5명 판 10Hz 갱신. 경과 시간 기준으로 진행하므로 CPU 가 느려도 초당 작업량이 같습니다.
20초씩 측정(2차 측정은 축소판 격자선 제거 직전 빌드로, 렌더러 비교 조건은 동일). "메인 스레드"는 렌더러 프로세스 메인 스레드 사용률이고, "브라우저 CPU"는 브라우저의 모든 프로세스
(GPU/합성 프로세스 포함) CPU 시간을 1코어 기준 %로 합산한 값입니다.

| Edge | 메인 스레드 % (1차 / 2차) | 브라우저 CPU % (1차 / 2차) | 내 판 갱신 p50 / max |
|---|---|---|---|
| HTML 셀, GPU ON | 4.56 / 4.41 | 13.5 / 12.2 | 0.1 / 0.3ms |
| **Canvas 2D, GPU ON** | **2.47 / 2.50** | **5.0 / 7.8** | 0.1 / 0.4ms |
| HTML 셀, GPU OFF | 4.20 / 4.40 | 10.5 / 8.6 | 0.1 / 0.4ms |
| **Canvas 2D, GPU OFF** | **2.89 / 2.91** | **6.9 / 5.6** | 0.1 / 0.5ms |
| HTML 셀, GPU OFF, CPU 4배 감속 | 33.6 / 31.4 | (측정 제외*) | 1.8 / 8.0ms |
| **Canvas 2D, GPU OFF, CPU 4배 감속** | **24.2 / 25.3** | (측정 제외*) | 1.5 / 7.7ms |

Chrome GPU OFF 에서도 같은 경향입니다: 메인 스레드 HTML 4.47% vs Canvas 3.03%, CPU 4배 감속 시 33.6% vs 24.7%.

\* CDP CPU 감속은 대기 루프로 구현되어 프로세스 CPU 시간을 부풀리므로 감속 시나리오의 브라우저 CPU 는 기록하지 않았습니다.
메인 스레드 %는 감속된 시간 기준입니다.

**결정: Canvas 2D 를 기본 렌더러로 선택했습니다.** 소프트웨어 렌더링에서 메인 스레드 약 30%, 브라우저 전체 CPU 약 35% 적게 썼고,
두 브라우저와 두 번의 반복 측정에서 같은 결과가 나왔습니다. HTML 셀 렌더러는 설정에서 선택할 수 있습니다.
(벤치마크의 메인 스레드 시간에는 페이지 안에서 돌리는 6개 게임 시뮬레이션과 봇 계산이 포함되어 있습니다.
실제 대전에서는 이 계산을 서버가 하므로 아래 3절의 값이 더 낮습니다.)

## 3. 실제 서버 대전 (2인 / 6인)

플레이어마다 **별도 브라우저 프로세스**를 띄워 같은 방에 접속하고(실제로는 각자 다른 PC), 플레이어당 초당 약 6회
키 입력을 20초 동안 보냈습니다. 렌더러는 기본값(Canvas 2D).

| Edge | 키→DOM p50 / p95 | 키→다음 프레임 p50 | 메인 스레드 %/인 | 브라우저 CPU % (합계) | 브라우저 메모리 (합계) | 서버 CPU % |
|---|---|---|---|---|---|---|
| 2인, GPU ON | 0.5 / 0.9ms | 2.1ms | 2.3 | 23.4 (2개) | 457MB | 1.79 |
| 2인, GPU OFF | 0.5 / 0.9ms | 1.2ms | 2.5 | 11.3 (2개) | 318MB | 0.39 |
| 6인, GPU OFF | 0.5 / 1.0ms | 1.1ms | 2.6~3.1 | 40.6 (6개, 인당 약 6.8) | 955MB (인당 약 159MB) | 1.72 |
| 6인, GPU OFF, CPU 4배 감속 | 3.0 / 8.2ms | 6.1ms | 12.8~14.4 | (측정 제외) | 903MB | 2.18 |

Chrome GPU OFF 6인: 키→DOM p50 0.5ms / p95 0.9ms, 메인 스레드 3.1~3.6%/인, 서버 CPU 0.62%.

- **앱 자체 메모리**: JS 힙 2.4~4.0MB, WASM 선형 메모리 1.1~1.2MB. 브라우저 메모리의 대부분(인스턴스당 약 150MB)은 브라우저 자체 기본 사용량입니다(대기실 유휴 상태도 150MB).
- **서버 메모리**: RSS 7~9MB (6인 대전 중 8.4MB).
- **수신 대역폭**: 6인 대전 중 클라이언트당 약 2.8~2.9KB/s (상태 표시줄 값).
- **화면 갱신 횟수**: 페이지당 초당 약 19회. 30fps 상한 + 변경 시에만 갱신.
- GPU ON 의 브라우저 CPU 가 OFF 보다 높게 나온 것은 GPU 프로세스의 고정 비용으로 보입니다. 이 앱은 GPU 가속에서 이득이 거의 없습니다.

### 입력 지연 측정 방식

- **키→DOM**: `KeyboardEvent.timeStamp` 부터 바뀐 셀을 DOM/캔버스에 쓴 직후까지. 입력은 rAF 를 기다리지 않고 키 핸들러 안에서 바로 그립니다.
- **키→다음 프레임**: 같은 시작점부터 그 변경이 그려질 프레임의 `requestAnimationFrame` 콜백 시작까지. 이 뒤에 페인트·합성·모니터 출력이 이어지며, 그 부분은 JavaScript 로 잴 수 없습니다(수동 절차 참고).
- 로컬 예측 때문에 네트워크 왕복은 입력 반응에 포함되지 않습니다. 서버 상태와 예측이 다르면 서버 상태가 도착할 때 보정됩니다.

## 4. 유휴 상태

| 시나리오 | 메인 스레드 % | 화면 갱신/초 | JS 힙 |
|---|---|---|---|
| 대기실 접속 후 10~20초 (Edge GPU OFF) | 0.06 | 0 | 2.9MB |
| 같은 조건 Chrome | 0.04 | 0 | — |

지속적인 `requestAnimationFrame` 루프가 없다는 것을 확인했습니다. 남은 작업은 1초 주기 성능 요약과 5초 주기 RTT 핑뿐이며,
둘 다 비활성 탭에서는 브라우저가 자동으로 늦춥니다.

## 5. 저사양 모드

저사양 모드: 애니메이션 끔, 60fps 설정 무시(30fps), 상대 판 4Hz·셀 6px.

| Edge GPU OFF, CPU 4배 감속 | 메인 스레드 % | 화면 갱신/초 |
|---|---|---|
| Canvas 2D 기본 | 24.2 | 17.9 |
| Canvas 2D 저사양 | 27.1 | 13.7 |
| HTML 셀 기본 | 33.6 | 18.1 |
| HTML 셀 저사양 | 31.4 | 13.8 |

저사양 모드는 화면 갱신 횟수를 약 25% 줄였지만, **메인 스레드 사용률 차이는 측정 오차 범위 안**이었습니다.
벤치마크 시간의 대부분이 렌더링이 아니라 페이지 안의 게임 시뮬레이션이기 때문으로 보이며, 기본 모드가 이미
충분히 가볍다는 뜻이기도 합니다. 저사양 모드의 효과를 과장하지 않기 위해 그대로 기록합니다.

## 6. 아케이드 모드 테마 비용

아케이드 모드는 블록 셀마다 입체 테두리를 그립니다 (Canvas: 사각형 4개 추가, HTML 셀: 테두리 색만 다름).
같은 벤치마크(상대 5명, Edge GPU OFF, 15초)로 비교했습니다.

| 메인 스레드 % | Spreadsheet Mode | 아케이드 모드 |
|---|---|---|
| Canvas 2D | 2.88 | 2.85 |
| HTML 셀 | 4.33 | 4.02 |
| Canvas 2D, CPU 4배 감속 | 21.4 | 19.5 |
| HTML 셀, CPU 4배 감속 | 23.2 | 26.5 |

테마에 따른 차이는 측정 오차 범위 안이었습니다.

이후 아케이드 모드에 전용 화면 구조(큰 상대 판, NEXT 5개, 오버레이 등)를 넣은 뒤 다시 측정한 값(GPU OFF, 15초)은
Canvas 2D 2.95%(Spreadsheet) / 3.40%(아케이드), HTML 셀 4.81% / 4.79% 입니다. 아케이드의 Canvas 값이 조금 높은 것은
상대 판 셀이 8px → 12px 로 커지고 미리보기가 3개 → 5개로 늘어난 영향으로 보입니다. 내 판 갱신 시간도 두 테마 모두 p50 0.1ms(감속 시 1.7~1.8ms)입니다.

## 측정하지 못한 것

| 항목 | 상태 | 대체 수단 |
|---|---|---|
| 실제 저사양 PC (예: 2코어 셀러론, 4GB) | **테스트하지 못함** | i7-1165G7 에서 CPU 4배 감속으로 근사 |
| 브라우저 설정 화면에서 "그래픽 가속 사용"을 끈 상태 (창이 보이는 일반 실행) | **테스트하지 못함** | `--disable-gpu` headless 로 대신했고, SwiftShader 소프트웨어 렌더링임을 `SystemInfo` 로 확인. 아래 수동 절차 참고 |
| 화면에 실제로 픽셀이 나타나기까지의 지연 (페인트·합성·디스플레이) | **측정하지 못함** | 다음 프레임 시작까지만 측정. 아래 DevTools 절차 참고 |
| 실제 네트워크 (사내망, 인터넷) | **테스트하지 못함** | localhost 만. 로컬 예측 구조라 키 반응에는 영향이 없고 서버 보정 지연만 늘어남 |
| 비활성 탭 동작 | 자동 측정 안 함 | 설계상 렌더링 정지·서버 진행 계속. 아래 수동 절차 참고 |
| 효과음 (사운드 ON) | 귀로 확인하지 않음 | 기본값 OFF 확인, 코드상 짧은 사인파 1회 재생 |
| Firefox, Safari | 테스트하지 않음 | Edge 157, Chrome 152 만 |
| headless vs 창 모드 차이 | 확인하지 않음 | 모든 자동 측정은 headless |

## 수동 테스트 절차 (그래픽 가속 OFF)

1. 서버 실행: `BS_DIST=client/dist ./target/release/bs-server`
2. 그래픽 가속 끄기
   - Edge: `edge://settings/system` → "사용 가능한 경우 그래픽 가속 사용" 끄기 → 다시 시작
   - Chrome: `chrome://settings/system` → "가능한 경우 그래픽 가속 사용" 끄기 → 다시 시작
   - 또는 명령줄: `msedge.exe --disable-gpu http://localhost:3000` (다른 Edge 창을 모두 닫은 뒤)
3. `edge://gpu` (또는 `chrome://gpu`) 의 Graphics Feature Status 에서 Canvas / Compositing / Rasterization 이
   "Software only" 인지 확인
4. 업무 환경 재현: 다른 탭 10개 이상(메일, 문서, 스프레드시트 등)을 열어 둔다
5. `http://localhost:3000` 접속 → 연습봇 추가 5회 → 게임 시작
6. 하단 상태 표시줄에서 확인: `입력→화면` (p50/p95), `판 갱신`, `렌더 n/s`, `수신 KB/s`
7. 세부 측정
   - 브라우저 작업 관리자(Shift+Esc): 이 탭과 "GPU 프로세스"의 CPU·메모리
   - DevTools → Performance → CPU 4x slowdown → 20초 기록 → Main 스레드의 Scripting / Rendering / Painting 비율,
     키 입력 이벤트부터 다음 Paint 까지의 시간(실제 픽셀 반영 지연)
   - DevTools 콘솔: `JSON.parse(__bsPerfDump())` 로 원본 샘플, `__bsPerfReset()` 으로 초기화
8. 비활성 탭: 게임 중 다른 탭으로 전환 → 10초 뒤 복귀 → 게임이 서버에서 계속 진행되었는지(블록 위치·점수) 확인.
   전환 중 해당 탭의 CPU 가 작업 관리자에서 0% 근처로 떨어지는지 확인
9. 설정 → 렌더러를 "HTML 셀" 로 바꿔 6~7번 반복 후 비교

## 재현 방법 (자동)

```bash
pip install playwright psutil
# 서버 실행 후
python tools/measure.py                       # Edge 전체 (약 6분)
python tools/measure.py --channel chrome      # Chrome
python tools/measure.py --only bench_ --out bench_repeat
```

결과는 `tools/out/*.json`, `tools/out/*.md` 에 저장됩니다.

### 측정 도구에 대한 참고

- headless Edge/Chrome 은 한 창에서 앞으로 가져오지 않은 탭의 `requestAnimationFrame` 을 초당 2~4회로 제한합니다.
  처음에는 이 때문에 다인 측정에서 입력 지연이 수백 ms 로 잘못 나왔고, 플레이어마다 별도 브라우저를 띄우도록 고쳤습니다.
  결과 JSON 의 `raf_per_sec_check` 로 각 페이지가 초당 약 60프레임이었음을 확인할 수 있습니다.
- 이 현상을 계기로 내 게임판의 입력 반영을 rAF 와 분리했습니다(키 핸들러에서 바로 그림). 이제 rAF 가 제한되는 환경에서도
  DOM 반영은 즉시 이루어집니다.
