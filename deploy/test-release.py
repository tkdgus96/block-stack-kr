#!/usr/bin/env python3
"""Offline release safety checks. Uses temporary paths and mocked host commands."""
import io
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile

original = Path(__file__).with_name('block-stack-release.sh').read_text()
with tempfile.TemporaryDirectory(prefix='bs-release-test-') as tmp:
    root = Path(tmp)
    base = root / 'app'
    fake = root / 'commands'
    fake.mkdir()
    cargo = root / 'cargo'
    cargo.write_text('#!/bin/sh\nexit 0\n')
    cargo.chmod(0o755)
    script = root / 'release.sh'
    script.write_text(original.replace('BASE=/opt/block-stack', f'BASE={base}')
        .replace('CARGO=/var/lib/block-stack-build/.cargo/bin/cargo', f'CARGO={cargo}')
        .replace('[[ $(id -u) -eq 0 ]]', '[[ 1 -eq 1 ]]'))
    commands = {
        'chown': '#!/bin/sh\nexit 0\n',
        'flock': '#!/bin/sh\nexit 0\n',
        'systemctl': '#!/bin/sh\necho "$*" >> "$TEST_BASE/systemctl.log"\nexit 0\n',
        'sleep': '#!/bin/sh\nexit 0\n',
        'curl': '#!/bin/sh\ncase "$(readlink "$TEST_BASE/current")" in *bad*) exit 1;; *) exit 0;; esac\n',
        # macOS mv lacks -T. Model GNU mv's rename of the symlink itself.
        'mv': '#!/usr/bin/env python3\nimport os,sys\na=[x for x in sys.argv[1:] if not x.startswith("-")]\nos.replace(a[0],a[1])\n',
    }
    for name, content in commands.items():
        p = fake / name
        p.write_text(content)
        p.chmod(0o755)
    env = dict(os.environ, PATH=f'{fake}:{os.environ["PATH"]}', TEST_BASE=str(base))

    def run(*args, success=True):
        result = subprocess.run(['bash', str(script), *args], env=env, capture_output=True, text=True)
        assert (result.returncode == 0) == success, (args, result.stdout, result.stderr)
        return result

    def release(tag):
        d = base / 'releases' / tag
        (d / 'bin').mkdir(parents=True)
        (d / 'dist').mkdir()
        (d / 'bin/bs-server').write_text('#!/bin/sh\n')
        (d / 'bin/bs-server').chmod(0o755)
        (d / 'dist/index.html').write_text('test')
        return str(d)

    a, b, bad = [release(tag) for tag in ('good-a', 'good-b', 'bad')]
    run('activate', 'good-a')
    assert os.readlink(base / 'current') == a
    run('activate', 'good-b')
    assert os.readlink(base / 'current') == b
    assert (base / 'previous').read_text().strip() == a
    run('activate', 'bad', success=False)
    assert os.readlink(base / 'current') == b
    assert (base / 'previous').read_text().strip() == a
    run('rollback')
    assert os.readlink(base / 'current') == a
    assert (base / 'previous').read_text().strip() == b
    (base / 'current').unlink()
    (base / 'previous').unlink()
    run('activate', 'bad', success=False)
    assert not (base / 'current').exists()
    assert 'stop block-stack' in (base / 'systemctl.log').read_text()
    for i, name in enumerate(('../escape', '/tmp/escape', 'source/link')):
        archive = root / f'unsafe-{i}.tar.gz'
        with tarfile.open(archive, 'w:gz') as tf:
            member = tarfile.TarInfo(name)
            if name.endswith('link'):
                member.type = tarfile.SYMTYPE
                member.linkname = '../../escape'
                tf.addfile(member)
            else:
                member.size = 1
                tf.addfile(member, io.BytesIO(b'x'))
        result = run('install', f'unsafe-{i}', str(archive), success=False)
        assert 'Unsafe archive member' in result.stderr
        assert not (base / 'releases' / f'unsafe-{i}').exists()
    runtime = root / 'runtime.tar.gz'
    header = bytearray(20)
    header[:6] = b'\x7fELF\x02\x01'
    header[18:20] = b'\x3e\x00'
    with tarfile.open(runtime, 'w:gz') as tf:
        for name, content in [('VERSION', b'runtime-1\n'), ('dist/index.html', b'hello'), ('bin/bs-server', header)]:
            member = tarfile.TarInfo(name)
            member.size = len(content)
            tf.addfile(member, io.BytesIO(content))
    run('install-runtime', 'runtime-1', str(runtime))
    installed = base / 'releases/runtime-1'
    assert (installed / 'bin/bs-server').stat().st_mode & 0o111
    assert (installed / 'dist/index.html').read_text() == 'hello'
    assert not (base / 'current').exists()  # install does not activate
    run('install-runtime', 'runtime-1', str(runtime), success=False)
    run('activate', '../escape', success=False)

    print('PASS: activate, rollback, automatic recovery, first-release failure, unsafe archives, invalid tags, prebuilt runtime installation')
