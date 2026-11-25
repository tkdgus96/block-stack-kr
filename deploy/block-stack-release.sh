#!/usr/bin/env bash
# Server: install TAG ARCHIVE | install-runtime TAG ARCHIVE | activate TAG | rollback | list
set -euo pipefail
BASE=/opt/block-stack
RELEASES=$BASE/releases
CURRENT=$BASE/current
PREVIOUS=$BASE/previous
CARGO=/var/lib/block-stack-build/.cargo/bin/cargo
[[ $(id -u) -eq 0 ]] || { echo 'Run with sudo' >&2; exit 1; }
usage() { echo "Usage: $0 install TAG ARCHIVE | install-runtime TAG ARCHIVE | activate TAG | rollback | list" >&2; exit 2; }
valid_tag() { [[ "$1" =~ ^[A-Za-z0-9][A-Za-z0-9._-]{0,79}$ ]] || { echo 'Invalid release tag' >&2; exit 2; }; }
mkdir -p "$RELEASES"
exec 9>"$BASE/release.lock"
flock -n 9 || { echo 'Another release operation is running' >&2; exit 1; }
STAGE=
trap 'if [[ -n "$STAGE" ]]; then rm -rf -- "$STAGE"; fi' EXIT
switch_to() {
  ln -s "$1" "$BASE/current.next"
  mv -Tf "$BASE/current.next" "$CURRENT"
}
restart_and_wait() {
  systemctl reset-failed block-stack 2>/dev/null || true
  systemctl restart block-stack || return 1
  local i
  for i in $(seq 1 30); do
    if systemctl is-active --quiet block-stack && curl --max-time 2 -fsS http://127.0.0.1:3010/healthz >/dev/null; then
      return 0
    fi
    sleep 1
  done
  return 1
}
activate() {
  local dir=$1 old
  [[ -x "$dir/bin/bs-server" && -s "$dir/dist/index.html" ]] || { echo 'Release is incomplete' >&2; return 1; }
  old=$(readlink "$CURRENT" || true)
  [[ "$old" != "$dir" ]] || { echo 'Already active'; return 0; }
  [[ ! -e "$CURRENT" || -L "$CURRENT" ]] || { echo 'current must be a symlink' >&2; return 1; }
  rm -f "$BASE/current.next"
  switch_to "$dir"
  if restart_and_wait; then
    if [[ -n "$old" ]]; then
      printf '%s\n' "$old" > "$PREVIOUS.tmp"
      mv -f "$PREVIOUS.tmp" "$PREVIOUS"
    else
      rm -f "$PREVIOUS"
    fi
    echo "Active: $dir"
    return 0
  fi
  echo 'Health check failed; restoring previous release' >&2
  if [[ -n "$old" && -x "$old/bin/bs-server" ]]; then
    switch_to "$old"
    if ! restart_and_wait; then echo 'CRITICAL: restored release also failed health check; inspect journalctl -u block-stack' >&2; fi
  else
    systemctl stop block-stack || true
    rm -f "$CURRENT"
    echo 'No previous release exists; service stopped' >&2
  fi
  return 1
}
case "${1:-}" in
  install|install-runtime)
    [[ $# -eq 3 ]] || usage
    tag=$2; archive=$3; valid_tag "$tag"
    [[ -f "$archive" && ! -L "$archive" ]] || { echo 'Archive must be a regular file' >&2; exit 1; }
    [[ ! -e "$RELEASES/$tag" ]] || { echo 'Release already exists' >&2; exit 1; }
    if [[ "$1" == install ]]; then
      [[ -x "$CARGO" ]] || { echo "Install Rust for block-stack-build first ($CARGO missing)" >&2; exit 1; }
    fi
    STAGE=$(mktemp -d "$RELEASES/.install-XXXXXXXX")
    # Validate the entire archive before extracting. Disallow links, devices,
    # duplicate names and traversal; extract manually to avoid tar link tricks.
    python3 - "$archive" "$STAGE" "$tag" "$1" <<'PY'
import pathlib, sys, tarfile
archive, destination, tag, mode = sys.argv[1:]
root = pathlib.Path(destination)
with tarfile.open(archive, 'r:gz') as tf:
    members = tf.getmembers()
    seen = set()
    size = 0
    for m in members:
        p = pathlib.PurePosixPath(m.name)
        if (p.is_absolute() or '..' in p.parts or not p.parts
            or p.parts[0] not in ('source', 'dist', 'bin', 'VERSION')
            or not (m.isdir() or m.isfile()) or str(p) in seen):
            raise SystemExit('Unsafe archive member: ' + m.name)
        seen.add(str(p))
        size += m.size
        if size > 512 * 1024 * 1024:
            raise SystemExit('Archive exceeds 512 MiB limit')
    for m in members:
        path = root.joinpath(*pathlib.PurePosixPath(m.name).parts)
        if m.isdir():
            path.mkdir(parents=True, exist_ok=True)
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            with tf.extractfile(m) as src, path.open('xb') as dst:
                import shutil
                shutil.copyfileobj(src, dst)
            path.chmod(0o644)
if (root / 'VERSION').read_text().strip() != tag:
    raise SystemExit('Archive version does not match requested tag')
required_files = ('bin/bs-server', 'dist/index.html') if mode == 'install-runtime' else ('source/Cargo.toml', 'source/Cargo.lock', 'source/server/Cargo.toml', 'dist/index.html')
for required in required_files:
    if not (root / required).is_file():
        raise SystemExit('Missing: ' + required)
if mode == 'install-runtime':
    header = (root / 'bin/bs-server').read_bytes()[:20]
    if header[:6] != b'\x7fELF\x02\x01' or header[18:20] != b'\x3e\x00':
        raise SystemExit('Runtime must be a Linux x86_64 ELF binary')
PY
    if [[ "$1" == install ]]; then
      chown -R block-stack-build:block-stack-build "$STAGE"
      runuser -u block-stack-build -- env HOME=/var/lib/block-stack-build PATH=/var/lib/block-stack-build/.cargo/bin:/usr/local/bin:/usr/bin:/bin CARGO_BUILD_JOBS=1 "$CARGO" build --locked --release --manifest-path "$STAGE/source/Cargo.toml" -p bs-server
      mkdir "$STAGE/bin"
      install -m 755 "$STAGE/source/target/release/bs-server" "$STAGE/bin/bs-server"
      # Keep only runtime files and release metadata; Rust caches remain in build HOME.
      rm -rf "$STAGE/source"
    else
      chmod 755 "$STAGE/bin/bs-server"
    fi
    chown -R root:root "$STAGE"
    chmod -R go-w "$STAGE"
    find "$STAGE" -type d -exec chmod 755 {} +
    mv "$STAGE" "$RELEASES/$tag"
    STAGE=
    echo "Installed: $tag (current unchanged)"
    ;;
  activate)
    [[ $# -eq 2 ]] || usage
    valid_tag "$2"
    activate "$RELEASES/$2"
    ;;
  rollback)
    [[ $# -eq 1 && -f "$PREVIOUS" ]] || { echo 'No previous release' >&2; exit 1; }
    dir=$(cat "$PREVIOUS")
    tag=${dir##*/}; valid_tag "$tag"
    [[ "$dir" == "$RELEASES/$tag" ]] || { echo 'Invalid previous release path' >&2; exit 1; }
    activate "$dir"
    ;;
  list)
    [[ $# -eq 1 ]] || usage
    find "$RELEASES" -mindepth 1 -maxdepth 1 -type d ! -name '.*' -printf '%f\n' | sort
    echo "Current: $(readlink "$CURRENT" || true)"
    echo "Previous: $(cat "$PREVIOUS" 2>/dev/null || true)"
    ;;
  *) usage;;
esac
