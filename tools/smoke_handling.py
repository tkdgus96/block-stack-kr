#!/usr/bin/env python3
"""조작감 1단계 확인: 180도 회전, 내리기 반복 0, ARR 0 유지, 경쟁용 프리셋.

Usage: python3 tools/smoke_handling.py --url http://localhost:3000
"""
import argparse
from playwright.sync_api import sync_playwright, expect

# ClientMsg::Input 은 postcard 번호 4, 마지막 바이트가 Input 번호다.
# Left/Right/CW/CCW/SoftDrop/HardDrop/Hold/Rotate180 = 0..7
LEFT, SOFT, HARD, R180 = 0, 4, 5, 7
SPY = '''(() => {
  window.__inputs = [];
  const send = WebSocket.prototype.send;
  WebSocket.prototype.send = function(data) {
    const bytes = Array.from(new Uint8Array(data.buffer || data, data.byteOffset || 0, data.byteLength));
    if (bytes[0] === 4) window.__inputs.push(bytes[bytes.length - 1]);
    return send.call(this, data);
  };
  if (!localStorage.getItem('blockstack.settings.v1')) {
    localStorage.setItem('blockstack.settings.v1', 'theme=arcade\\nname=handling-smoke\\nsound=0\\ndas=120\\narr=0\\nsdr=0');
  }
})();'''


def count(page, action):
    return page.evaluate('(a) => window.__inputs.filter(x => x === a).length', action)


def settings(page):
    raw = page.evaluate("localStorage.getItem('blockstack.settings.v1')")
    return dict(line.split('=', 1) for line in raw.splitlines() if '=' in line)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--url', default='http://localhost:3000')
    args = parser.parse_args()
    with sync_playwright() as pw:
        browser = pw.chromium.launch(channel='chrome', headless=True)
        context = browser.new_context(viewport={'width': 1512, 'height': 900})
        context.add_init_script(SPY)
        page = context.new_page()
        errors = []
        page.on('pageerror', lambda error: errors.append(str(error)))
        page.goto(args.url)
        expect(page.locator('#chat-input')).to_be_enabled()
        page.get_by_role('button', name='준비하고 시작  →', exact=True).click()
        expect(page.locator('.arc-phase')).to_have_text('BATTLE IN PROGRESS', timeout=15000)
        page.locator('#my-board').click()

        # 180도 회전: A 키 한 번에 입력 7 이 한 번 나간다.
        page.keyboard.press('KeyA')
        page.wait_for_timeout(80)
        assert count(page, R180) == 1, f'Rotate180 not sent: {count(page, R180)}'

        # 내리기 반복 0: 한 번 누르면 바닥까지 내려가고, 바닥에서 실패할 입력은 보내지 않는다.
        before = count(page, SOFT)
        page.keyboard.press('ArrowDown')
        page.wait_for_timeout(150)
        soft = count(page, SOFT) - before
        assert 15 <= soft <= 20, f'Instant soft drop sent {soft} inputs (expected to reach the floor)'
        page.keyboard.press('ArrowDown')
        page.wait_for_timeout(100)
        assert count(page, SOFT) - before == soft, 'soft drop at the floor must not send inputs'

        # ARR 0 유지: 왼쪽을 누른 채로 있으면 벽에 닿은 뒤 입력을 더 보내지 않는다.
        # 바닥에 놓인 블록은 0.5초 뒤 고정되므로 새 블록으로 시작한다.
        page.keyboard.press('Space')
        page.wait_for_timeout(100)
        page.keyboard.down('ArrowLeft')
        page.wait_for_timeout(400)
        first = count(page, LEFT)
        assert 1 <= first <= 6, f'ARR 0 sent {first} left inputs'
        page.wait_for_timeout(300)
        assert count(page, LEFT) == first, 'holding at the wall kept sending inputs'
        # 누른 채로 바로 내리면 다음 블록도 왼쪽 벽까지 간다.
        page.keyboard.press('Space')
        page.wait_for_timeout(250)
        assert count(page, LEFT) > first, 'DAS was not kept for the next piece'
        page.keyboard.up('ArrowLeft')
        after_release = count(page, LEFT)
        page.keyboard.press('Space')
        page.wait_for_timeout(250)
        assert count(page, LEFT) == after_release, 'left kept repeating after release'

        # 경쟁용 프리셋: 조작 값만 바꾸고 키 배치와 이름은 그대로 둔다.
        page.evaluate("localStorage.setItem('blockstack.settings.v1', 'theme=arcade\\nname=handling-smoke\\nsound=0\\ndas=200\\narr=40\\nsdr=40\\nkey0=KeyJ')")
        page.reload()
        page.locator('.arc-nav').get_by_role('button', name='설정', exact=True).click()
        expect(page.get_by_text('180도 회전', exact=True)).to_be_visible()
        page.get_by_role('button', name='적용', exact=True).click()
        page.wait_for_function("localStorage.getItem('blockstack.settings.v1').includes('arr=0')")
        s = settings(page)
        assert (s['das'], s['arr'], s['sdr'], s['key0'], s['name']) == ('100', '0', '0', 'KeyJ', 'handling-smoke'), s
        assert s['key7'] == 'KeyA', s

        assert not errors, errors
        browser.close()
    print('PASS: rotate 180, instant soft drop, ARR 0 hold across pieces, competitive preset')


if __name__ == '__main__':
    main()
