#!/usr/bin/env python3
"""Build the WASM client and deploy to Cloudflare Pages, like golf-join-app.

BS_WS_URL=wss://game.example.org/ws TRUNK=trunk python3 scripts/deploy_web.py --tag web-v1.0.0 --build-only
Set CLOUDFLARE_ACCOUNT_ID and CLOUDFLARE_API_TOKEN for deployment.
"""
import argparse
import base64
import hashlib
import ipaddress
import json
from html.parser import HTMLParser
import os
from pathlib import Path
import re
import shlex
import subprocess
import sys
import time
from urllib.error import URLError
from urllib.parse import urlsplit
from urllib.request import Request, urlopen
import uuid

ROOT = Path(__file__).resolve().parents[1]


def load_local_env(path):
    """Read the ignored local credentials file without executing shell code."""
    if not path.is_file():
        return
    allowed = {"CLOUDFLARE_ACCOUNT_ID", "CLOUDFLARE_API_TOKEN", "BS_WS_URL", "TRUNK", "WRANGLER"}
    for number, line in enumerate(path.read_text().splitlines(), 1):
        line = line.strip()
        if line.startswith("export "):
            line = line[7:].strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        key, value = line.split("=", 1)
        key = key.strip()
        if key not in allowed:
            continue
        parts = shlex.split(value, comments=True)
        if len(parts) > 1:
            raise ValueError(f"Invalid .env value on line {number}; quote values containing spaces")
        os.environ.setdefault(key, parts[0] if parts else "")


class InlineScripts(HTMLParser):
    def __init__(self):
        super().__init__(convert_charrefs=False)
        self.collect = False
        self.scripts = []
        self.current = ""

    def handle_starttag(self, tag, attrs):
        if tag == "script":
            self.collect = "src" not in dict(attrs)
            self.current = ""

    def handle_data(self, data):
        if self.collect:
            self.current += data

    def handle_endtag(self, tag):
        if tag == "script" and self.collect:
            self.scripts.append(self.current)
            self.collect = False


def websocket_url(value):
    url = urlsplit(value)
    if (url.scheme != "wss" or not url.hostname or url.username or url.password
            or url.query or url.fragment or not url.path.endswith("/ws")
            or not re.fullmatch(r"wss://[A-Za-z0-9.:[\]-]+/[A-Za-z0-9/_-]*ws", value)):
        raise ValueError("BS_WS_URL must be a public wss://host/.../ws URL without credentials or query parameters")
    # Accessing .port also rejects nonnumeric or out-of-range ports.
    if url.netloc.endswith(":") or url.port == 0:
        raise ValueError("BS_WS_URL port must be between 1 and 65535")
    host = url.hostname.lower()
    if host == "localhost" or host.endswith(".localhost") or "." not in host and ":" not in host:
        raise ValueError("BS_WS_URL must point to a public host")
    try:
        address = ipaddress.ip_address(host)
    except ValueError:
        if not re.fullmatch(r"(?:[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\.)+[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?", host):
            raise ValueError("BS_WS_URL has an invalid hostname")
    else:
        if not address.is_global:
            raise ValueError("BS_WS_URL must point to a public host")
    return value


def write_headers(dist, endpoint):
    parser = InlineScripts()
    parser.feed((dist / "index.html").read_text())
    hashes = ["'sha256-" + base64.b64encode(hashlib.sha256(s.encode()).digest()).decode() + "'" for s in parser.scripts]
    csp = "; ".join([
        "default-src 'self'", "script-src 'self' 'wasm-unsafe-eval' " + " ".join(hashes),
        "style-src 'self' 'unsafe-inline'", f"connect-src 'self' {endpoint} {endpoint.replace('wss://', 'https://', 1).removesuffix('/ws')}/api/profile",
        "img-src 'self' data:", "object-src 'none'", "base-uri 'self'",
        "frame-ancestors 'none'", "form-action 'self'",
    ])
    (dist / "_headers").write_text(
        "/*\n  X-Content-Type-Options: nosniff\n  X-Frame-Options: DENY\n"
        "  Referrer-Policy: no-referrer\n  Cache-Control: no-cache\n"
        f"  Content-Security-Policy: {csp}\n"
    )


