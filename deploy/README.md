# Oracle Ubuntu 배포

골프 조인과 같은 태그별 `install → activate → rollback` 방식이다. 기존 golfjoin(3000번), Caddy 전역 설정은 스크립트가 수정하지 않는다. Block Stack은 별도 계정·systemd 서비스로 `127.0.0.1:3010`을 사용한다. 재시작하면 진행 중인 방·채팅은 초기화된다.

## 처음 한 번: 서버

Ubuntu x86_64의 sudo 권한, 외부 HTTPS 연결이 필요하다. 이 폴더를 서버에 복사하고 실행한다.

```sh
sudo bash deploy/install.sh
```

설치 스크립트는 빌드 의존성·계정·서비스를 준비하고 서비스를 enable만 한다. `/etc/block-stack/env`는 기존 값이 있으면 보존한다. Rust는 별도 빌드 계정에 설치한다.

```sh
sudo -u block-stack-build -H bash
# 아래는 block-stack-build 셸 안에서 실행
installer=$(mktemp)
curl --proto '=https' --tlsv1.2 -fsS https://sh.rustup.rs -o "$installer"
sh "$installer" -y --profile minimal
rm -f "$installer"
exit
```

서버는 배포 소스에서 `cargo build --locked --release -p bs-server`로 직접 빌드한다. Docker·교차 컴파일은 필요 없다. 작은 Oracle VM에 맞춰 빌드 동시 작업 수는 1로 고정한다. 최초 빌드는 시간이 걸리며 메모리가 부족하면 swap이 필요하다. 현재 서비스는 새 빌드가 끝날 때까지 유지된다.

## 개발 기계: 묶고 보내기

Rust, `wasm32-unknown-unknown` 타깃, Trunk가 필요하다. 소스와 프런트엔드를 같은 태그 또는 같은 작업 트리에서 묶는다. `TRUNK=/path/to/trunk`로 도구 위치를 지정할 수 있다. `RELEASE_SSH_OPTS='-i /path/to/key'` 또는 `~/.ssh/config`로 SSH 키를 지정한다.

```sh
# 태그를 만든 뒤 배포 (배포 도구도 해당 태그에 포함해야 한다)
git tag block-v1.0.0
deploy/release.sh block-v1.0.0 ubuntu@SERVER

# 커밋 전 변경을 포함해 패키지만 만들기: 네트워크 배포 없음
deploy/release.sh preview-1 --worktree --package-only /tmp/block-stack-preview.tar.gz

# 설치만 하고 서비스 전환은 보류
deploy/release.sh block-v1.0.0 ubuntu@SERVER --install-only
```

패키지에는 Rust 소스·잠금 파일과 Trunk `--release`로 빌드한 정적 파일이 포함된다. 빌드 `target` 폴더는 포함하지 않는다. 서버는 압축 파일의 경로·파일 종류·태그를 확인하고 빌드한다. 경로 탈출, 심볼릭 링크, 중복 파일은 거부한다. 패키지는 운영자가 만든 신뢰할 수 있는 소스여야 한다.

## 서버: 전환과 되돌리기

```sh
sudo block-stack-release list
sudo block-stack-release activate block-v1.0.0
sudo block-stack-release rollback
curl -fsS http://127.0.0.1:3010/healthz
sudo journalctl -u block-stack -n 50
```

`/opt/block-stack/releases/TAG`에 설치하고 `current`를 원자적으로 바꾼다. 재시작 후 `/healthz`를 최대 30회 확인한다. 실패하면 자동으로 이전 판으로 복구하고 다시 상태를 확인한다. 첫 배포가 실패하면 서비스를 중지한다. 명령은 실패 코드로 끝나므로 실패한 배포를 성공으로 처리하지 않는다. 수동 `rollback`도 같은 검증을 거치며 성공하면 앞뒤 판을 맞바꾼다. 동시에 두 배포가 실행되지 않도록 잠금을 잡는다.

## Caddy: 기존 사이트와 함께 쓰기

`Caddyfile.fragment`의 `games.example.com`을 전용 호스트 이름으로 바꾸어 `/etc/caddy/sites-enabled/block-stack.caddy`에 둔다. DNS가 서버를 가리키고 443/TCP가 열려 있어야 한다. 기존 Caddyfile에 다음 import를 검토 후 추가한다. 골프 조인 사이트·전역 블록은 그대로 유지한다.

```caddyfile
import /etc/caddy/sites-enabled/block-stack.caddy
```

```sh
sudo caddy validate --config /etc/caddy/Caddyfile
sudo systemctl reload caddy
curl -fsS https://YOUR_GAME_HOST/healthz
```

Caddy는 정적 화면과 `/ws`를 모두 Block Stack으로 프록시한다. 기존 골프 조인 설정처럼 인증서 검증은 443의 TLS-ALPN을 사용하며 HTTP 리다이렉트에 의존하지 않는다. Cloudflare 프록시를 앞에 두면 이 인증서 발급 방식과 충돌할 수 있으므로 DNS only로 발급하거나 기존 서버의 인증서 운영 방식에 맞춰 조정한다. 프런트엔드를 Cloudflare Pages에 따로 배포한다면 저장소의 별도 웹 배포 도구를 사용하고 WebSocket 서버 주소를 맞춘다.

## 로컬 검증

```sh
for script in deploy/*.sh; do bash -n "$script" || exit; done
python3 deploy/test-release.py
```

안전 검증은 임시 폴더와 모의 systemd·상태 확인 명령만 사용한다. 정상 전환, 수동·자동 되돌리기, 첫 배포 실패, 위험 압축 파일과 잘못된 태그 거부를 확인한다. 실제 Ubuntu의 설치·서비스 기동·공개 TLS는 서버 배포 때 별도로 확인해야 한다.

## 작은 운영 VM: 미리 빌드한 바이너리 사용

운영 VM에서 Rust를 빌드하지 않으려면 개발 기계에서 Linux x86_64 정적 바이너리를 만든다. `cargo-zigbuild`, Zig와 Rust의 `x86_64-unknown-linux-musl` 타깃을 설치한 환경에서:

```sh
cargo zigbuild --locked --release -p bs-server --target x86_64-unknown-linux-musl
# DIST는 배포 서버 주소로 빌드한 프런트엔드 폴더
# 같은 호스트의 /block-stack/에서 제공하면 Trunk --public-url /block-stack/ 사용
deploy/package-runtime.sh block-v1.0.0 target/x86_64-unknown-linux-musl/release/bs-server /path/to/dist /tmp/block-stack-runtime.tar.gz
```

서버에는 빌드 의존성·Rust 설치 없이 runtime만 준비할 수 있다. `python3`, `curl`, `flock`은 이미 있어야 한다. 기존 서비스와 Caddy는 변경하지 않는다.

```sh
sudo bash deploy/install.sh --runtime-only
sudo block-stack-release install-runtime block-v1.0.0 /tmp/block-stack-runtime.tar.gz
sudo block-stack-release activate block-v1.0.0
```

`install-runtime`도 동일한 압축 파일 검증과 태그 검증을 거친다. 바이너리가 Linux x86_64 ELF인지 확인하며, 전환 이후 상태 확인·자동 복구는 소스 빌드 방식과 같다. x86_64 musl 정적 바이너리는 별도 공유 라이브러리 없이 실행된다. 바이너리 아키텍처 확인만으로 정적 링크 여부를 보증하지는 않으므로 배포 전 Linux에서 기동 검증을 수행한다.
