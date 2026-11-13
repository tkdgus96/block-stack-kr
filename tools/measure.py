"""Block Stack 성능 측정 스크립트.

실제 Microsoft Edge(또는 Chrome)를 Playwright 로 구동해 다음을 측정한다.

  - 초기 로딩: 전송 크기(압축), WASM 크기, DOMContentLoaded / load / 첫 게임판 표시 시각
  - 렌더링 벤치마크(?bench=...): DOM 셀 vs Canvas 2D, 하드웨어 가속 ON vs OFF(SwiftShader),
    CPU 4배 감속(저사양 PC 근사)
  - 실제 서버 대전: 2인(브라우저 2) / 6인(브라우저 6) — 입력 지연, 게임판 갱신 시간,
    메인 스레드 사용률, 브라우저 전체 CPU, 메모리, 서버 CPU
  - 대기실 유휴 상태 CPU (지속 렌더 루프가 없는지 확인)

사용법:
  1) cargo build --release -p bs-server && (cd client && trunk build --release)
  2) 서버 실행: BS_DIST=client/dist ./target/release/bs-server
  3) pip install playwright psutil
  4) python tools/measure.py [--channel msedge|chrome] [--quick]

결과: tools/out/results.json, tools/out/results.md
"""

import argparse
import json
import os
import random
import statistics
import sys

import time
import uuid

import psutil
from playwright.sync_api import sync_playwright

BASE = os.environ.get("BS_URL", "http://localhost:3000")
OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "out")

GPU_ON = []  # headless 기본값: 하드웨어 GPU 사용 (SystemInfo 로 확인)
GPU_OFF = ["--disable-gpu"]  # 소프트웨어 전용 (SwiftShader / CPU 래스터화)


def pct(values, p):
    if not values:
        return None
    v = sorted(values)
    return round(v[min(len(v) - 1, int(round((len(v) - 1) * p)))], 3)


def summary(values):
    if not values:
        return {"n": 0}
    return {
        "n": len(values),
        "p50": pct(values, 0.5),
        "p95": pct(values, 0.95),
        "max": round(max(values), 3),
        "mean": round(statistics.fmean(values), 3),
    }


class Browser:
    """프로필 디렉터리를 고정해 해당 브라우저의 모든 프로세스 CPU/메모리를 추적한다."""

    def __init__(self, pw, channel, gpu, headless=True):
        args = (GPU_ON if gpu else GPU_OFF) + ["--no-first-run", "--disable-extensions"]
        before = {p.pid for p in psutil.process_iter()}
        self.browser = pw.chromium.launch(channel=channel, headless=headless, args=args)
        # 새로 생긴 브라우저 루트 프로세스 (--remote-debugging-pipe) 를 찾아 하위 프로세스까지 추적
        self.root = None
        for p in psutil.process_iter(["cmdline"]):
            try:
                if p.pid not in before and "--remote-debugging-pipe" in " ".join(p.info["cmdline"] or []):
                    self.root = p
                    break
            except (psutil.NoSuchProcess, psutil.AccessDenied):
                pass
        self.ctx = self.browser.new_context(viewport={"width": 1400, "height": 820})
        info = self.browser.new_browser_cdp_session().send("SystemInfo.getInfo")
        fs = info["gpu"]["featureStatus"]
        self.gpu_status = {k: fs.get(k) for k in ("2d_canvas", "gpu_compositing", "rasterization")}
        dev = info["gpu"]["devices"]
        self.gpu_device = dev[0].get("deviceString") if dev else None

    def procs(self):
        if not self.root:
            return []
        try:
            return [self.root] + self.root.children(recursive=True)
        except psutil.NoSuchProcess:
            return []

    def cpu_seconds(self):
        total = 0.0
        for p in self.procs():
            try:
                t = p.cpu_times()
                total += t.user + t.system
            except psutil.NoSuchProcess:
                pass
        return total

    def memory_mb(self):
        total = 0
        for p in self.procs():
            try:
                mi = p.memory_info()
                total += getattr(mi, "private", mi.rss)
            except psutil.NoSuchProcess:
                pass
        return round(total / 1024 / 1024, 1)

    def close(self):
        self.browser.close()


