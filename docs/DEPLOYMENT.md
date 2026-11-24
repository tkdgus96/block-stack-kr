# 골프 조인과 같은 배포 흐름

Oracle Ubuntu + systemd + Caddy에 게임 서버를, Cloudflare Pages에 WASM 웹 화면을 배포한다. 골프 조인의 `docs/ops/deploy.md`와 태그별 설치·전환 방식을 참고했다. 실제 골프 조인 설정이나 계정 정보는 이 저장소에 복사하지 않는다.

| 구성 | 설정 |
|---|---|
| 공개 웹 화면 | `https://block-stack-kr.pages.dev` (사용 가능한 프로젝트 이름으로 생성) |
| 게임 서버 | `https://GAME_HOST/healthz`, `wss://GAME_HOST/ws` |
| Oracle 내부 주소 | `127.0.0.1:3010`, `block-stack.service` |
| 게임 서버 릴리스 | `/opt/block-stack/releases/TAG`, `current`, `previous` |

`GAME_HOST`는 실제 서버를 가리키는 주소로 바꾼다. 전용 호스트를 만들거나 기존 호스트 아래 경로를 분리할 수 있다. 서버는 방 상태를 메모리에 보관하므로 인스턴스 하나로 운영하며, 재시작 시 진행 중인 방과 채팅은 초기화된다.

## 현재 운영 배포 (2026-11-16)

- 웹: `https://block-stack-kr.pages.dev`, 웹 릴리스 `web-v1.3.0`.
- 게임 연결: `wss://golfjoinn.duckdns.org/block-stack/ws`.
- 상태 확인: `https://golfjoinn.duckdns.org/block-stack/healthz`.
- Oracle에서 화면까지 확인: `https://golfjoinn.duckdns.org/block-stack/`.
- 게임 서비스: `block-stack`, 내부 주소 `127.0.0.1:3010`, 릴리스 `block-v1.4.0`.
- 기존 골프 조인 호스트에 `/block-stack/*` 경로만 분리했다. 기존 `/ws`와 3000번 서비스는 골프 조인용이다. 경로 설정 예제는 `deploy/Caddyfile.path.fragment`에 있다. Oracle용 정적 파일은 Trunk `--public-url /block-stack/`로 별도 빌드했다.
- 서버 변경 전 Caddy 사본: `/etc/caddy/Caddyfile.before-block-stack-20261114`. 골프 조인 설치 도구로 Caddyfile 전체를 다시 생성한다면 게임 경로 설정도 보존해야 한다.
- 서버는 개발 기계에서 만든 Linux x86_64 정적 바이너리를 `install-runtime`으로 배포했다. 운영 서버에 컴파일러를 설치하거나 빌드하지 않았다.
- 배포 후 공개 Oracle 주소와 Pages 주소 양쪽에서 두 Chrome 세션으로 방·채팅·봇·게임 시작 검증을 통과했다.

### 2분 대결·공격 통계·모바일 조작 배포

`block-v1.1.0` / `web-v1.1.0`에는 카운트다운 제외 2분 대결, 동점 순위, 개인별 공격량/APM/완료 경기 평균과 터치 조작이 포함된다. 서버 메시지는 기존 postcard variant 뒤에 추가하여 이전 클라이언트 메시지 번호와 점수 payload를 유지했다. 서버 재시작으로 기존 메모리 방은 초기화된다. 이전 서버 릴리스 `block-v1.0.0`은 rollback용으로 보관한다. 엔진·서버 테스트 30개와 Pages의 기존 방/채팅 브라우저 검사, Pages·Oracle 양쪽의 실제 모바일 터치/시간/통계 브라우저 검사를 통과했다.

```sh
python3 tools/smoke_battle.py --url https://block-stack-kr.pages.dev
```

### 결과·재대결·팀전·기록·방/모바일 설정 배포

`block-v1.2.0` / `web-v1.2.0`에는 최종 결과표와 준비 후 자동 재대결, 2:2·3:3 팀전, 브라우저 개인 기록, 공개/비공개·시간·정원·봇 난이도, 모바일 버튼 설정이 포함된다. 새 `RoomState`, `MatchSummary`, `RoomRules`는 기존 메시지 뒤에 추가했다. 기존 `Rooms`와 점수 payload는 유지한다. 방 목록은 공개 방의 실제 정원·모드·시간을 표시한다.

