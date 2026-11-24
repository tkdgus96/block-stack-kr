#!/usr/bin/env bash
# Package a locally cross-built static Linux x86_64 binary plus frontend dist.
set -euo pipefail
[[ $# -eq 4 && "$1" =~ ^[A-Za-z0-9][A-Za-z0-9._-]{0,79}$ ]] || { echo "Usage: $0 TAG LINUX_BINARY DIST OUTPUT.tar.gz" >&2; exit 2; }
TAG=$1; BINARY=$2; DIST=$3; OUTPUT=$4
[[ -f "$BINARY" && -s "$DIST/index.html" ]] || { echo 'Binary or frontend missing' >&2; exit 1; }
python3 - "$BINARY" <<'PY'
import sys
with open(sys.argv[1], 'rb') as f:
    header = f.read(20)
if header[:6] != b'\x7fELF\x02\x01' or header[18:20] != b'\x3e\x00':
    raise SystemExit('Expected Linux x86_64 ELF executable')
PY
STAGE=$(mktemp -d)
trap 'rm -rf "$STAGE"' EXIT
mkdir -p "$STAGE/bin" "$STAGE/dist"
install -m 755 "$BINARY" "$STAGE/bin/bs-server"
cp -R "$DIST/." "$STAGE/dist/"
printf '%s\n' "$TAG" > "$STAGE/VERSION"
export COPYFILE_DISABLE=1
tar -czf "$OUTPUT" -C "$STAGE" bin dist VERSION
echo "Packaged runtime: $OUTPUT"