def server_proc():
    for p in psutil.process_iter(["name"]):
        if (p.info["name"] or "").lower().startswith("bs-server"):
            return p
    return None


def page_metrics(cdp):
    m = {x["name"]: x["value"] for x in cdp.send("Performance.getMetrics")["metrics"]}
    return m


def new_page(br, url, throttle=1):
    page = br.ctx.new_page()
    cdp = br.ctx.new_cdp_session(page)
    cdp.send("Performance.enable")
    if throttle > 1:
        cdp.send("Emulation.setCPUThrottlingRate", {"rate": throttle})
    page.goto(url)
    page.wait_for_function("typeof window.__bsPerfDump === 'function'")
    # headless 에서는 앞으로 가져오지 않은 탭의 rAF 가 초당 2회 수준으로 제한되므로 명시적으로 활성화
    page.bring_to_front()
    return page, cdp


RAF_RATE_JS = """new Promise(r => { let n = 0; const t0 = performance.now();
  function f() { n++; if (performance.now() - t0 < 1000) requestAnimationFrame(f); else r(n); }
  requestAnimationFrame(f); })"""


def collect(page):
    raw = page.evaluate("window.__bsPerfDump()")
    return json.loads(raw)


def run_window(brs, pages, seconds, drive=None, throttled=False):
    """seconds 동안 실행하며 메인 스레드/브라우저/서버 CPU 를 측정"""
    srv = server_proc()
    for page, _ in pages:
        page.evaluate("window.__bsPerfReset()")
    m0 = [page_metrics(c) for _, c in pages]
    c0 = sum(b.cpu_seconds() for b in brs)
    s0 = sum(srv.cpu_times()[:2]) if srv else None
    t0 = time.perf_counter()
    end = t0 + seconds
    rnd = random.Random(7)
    keys = ["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowLeft", "ArrowRight", "ArrowDown", "Space"]
    i = 0
    while time.perf_counter() < end:
        if drive:
            page = pages[i % len(pages)][0]
            page.keyboard.press(rnd.choice(keys))
            i += 1
            time.sleep(max(0.0, drive / len(pages)))
        else:
            time.sleep(0.2)
    wall = time.perf_counter() - t0
    m1 = [page_metrics(c) for _, c in pages]
    c1 = sum(b.cpu_seconds() for b in brs)
    s1 = sum(srv.cpu_times()[:2]) if srv else None
    perf = [collect(p) for p, _ in pages]
    raf = [p.evaluate(RAF_RATE_JS) for p, _ in pages]

    def agg(key):
        vals = []
        for d in perf:
            vals += d.get(key, [])
        return summary(vals)

    main_busy = [
        round(100 * (b["TaskDuration"] - a["TaskDuration"]) / wall, 2) for a, b in zip(m0, m1)
    ]
    script = [round(100 * (b["ScriptDuration"] - a["ScriptDuration"]) / wall, 2) for a, b in zip(m0, m1)]
    style_layout = [
        round(
            100
            * ((b["RecalcStyleDuration"] - a["RecalcStyleDuration"]) + (b["LayoutDuration"] - a["LayoutDuration"]))
            / wall,
            2,
        )
        for a, b in zip(m0, m1)
    ]
    heap = [round(m["JSHeapUsedSize"] / 1024 / 1024, 2) for m in m1]
    return {
        "seconds": round(wall, 1),
        "pages": len(pages),
        "raf_per_sec_check": raf,
        "inputs_sent": i,
        "input_to_dom_ms": agg("input"),
        "input_to_next_frame_ms": agg("frame"),
        "own_board_update_ms": agg("board"),
        "opp_boards_update_ms": agg("opp"),
        "renders_per_sec_per_page": round(sum(d["renders"] for d in perf) / len(perf) / wall, 1),
        "rtt_ms": [d["rtt"] for d in perf],
        "main_thread_busy_pct": main_busy,
        "script_pct": script,
        "style_layout_pct": style_layout,
        "js_heap_mb": heap,
        # CDP CPU 감속은 대기 루프로 구현되어 프로세스 CPU 를 부풀리므로 감속 시나리오에서는 기록하지 않는다
        "browser_cpu_pct_of_one_core": None if throttled else max(0.0, round(100 * (c1 - c0) / wall, 1)),
        "browser_private_mem_mb": round(sum(b.memory_mb() for b in brs), 1),
        "browsers": len(brs),
        "wasm_memory_mb": [round(d.get("wasm_mem", 0) / 1024 / 1024, 2) for d in perf],
        "server_cpu_pct_of_one_core": round(100 * (s1 - s0) / wall, 2) if srv else None,
        "server_rss_mb": round(srv.memory_info().rss / 1024 / 1024, 1) if srv else None,
    }


