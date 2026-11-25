#!/usr/bin/env python3
"""대전 규칙 2단계 확인: 3인 방의 공격 대상 선택과 받을 공격 표시.

Usage: python3 tools/smoke_battle_rules.py --url http://localhost:3000
"""
import argparse
from playwright.sync_api import sync_playwright, expect

SPY = '''(() => {
  window.__sent = [];
  const send = WebSocket.prototype.send;
  WebSocket.prototype.send = function(data) {
    window.__sent.push(Array.from(new Uint8Array(data.buffer || data, data.byteOffset || 0, data.byteLength)));
    return send.call(this, data);
  };
  localStorage.setItem('blockstack.settings.v1', 'theme=arcade\\nname=rules-smoke\\nsound=0');
})();'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--url', default='http://localhost:3000')
    args = parser.parse_args()
    with sync_playwright() as pw:
        browser = pw.chromium.launch(channel='chrome', headless=True)
        page = browser.new_context(viewport={'width': 1512, 'height': 900}).new_page()
        page.context.add_init_script(SPY)
        errors = []
        page.on('pageerror', lambda error: errors.append(str(error)))
        page.goto(args.url)
        expect(page.locator('#chat-input')).to_be_enabled()
        select = page.get_by_label('공격 대상 선택')
        expect(select).to_have_count(0)
        add_bot = page.get_by_role('button', name='+ 연습봇', exact=True)
        add_bot.click()
        expect(page.locator('.arc-roster-player')).to_have_count(2)
        expect(select).to_have_count(0)
        add_bot.click()
        expect(page.locator('.arc-roster-player')).to_have_count(3)
        expect(select).to_be_visible()
        select.select_option(label='반격')
        page.wait_for_function('window.__sent.some(b => b.length === 2 && b[0] === 14 && b[1] === 2)')
        page.get_by_role('button', name='준비하고 시작  →', exact=True).click()
        expect(page.locator('.arc-phase')).to_have_text('BATTLE IN PROGRESS', timeout=15000)
        expect(page.locator('.arc-incoming-ready')).to_be_visible()
        expect(select).to_have_value('2')
        assert not errors, errors
        browser.close()
    print('PASS: target mode select appears with 3 players, sends SetTargetMode, incoming display')


if __name__ == '__main__':
    main()