배포 후 Pages에서 기존 방/채팅 검사와 새 결과/재대결/기록/규칙/팀전 검사, 실제 모바일 설정 검사를 통과했다. Oracle 경로에서도 새 기능 검사를 통과했다. 핵심 Rust 테스트는 47개이며 배포 도구의 오프라인 테스트는 8개다.

기록은 서버 계정 데이터가 아닌 현재 브라우저 localStorage에 저장한다. 최근 50경기와 별도 누적 합계를 보관하며 관전·결과 재전송은 중복 저장하지 않는다. 모바일 설정은 기존 settings 저장 형식과 호환된다. 비공개는 목록 숨김이며 링크/코드 입장에는 비밀번호가 없다.

```sh
python3 tools/smoke_evolution.py --url https://block-stack-kr.pages.dev
python3 tools/smoke_touch_settings.py --url https://block-stack-kr.pages.dev
```

### T-Spin·콤보 피드백 배포

`block-v1.3.0` / `web-v1.3.0`에는 SRS 회전, T-Spin Full/Mini, B2B, 지속 콤보 카운터와 실제 공격 전송 피드백을 포함한다. 생성·상쇄·전송을 구분하며 실제 공격 대상의 보드와 받을 공격 게이지를 강조한다. 상세한 자체 규칙은 [ATTACK_RULES.md](ATTACK_RULES.md)에 있다.

기존 `Game` 직렬화와 메시지 번호는 유지한다. 새 클라이언트는 `EnableCombat`을 보내 `YourStateV2` sidecar를 복원한 후 미확인 입력을 재적용한다. 기술 표시는 예측값이 아닌 서버의 고정별 `CombatEvent`를 사용한다. 이전 클라이언트는 새 규칙을 정확히 예측하지 못하므로 새로고침하여 새 웹 버전을 받는다.

핵심 Rust 테스트 65개와 배포 도구 테스트 8개를 통과했다. Pages·Oracle 양쪽에서 실제 V2 협상과 전투 UI 검사를 확인했으며 Pages의 기존 방·채팅 검사도 통과했다.

`smoke_combat.py`는 실제 V2 협상을 확인한 뒤 격리된 브라우저에 전투 패킷 fixture를 전달하여 UI를 검증한다. T-Spin 판정·공격 배분 자체는 엔진/서버 테스트가 검증한다. 프로덕션 앱에 테스트용 게임 조작 API는 추가하지 않았다.

```sh
python3 tools/smoke_combat.py --url https://block-stack-kr.pages.dev
```

### 이후 Pages API 배포

`.env.example`을 참고하여 로컬 `.env`에 `CLOUDFLARE_ACCOUNT_ID`, `CLOUDFLARE_API_TOKEN`, `BS_WS_URL`을 준비한다. `.env`와 `.env.*`는 Git에서 제외하며 `.env.example`만 예외다. 스크립트는 허용된 설정만 읽고 셸 코드로 실행하지 않는다. 명시적으로 전달한 환경변수가 `.env`보다 우선한다.

```sh
python3 scripts/deploy_web.py --tag web-v1.3.1
```

이 명령은 빌드·업로드·운영 릴리스 확인까지 수행한다. `release.json`의 고유 배포 값과 HTML 해시를 비교하며, Cloudflare의 Browser Integrity Check에서 기본 Python User-Agent 요청이 차단되는 경우를 피하기 위해 식별 가능한 `BlockStackDeploy/1.0` User-Agent를 사용한다. 운영 별칭의 전파 지연에는 3초 간격으로 최대 10회 동일 릴리스·HTML·CSP를 확인하고 계속 다르면 실패한다. 토큰 값은 출력하지 않는다.

## 1. 최초 준비