def scenario_load(pw, channel, gpu, throttle=1):
    br = Browser(pw, channel, gpu)
    page = br.ctx.new_page()
    if throttle > 1:
        br.ctx.new_cdp_session(page).send("Emulation.setCPUThrottlingRate", {"rate": throttle})
    page.goto(BASE + "/")
    page.wait_for_function("performance.getEntriesByName('bs-board-ready').length > 0", timeout=15000)
    data = page.evaluate(
        """() => {
        const nav = performance.getEntriesByType('navigation')[0];
        const res = performance.getEntriesByType('resource').map(r => ({
            name: r.name.split('/').pop(), transfer: r.transferSize, encoded: r.encodedBodySize, decoded: r.decodedBodySize,
            duration: Math.round(r.duration)}));
        const ready = performance.getEntriesByName('bs-board-ready')[0].startTime;
        return {domContentLoaded: Math.round(nav.domContentLoadedEventEnd), load: Math.round(nav.loadEventEnd),
                boardReady: Math.round(ready), htmlTransfer: nav.transferSize, resources: res};
    }"""
    )
    data["total_transfer_bytes"] = data["htmlTransfer"] + sum(r["transfer"] for r in data["resources"])
    data["gpu"] = br.gpu_status
    br.close()
    return data


def scenario_bench(pw, channel, gpu, renderer, throttle, seconds, low=False):
    br = Browser(pw, channel, gpu)
    url = f"{BASE}/?bench={renderer}&opps=5" + ("&low=1" if low else "")
    page, cdp = new_page(br, url, throttle)
    time.sleep(1.5)
    r = run_window([br], [(page, cdp)], seconds, throttled=throttle > 1)
    r["gpu"] = br.gpu_status
    r["gpu_device"] = br.gpu_device
    br.close()
    return r


def scenario_idle(pw, channel, gpu, seconds):
    br = Browser(pw, channel, gpu)
    page, cdp = new_page(br, BASE + "/")
    page.wait_for_selector(".conn.ok")
    time.sleep(1.0)
    r = run_window([br], [(page, cdp)], seconds)
    br.close()
    return r


