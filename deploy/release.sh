#!/usr/bin/env bash
# Local: package a tag (or current worktree), then upload/install/activate.
set -euo pipefail
usage() { echo "Usage: $0 TAG [user@host] [--worktree] [--install-only] [--package-only OUTPUT.tar.gz]" >&2; exit 2; }
[[ $# -ge 1 ]] || usage
TAG=$1; shift
[[ "$TAG" =~ ^[A-Za-z0-9][A-Za-z0-9._-]{0,79}$ ]] || usage
TARGET=; WORKTREE=0; INSTALL_ONLY=0; OUTPUT=
while [[ $# -gt 0 ]]; do
  case "$1" in
    --worktree) WORKTREE=1; shift;;
    --install-only) INSTALL_ONLY=1; shift;;
    --package-only) [[ $# -ge 2 ]] || usage; OUTPUT=$2; shift 2;;
    -*) usage;;
    *) [[ -z "$TARGET" ]] || usage; TARGET=$1; shift;;
  esac
done
[[ -n "$OUTPUT" || "$TARGET" =~ ^[A-Za-z0-9._-]+@[A-Za-z0-9][A-Za-z0-9.-]*$ ]] || usage
ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
STAGE=$(mktemp -d)
trap 'rm -rf "$STAGE"' EXIT
mkdir -p "$STAGE/source"
FILES=(Cargo.toml Cargo.lock engine server client deploy)
export COPYFILE_DISABLE=1
if [[ "$WORKTREE" -eq 1 ]]; then
  (cd "$ROOT" && tar --exclude=client/dist --exclude=.DS_Store -cf - "${FILES[@]}") | tar -xf - -C "$STAGE/source"
else
  git -C "$ROOT" rev-parse --verify "refs/tags/$TAG" >/dev/null
  git -C "$ROOT" archive "refs/tags/$TAG" "${FILES[@]}" | tar -xf - -C "$STAGE/source"
fi
# Build the exact packaged source, including uncommitted UI when --worktree is used.
TRUNK=${TRUNK:-trunk}
(cd "$STAGE/source/client" && CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}" "$TRUNK" build --release --dist "$STAGE/dist")
[[ -s "$STAGE/dist/index.html" ]] || { echo "Frontend build missing" >&2; exit 1; }
# Client source remains for Cargo workspace parsing; server builds only bs-server.
rm -rf "$STAGE/source/target"
printf '%s\n' "$TAG" > "$STAGE/VERSION"
tar -czf "$STAGE/release.tar.gz" -C "$STAGE" source dist VERSION
if [[ -n "$OUTPUT" ]]; then
  cp "$STAGE/release.tar.gz" "$OUTPUT"
  echo "Packaged: $OUTPUT"
  exit 0
fi
SSH_OPTS=()
# Whitespace-separated SSH options; use ~/.ssh/config for paths containing spaces.
if [[ -n "${RELEASE_SSH_OPTS:-}" ]]; then read -r -a SSH_OPTS <<< "$RELEASE_SSH_OPTS"; fi
REMOTE="/tmp/block-stack-$TAG-$(date +%s)-$$.tar.gz"
scp ${SSH_OPTS[@]+"${SSH_OPTS[@]}"} "$STAGE/release.tar.gz" "$TARGET:$REMOTE"
ssh ${SSH_OPTS[@]+"${SSH_OPTS[@]}"} "$TARGET" "sudo /usr/local/sbin/block-stack-release install '$TAG' '$REMOTE' && rm -f '$REMOTE'"
if [[ "$INSTALL_ONLY" -eq 0 ]]; then
  ssh ${SSH_OPTS[@]+"${SSH_OPTS[@]}"} "$TARGET" "sudo /usr/local/sbin/block-stack-release activate '$TAG'"
fi
