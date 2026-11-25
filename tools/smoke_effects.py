#!/usr/bin/env python3
"""아케이드 연출 5단계 확인: 줄 삭제 빛, 큰 공격 흔들림, 애니메이션을 끄면 효과 없음.

smoke_combat.py 의 메시지 주입 방식을 그대로 쓴다(운영 코드에 디버그 hook 없음).
Usage: python3 tools/smoke_effects.py --url http://localhost:3000
"""
import argparse
from playwright.sync_api import sync_playwright, expect
from smoke_combat import INIT, event, inject

WATCH = '''() => {
  window.__fx = [];
  const wrap = document.querySelector('.arc-board-wrap');
  new MutationObserver(() => wrap.classList.forEach(c => c.startsWith('fx-') && window.__fx.push(c)))
    .observe(wrap, {attributes: true, attributeFilter: ['class']});
}'''


def start(browser, url, settings):
    contexts = [browser.new_context(viewport={'width': 1512, 'height': 900}) for _ in range(2)]
    contexts[0].add_init_script(INIT)
    contexts[0].add_init_script(f"localStorage.setItem('blockstack.settings.v1', '{settings}')")
    a, b = [c.new_page() for c in contexts]
    a.goto(url)
    expect(a.locator('#chat-input')).to_be_enabled()
    b.goto(a.url)
    expect(a.locator('.arc-roster-player')).to_have_count(2)
    b.locator('.room-controls .ready-button').click()
    a.get_by_role('button', name='준비하고 시작  →', exact=True).click()
    expect(a.locator('.arc-phase')).to_have_text('BATTLE IN PROGRESS', timeout=15000)
    a.wait_for_function('window.__combatV2 > 0')
    a.evaluate('window.__combatFixtureMode = true')
    a.evaluate(WATCH)
    match_id = 900000
    return a, b, match_id


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--url', default='http://localhost:3000')
    args = parser.parse_args()
    with sync_playwright() as pw:
        browser = pw.chromium.launch(channel='chrome', headless=True)
        a, _, mid = start(browser, args.url, 'theme=arcade\\nname=fx\\nanim=1')
        errors = []
        a.on('pageerror', lambda e: errors.append(str(e)))
        # 내가 테트리스: 큰 빛
        inject(a, event(1, player=1, match_id=mid, cleared=4, spin=0, combo=0, generated=4, cancelled=0, sent=4, target=2))
        a.wait_for_function("window.__fx.includes('fx-big-clear')")
        # 일반 2줄: 작은 빛
        inject(a, event(2, player=1, match_id=mid, cleared=2, spin=0, combo=1, generated=1, cancelled=0, sent=1, target=2))
        a.wait_for_function("window.__fx.includes('fx-clear')")
        # 상대가 나에게 5줄: 흔들림. 3줄은 흔들지 않는다.
        inject(a, event(1, player=2, match_id=mid, cleared=2, spin=0, combo=0, generated=3, cancelled=0, sent=3, target=1))
        a.wait_for_timeout(400)
        assert 'fx-shake' not in a.evaluate('window.__fx'), 'small attack must not shake'
        inject(a, event(2, player=2, match_id=mid, cleared=4, spin=0, combo=0, generated=5, cancelled=0, sent=5, target=1))
        a.wait_for_function("window.__fx.includes('fx-shake')")
        a.wait_for_timeout(500)
        assert not a.locator('.arc-board-wrap').evaluate("w => [...w.classList].some(c => c.startsWith('fx-'))"), 'effect classes are removed'

        # 애니메이션을 끈 사용자(기본값)는 효과가 없다.
        c, _, mid = start(browser, args.url, 'theme=arcade\\nname=fx2\\nanim=0')
        inject(c, event(1, player=1, match_id=mid, cleared=4, spin=0, combo=0, generated=4, cancelled=0, sent=4, target=2))
        inject(c, event(1, player=2, match_id=mid, cleared=4, spin=0, combo=0, generated=5, cancelled=0, sent=5, target=1))
        c.wait_for_timeout(500)
        assert c.evaluate('window.__fx') == [], c.evaluate('window.__fx')
        assert not errors, errors
        browser.close()
    print('PASS: clear flash, tetris flash, heavy-hit shake, no effects when animations are off')


if __name__ == '__main__':
    main()
