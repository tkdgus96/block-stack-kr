#!/usr/bin/env bash
# Ubuntu bootstrap; does not touch Caddy or golfjoin and does not start the app.
set -euo pipefail
[[ $# -le 1 && "${1:-}" =~ ^(--runtime-only)?$ && $(id -u) -eq 0 ]] || { echo "Usage: sudo $0 [--runtime-only]" >&2; exit 2; }
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
if [[ "${1:-}" != --runtime-only ]]; then
  apt-get update
  apt-get install -y build-essential pkg-config libssl-dev ca-certificates curl python3 util-linux
else
  for command in python3 curl flock; do command -v "$command" >/dev/null || { echo "Missing: $command" >&2; exit 1; }; done
fi
id block-stack >/dev/null 2>&1 || useradd --system --user-group --home-dir /nonexistent --shell /usr/sbin/nologin block-stack
if [[ "${1:-}" != --runtime-only ]]; then
  id block-stack-build >/dev/null 2>&1 || useradd --system --user-group --create-home --home-dir /var/lib/block-stack-build --shell /bin/bash block-stack-build
fi
install -d -m 755 /opt/block-stack/releases
install -d -m 750 /etc/block-stack
if [[ ! -e /etc/block-stack/env ]]; then
  printf '%s\n' 'BS_BIND=127.0.0.1:3010' 'BS_DIST=/opt/block-stack/current/dist' > /etc/block-stack/env
  chmod 600 /etc/block-stack/env
fi
install -m 755 "$HERE/block-stack-release.sh" /usr/local/sbin/block-stack-release
install -m 644 "$HERE/block-stack.service" /etc/systemd/system/block-stack.service
systemctl daemon-reload
systemctl enable block-stack
if [[ "${1:-}" != --runtime-only && ! -x /var/lib/block-stack-build/.cargo/bin/cargo ]]; then
  echo 'Install Rust for the build account before the first release:'
  echo "See deploy/README.md: install rustup as block-stack-build (not root)."
fi
echo 'Bootstrap complete. Caddy config and existing services were not changed.'