def scenario_live(pw, channel, gpu, players, seconds, throttle=1, renderer="canvas"):
    """플레이어마다 별도 브라우저 프로세스 (실제로는 각자 다른 PC). headless 는 창마다 활성 탭이 하나뿐이다."""
    brs = [Browser(pw, channel, gpu) for _ in range(players)]
    room = "M" + uuid.uuid4().hex[:5].upper()
    pages = []
    for br in brs:
        pg, cdp = new_page(br, f"{BASE}/?room={room}", throttle)
        pg.wait_for_selector(".conn.ok")
        pages.append((pg, cdp))
    host = pages[0][0]
    host.wait_for_function(f"document.querySelectorAll('.players tbody tr').length >= {players}")
    host.click("text=게임 시작")
    host.wait_for_function("document.querySelector('.st-phase').textContent.includes('진행')", timeout=10000)
    time.sleep(0.5)
    # 플레이어마다 초당 약 6회 입력
    r = run_window(brs, pages, seconds, drive=0.16, throttled=throttle > 1)
    r["renderer"] = renderer
    r["gpu"] = brs[0].gpu_status
    for br in brs:
        br.close()
    return r


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--channel", default="msedge")
    ap.add_argument("--quick", action="store_true")
    ap.add_argument("--only", default="", help="이름에 이 문자열이 포함된 시나리오만 실행")
    ap.add_argument("--out", default="results", help="결과 파일 이름 (확장자 제외)")
    a = ap.parse_args()
    secs = 8 if a.quick else 20
    os.makedirs(OUT, exist_ok=True)
    results = {"channel": a.channel, "base": BASE, "cpu": None, "scenarios": {}}
    try:
        import platform

        results["cpu"] = platform.processor()
        results["logical_cpus"] = psutil.cpu_count()
        results["ram_gb"] = round(psutil.virtual_memory().total / 1024**3)
    except Exception:
        pass

    plan = [
        ("load_gpu_on", lambda pw: scenario_load(pw, a.channel, True)),
        ("load_gpu_off", lambda pw: scenario_load(pw, a.channel, False)),
        ("load_gpu_off_cpu4x", lambda pw: scenario_load(pw, a.channel, False, 4)),
        ("idle_lobby_gpu_off", lambda pw: scenario_idle(pw, a.channel, False, secs)),
    ]
    for gpu in (True, False):
        for rend in ("dom", "canvas"):
            plan.append((f"bench_{rend}_gpu_{'on' if gpu else 'off'}", lambda pw, g=gpu, r=rend: scenario_bench(pw, a.channel, g, r, 1, secs)))
    for rend in ("dom", "canvas"):
        plan.append((f"bench_{rend}_gpu_off_cpu4x", lambda pw, r=rend: scenario_bench(pw, a.channel, False, r, 4, secs)))
    for rend in ("dom", "canvas"):
        plan.append((f"bench_{rend}_gpu_off_cpu4x_lowspec", lambda pw, r=rend: scenario_bench(pw, a.channel, False, r, 4, secs, low=True)))
    plan.append(("live_2p_gpu_on", lambda pw: scenario_live(pw, a.channel, True, 2, secs)))
    plan.append(("live_2p_gpu_off", lambda pw: scenario_live(pw, a.channel, False, 2, secs)))
    plan.append(("live_6p_gpu_off", lambda pw: scenario_live(pw, a.channel, False, 6, secs)))
    plan.append(("live_6p_gpu_off_cpu4x", lambda pw: scenario_live(pw, a.channel, False, 6, secs, throttle=4)))

    with sync_playwright() as pw:
        for name, fn in plan:
            if a.only and a.only not in name:
                continue
            print(f"[측정] {name} ...", flush=True)
            try:
                results["scenarios"][name] = fn(pw)
            except Exception as e:  # 실패도 결과로 남긴다
                results["scenarios"][name] = {"error": repr(e)}
                print("   실패:", e, flush=True)

    with open(os.path.join(OUT, a.out + ".json"), "w", encoding="utf-8") as f:
        json.dump(results, f, ensure_ascii=False, indent=2)
    write_md(results, a.out)
    print("저장:", os.path.join(OUT, a.out + ".json"))


def fmt(s, k="p50"):
    if not isinstance(s, dict) or s.get("n", 0) == 0:
        return "-"
    return f"{s[k]:.2f}"


def write_md(r, fname="results"):
    lines = ["| 시나리오 | 입력→DOM p50/p95 (ms) | 입력→다음 프레임 p50 | 내 판 갱신 p50/max | 상대 판 갱신 p50 | 메인스레드 % | 브라우저 CPU % | 메모리 MB | 서버 CPU % |", "|---|---|---|---|---|---|---|---|---|"]
    for name, s in r["scenarios"].items():
        if "error" in s or name.startswith("load"):
            continue
        lines.append(
            f"| {name} | {fmt(s['input_to_dom_ms'])} / {fmt(s['input_to_dom_ms'], 'p95')} | {fmt(s['input_to_next_frame_ms'])} | "
            f"{fmt(s['own_board_update_ms'])} / {fmt(s['own_board_update_ms'], 'max')} | {fmt(s['opp_boards_update_ms'])} | "
            f"{s['main_thread_busy_pct']} | {'n/a(감속)' if s['browser_cpu_pct_of_one_core'] is None else s['browser_cpu_pct_of_one_core']} | {s['browser_private_mem_mb']} | {s['server_cpu_pct_of_one_core']} |"
        )
    with open(os.path.join(OUT, fname + ".md"), "w", encoding="utf-8") as f:
        f.write("\n".join(lines) + "\n")


if __name__ == "__main__":
    sys.exit(main())