def write_release(dist, tag):
    release = {"tag": tag, "deployment": uuid.uuid4().hex,
               "index_sha256": hashlib.sha256((dist / "index.html").read_bytes()).hexdigest()}
    (dist / "release.json").write_text(json.dumps(release) + "\n")
    return release


def verify_release(url, release):
    # A unique manifest prevents an earlier successful deployment from passing.
    # Verify the actual HTML too, so a fresh manifest alone is insufficient.
    query = "?deployment=" + release["deployment"]
    def fetch(path):
        request = Request(url + path + query, headers={
            "Cache-Control": "no-cache", "User-Agent": "BlockStackDeploy/1.0",
        })
        with urlopen(request, timeout=30) as response:
            if response.status != 200 or not response.headers.get("Content-Security-Policy"):
                raise ValueError("Pages response or CSP header check failed; inspect the deployment before sharing it")
            return response.read()
    if json.loads(fetch("release.json")) != release:
        raise ValueError("Pages is serving a different release; inspect the deployment before sharing it")
    if hashlib.sha256(fetch("")).hexdigest() != release["index_sha256"]:
        raise ValueError("Pages HTML does not match the built release")


def wait_for_release(url, release):
    # Pages can briefly serve the previous production alias after upload succeeds.
    for attempt in range(10):
        try:
            verify_release(url, release)
            return
        except (ValueError, URLError, TimeoutError):
            if attempt == 9:
                raise
            print("Waiting for Pages propagation...", flush=True)
            time.sleep(3)


def run(command, cwd=None):
    subprocess.run(command, cwd=cwd or ROOT, check=True,
                   env={**os.environ, "CI": "true", "WRANGLER_SEND_METRICS": "false"})


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--project", default="block-stack-kr")
    parser.add_argument("--branch", default="main")
    parser.add_argument("--dist", type=Path, default=ROOT / "client/dist")
    parser.add_argument("--build-only", action="store_true")
    parser.add_argument("--create-project", action="store_true", help="Create the Pages project before its first deployment")
    args = parser.parse_args()
    load_local_env(ROOT / ".env")
    endpoint = websocket_url(os.environ.get("BS_WS_URL", ""))
    if not re.fullmatch(r"[a-z0-9][a-z0-9-]{0,57}[a-z0-9]", args.project):
        raise ValueError("Invalid Pages project name")
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]{0,79}", args.tag):
        raise ValueError("Invalid release tag")
    if not args.build_only:
        missing = [k for k in ("CLOUDFLARE_ACCOUNT_ID", "CLOUDFLARE_API_TOKEN") if not os.environ.get(k)]
        if missing:
            raise ValueError("Missing deployment environment variables: " + ", ".join(missing))
    dist = args.dist.resolve()
    run([os.environ.get("TRUNK", "trunk"), "build", "--release", "--dist", str(dist)], ROOT / "client")
    write_headers(dist, endpoint)
    if not list(dist.glob("*.wasm")) or not list(dist.glob("*.js")):
        raise ValueError("Build is missing WASM or JavaScript artifacts")
    release = write_release(dist, args.tag)
    print(f"Built {args.tag}: {dist}; WebSocket: {endpoint}", flush=True)
    if args.build_only:
        return
    wrangler = shlex.split(os.environ.get("WRANGLER", "npx --yes wrangler@4.151.0"))
    if args.create_project:
        run([*wrangler, "pages", "project", "create", args.project, "--production-branch", args.branch])
    run([*wrangler, "pages", "deploy", str(dist), "--project-name", args.project,
         "--branch", args.branch, "--commit-message", args.tag, "--commit-dirty=true"])
    url = f"https://{args.project}.pages.dev/"
    wait_for_release(url, release)
    print(f"Deployed: {url}\nVerified release: {release['tag']} ({release['deployment']})\n"
          f"Manifest: {url}release.json\nRun: python3 tools/smoke.py --url {url}")


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        print(f"Deploy failed: {error}", file=sys.stderr)
        sys.exit(1)