- 기존 Oracle 서버를 사용한다면 골프 조인의 3000번 포트와 다른 3010번을 사용한다.
- 전용 호스트를 쓴다면 게임용 DNS 이름을 서버의 공인 IP에 연결한다. 기존 호스트 경로를 재사용하는 현재 구성에는 새 DNS가 필요 없다. 새 이름을 만드는 경우 기존 DuckDNS 자동 갱신 작업이 새 이름도 갱신한다고 가정하지 않는다.
- Cloudflare Pages 프로젝트를 정하고 계정 ID와 Pages 편집 권한 토큰을 환경변수로 준비한다. 토큰은 저장소나 명령줄 인자에 넣지 않는다.
- 개발 기계에는 Rust, WASM 타깃, Trunk, Python 3, Node.js/npx가 필요하다. `TRUNK`와 `WRANGLER` 환경변수로 실행 파일 위치를 지정할 수 있다.

서버 계정·서비스·Rust 설치와 Caddy 사이트 추가는 [deploy/README.md](../deploy/README.md)를 따른다. 기존 Caddyfile 전체를 게임 설정으로 덮어쓰지 않는다. 사이트 조각을 import하고 검증 후 reload한다.

## 2. 백엔드 설치 → 활성화 → 상태 확인

```sh
# 소스와 배포 도구를 포함하는 태그가 준비된 경우
RELEASE_SSH_OPTS='-i /path/to/ssh-key' \
  deploy/release.sh block-v1.0.0 ubuntu@SERVER --install-only

# 서버에서
sudo block-stack-release activate block-v1.0.0
curl -fsS http://127.0.0.1:3010/healthz
curl -fsS https://GAME_HOST/healthz
```

커밋 전 파일을 시험하려면 `--worktree`를 명시한다. 배포 없이 패키지만 만들 수도 있다.

```sh
deploy/release.sh preview-1 --worktree --package-only /tmp/block-stack-preview.tar.gz
```

최초 서버 설치 이후에는 새 소스를 업로드하고 빌드하며, 성공하기 전까지 현재 서비스는 계속 실행된다. 상태 확인이 실패하면 이전 릴리스로 자동 복구한다. 서비스의 실제 기동과 외부 TLS는 서버에서 별도로 확인한다.

## 3. Pages 빌드 → 배포 → 배포판 확인

```sh
# 환경변수로 준비: CLOUDFLARE_ACCOUNT_ID, CLOUDFLARE_API_TOKEN
BS_WS_URL=wss://GAME_HOST/ws \
  python3 scripts/deploy_web.py --tag web-v1.0.0 --project block-stack-kr --create-project
```

`--create-project`는 처음 프로젝트를 만들 때만 사용한다. 이후에는 같은 명령에서 이 옵션을 뺀다. 운영 갈래는 기본 `main`이며 `--branch`로 지정할 수 있다. 현재 Git 작업 갈래에 따라 미리보기 주소로 배포되지 않게 갈래를 명시해서 전달한다.

웹 빌드는 공개 `wss://` 연결 주소를 검사하고 Trunk 릴리스 빌드를 실행한다. `_headers`에 WASM 실행과 해당 서버 연결에 필요한 CSP를 생성한다. Trunk의 인라인 초기화 스크립트에는 해시를 사용한다. Cloudflare 토큰이 없으면 원격 작업 전에 실패한다.

Cloudflare 없이 빌드까지만 확인하는 명령:

```sh
BS_WS_URL=wss://GAME_HOST/ws \
  python3 scripts/deploy_web.py --tag web-v1.0.0 --build-only
```

`BS_WS_URL`은 빌드 시 WASM에 포함되는 공개 주소다. 비밀값을 넣지 않는다. 주소를 바꾸면 다시 빌드한다. 이 값을 지정하지 않은 일반 Trunk 빌드는 기존처럼 현재 웹 주소의 `/ws`에 연결한다. 이는 로컬 개발 및 Oracle에서 정적 파일까지 제공하는 경우에 쓴다.

## 4. 배포 후 확인과 되돌리기

```sh
curl -fsS https://GAME_HOST/healthz
python3 tools/smoke.py --url https://block-stack-kr.pages.dev
```

브라우저 검사는 별도 세션 두 개로 방 생성·참가·채팅·한글 입력·봇·게임 시작·테마 전환·화면 크기 변경을 확인한다. 로컬 자체 서명 인증서 시험용 `--ignore-https-errors`는 운영 검사에 사용하지 않는다.

```sh
# 서버 이전 판으로 되돌리기
sudo block-stack-release rollback
```

Pages는 Cloudflare 대시보드의 해당 프로젝트 배포 목록에서 이전 운영 배포로 Rollback한다. 서버와 Pages는 별도 배포이므로 두 판의 프로토콜이 맞는지 확인한다. 과거 성능 보고서는 이 배포의 부하 시험 결과가 아니다.

## 공식 문서

- [Cloudflare Pages 응답 헤더](https://developers.cloudflare.com/pages/configuration/headers/)
- [Caddy WebSocket reverse proxy](https://caddyserver.com/docs/caddyfile/directives/reverse_proxy)
- [Caddy 설정 import](https://caddyserver.com/docs/caddyfile/directives/import)


### 연습·리플레이·프로필·미션·협동전 배포 (2026-11-17)

현재 릴리스는 `block-v1.4.0` / `web-v1.4.0`이다. Pages 배포 URL은 `https://2295ae1d.block-stack-kr.pages.dev`, 운영 주소는 `https://block-stack-kr.pages.dev`이며 배포 식별자는 `c0a70e71e7784056a2b67d2def77b0cc`이다. Linux 정적 서버 SHA-256은 `1c158b2c243c0e6a957cf7616364588a12030f8c5b318956ca6b594dea06f539`이다. 이전 `block-v1.3.0`은 rollback용으로 보관한다.

대기실 상단 참가자 요약(내 이름·P번호·방장·준비 상태), 첫 경기부터 다른 참가자 준비 확인, 연습실 5모드, 마지막 15초 내 보드 리플레이·기술 타임라인·최근 APM 추이, 서버 프로필·일일 미션·해금 테마, 선택형 모바일 제스처, 반응형 사운드·배경, 협동 생존전이 포함된다. 자세한 저장·집계 범위는 [GROWTH_FEATURES.md](GROWTH_FEATURES.md)에 있다.

`AttachProfile`과 `SetCoop`은 ClientMsg 12/13, `CoopState`와 `CoopRooms`는 ServerMsg 22/23에 추가했다. 기존 payload·번호는 유지한다. 방 목록은 협동 모드도 별도 표기한다.

프로필 API는 `https://golfjoinn.duckdns.org/block-stack/api/profile`이며 Pages의 CSP connect-src와 API의 CORS 허용 원본을 함께 구성했다. 인증 없는 요청은 401이다. 서버 데이터는 systemd `StateDirectory=block-stack`, `BS_DATA_DIR=/var/lib/block-stack`으로 릴리스와 분리했다. 디렉터리 권한은 0700, 프로필 파일은 0600이다. 서비스 파일의 이전 사본은 `/etc/systemd/system/block-stack.service.before-growth`다. 릴리스 rollback은 서버 데이터 폴더를 삭제하지 않는다. 같은 폴더 아래 `replays/`(전체 경기 리플레이, 최근 500개)와 `leaderboard/`(연습 순위표)도 생기므로 함께 백업한다. Pages CSP의 connect-src는 프로필·리플레이·순위표를 모두 쓰도록 `/api/` 경로 전체를 허용한다.

핵심 Rust 테스트 82개, 웹 배포 도구 테스트 8개와 릴리스 안전 검증을 통과했다. 신규 연습·제스처·대기실·사운드·프로필·리플레이/협동 검사와 기존 대전·콤보·재대결 회귀 검사를 로컬에서 수행했다. 운영 Oracle fallback에서는 연습·제스처·실제 대전 후 프로필 갱신·복구·저장 차단 시 코드 유지 검사를 통과했다.

운영 Pages에서도 기본 방/채팅/IME/테마/반응형 검사, 대기실 내 P번호·방장 승계·준비 취소/시작 제한, 결과/재대결/팀전, 모바일 설정, 실제 리플레이/협동전, 프로필 CORS/CSP·인증·복구·미션/보상·브라우저 저장 차단 회귀 검사를 통과했다. 서버의 실행 바이너리 해시 일치, 프로필 파일 0600과 상태 디렉터리 0700, block-stack/golfjoin/caddy의 active 상태를 확인했다.
