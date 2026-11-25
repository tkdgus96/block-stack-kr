#!/usr/bin/env python3
"""Combat UI regression with isolated browser-only incoming-message fixtures.

Creates a real two-player game, verifies EnableCombat/V2 negotiation, then injects
postcard messages through that isolated page's WebSocket. Fixtures verify only UI
behavior; authoritative T-spin/attack rules are covered by native engine/server tests.
No production debug hooks or persistent browser profiles are used.

python3 tools/smoke_combat.py --url http://localhost:3032
"""
import argparse
import re
from pathlib import Path
from playwright.sync_api import sync_playwright, expect

INIT = '''(() => {
  const OriginalWebSocket = window.WebSocket;
  window.__combatSent = [];
  window.__combatV2 = 0;
  window.__combatFixtureMode = false;
  window.WebSocket = class extends OriginalWebSocket {
    constructor(...args) {
      super(...args);
      window.__combatSocket = this;
      this.addEventListener('message', event => {
        if (!(event.data instanceof ArrayBuffer)) return;
        const bytes = new Uint8Array(event.data);
        if (bytes[0] === 19) {
          window.__combatV2++;
        }
        if (window.__combatFixtureMode && [19,20,21].includes(bytes[0]) && !event.__fixture) event.stopImmediatePropagation();
      }, true);
    }
    send(data) {
      const bytes = new Uint8Array(data.buffer || data, data.byteOffset || 0, data.byteLength);
      window.__combatSent.push(Array.from(bytes));
      return super.send(data);
    }
  };
})();'''


def uint(value):
    assert value >= 0
    out = []
    while value >= 128:
        out.append((value & 127) | 128)
        value >>= 7
    return out + [value]


def signed_byte(value):
    # Postcard serialize_i8 uses the raw two's-complement byte, unlike signed
    # i16/i32/i64 varints, which use zigzag. E.g. -1 => 0xff, 3 => 0x03.
    assert -128 <= value <= 127
    return [value & 255]


def event(piece, *, player=1, match_id=123456, cleared=2, spin=2, combo=3,
          b2b=4, back_to_back=True, perfect_clear=False, generated=6,
          cancelled=2, sent=4, target=2):
    return ([20] + uint(match_id) + [player] + uint(piece) + [cleared] + uint(spin)
            + signed_byte(combo) + uint(b2b) + [int(back_to_back), int(perfect_clear), generated, cancelled, sent]
            + ([0] if target is None else [1, target]))


def status(rows):
    return [21] + uint(len(rows)) + [byte for identity, combo, b2b in rows for byte in [identity, *signed_byte(combo), *uint(b2b)]]


def inject(page, *messages):
    # One evaluate task supports burst delivery before reactive effects flush.
    page.evaluate('''messages => {
      for (const bytes of messages) {
        const event = new MessageEvent('message', {data:new Uint8Array(bytes).buffer});
        event.__fixture = true;
        window.__combatSocket.dispatchEvent(event);
      }
    }''', list(messages))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--url', default='http://localhost:3032')
    parser.add_argument('--screenshot', default='/tmp/block-stack-combat.png')
    args = parser.parse_args()
    errors = []
    with sync_playwright() as pw:
        browser = pw.chromium.launch(channel='chrome', headless=True)
        contexts = [browser.new_context(viewport={'width': 1512, 'height': 900}),
                    browser.new_context(viewport={'width': 1512, 'height': 900})]
        contexts[0].add_init_script(INIT)
        a, b = [context.new_page() for context in contexts]
        for page in (a, b):
            page.on('pageerror', lambda error: errors.append(str(error)))
        a.goto(args.url)
        expect(a.locator('#chat-input')).to_be_enabled()
        a.get_by_role('button', name='방 찾기', exact=True).click()
        old = a.url
        a.get_by_role('button', name='새 방 만들기', exact=True).click()
        a.wait_for_url(lambda url: url != old)
        expect(a.locator('#chat-input')).to_be_enabled()
        b.goto(a.url)
        expect(b.locator('#chat-input')).to_be_enabled()
        expect(a.locator('.arc-roster-player')).to_have_count(2)
        b.locator('.room-controls .ready-button').click()
        a.get_by_role('button', name='준비하고 시작  →', exact=True).click()
        expect(a.locator('.arc-phase')).to_have_text('BATTLE IN PROGRESS', timeout=15000)
        a.wait_for_function('window.__combatV2 > 0')
        assert a.evaluate('window.__combatSent.some(bytes => bytes.length === 1 && bytes[0] === 11)'), 'EnableCombat was not sent'
        assert a.locator('#opp-2').count() == 1, 'Isolated new room should assign own id 1 and opponent 2'
        a.evaluate('window.__combatFixtureMode = true')
        panel = a.locator('.combat-panel')
        combo = panel.locator('.combo-counter b')
        b2b = panel.locator('.b2b-counter b')
        tech = panel.locator('.combat-tech')
        hit = a.locator('.arc-opp-board:has(#opp-2) .combat-hit-frame')
        inject(a, status([(1, -1, 0), (2, -1, 0)]))
        expect(combo).to_have_text('—')
        expect(b2b).to_have_text('—')
        for piece, cleared, label in [(1, 1, 'T-SPIN SINGLE'), (2, 2, 'T-SPIN DOUBLE'), (3, 3, 'T-SPIN TRIPLE')]:
            inject(a, event(piece, cleared=cleared, combo=piece, b2b=piece + 1))
            expect(tech).to_have_text(label)
            expect(combo).to_have_text(str(piece))
            expect(panel.locator('.combat-amount')).to_contain_text('전송 +4줄')
            expect(panel.locator('.combat-amount')).to_contain_text('생성 6 · 상쇄 2')
            expect(hit).to_have_class(re.compile(r'(^|\s)hit(\s|$)'))
        inject(a, event(4, spin=1, cleared=1, combo=4, b2b=5))
        expect(tech).to_have_text('T-SPIN MINI SINGLE')
        inject(a, event(5, spin=0, cleared=4, combo=5, b2b=6, perfect_clear=True, generated=10, sent=8))
        expect(tech).to_have_text('TETRIS')
        expect(panel.locator('.combat-bonuses')).to_contain_text('PERFECT CLEAR')
        expect(panel.locator('.combat-bonuses')).to_contain_text('BACK-TO-BACK +1')
        expect(b2b).to_have_text('×5')
        a.wait_for_timeout(950)
        expect(hit).not_to_have_class(re.compile(r'(^|\s)hit(\s|$)'))
        # A persistent combo survives the temporary notice, and duplicate delivery
        # cannot restart that notice or roll the counter back.
        latest = event(30)
        inject(a, latest)
        expect(combo).to_have_text('3')
        expect(tech).to_have_text('T-SPIN DOUBLE')
        a.wait_for_timeout(2150)
        expect(tech).to_have_count(0)
        expect(combo).to_have_text('3')
        inject(a, latest, event(30, combo=99), event(29, combo=0), event(999, match_id=123455, combo=2))
        expect(combo).to_have_text('3')
        expect(tech).to_have_count(0)
        expect(hit).not_to_have_class(re.compile(r'(^|\s)hit(\s|$)'))
        inject(a, event(31, cleared=0, spin=0, combo=-1, b2b=4, back_to_back=False, generated=0, cancelled=0, sent=0, target=None))
        expect(combo).to_have_text('—')
        expect(b2b).to_have_text('×3')
        expect(tech).to_have_count(0)
        # Locally predicted rotations/movement alone must not announce a skill.
        a.keyboard.press('ArrowUp')
        for _ in range(14):
            a.keyboard.press('ArrowLeft')
        # Further left inputs fail at the wall and still produce no announcement.
        a.keyboard.press('ArrowLeft')
        expect(tech).to_have_count(0)
        # A significant lock followed by no clear in the same delivery task
        # keeps the technique notice while immediately resetting the counter.
        inject(a, event(32, combo=2, b2b=5), event(33, cleared=0, spin=0, combo=-1, b2b=5,
               back_to_back=False, generated=0, cancelled=0, sent=0, target=None))
        expect(tech).to_have_text('T-SPIN DOUBLE')
        expect(combo).to_have_text('—')
        expect(b2b).to_have_text('×4')
        a.wait_for_timeout(950)
        inject(a, event(34, combo=3, b2b=5, cancelled=6, sent=0, target=None))
        expect(panel.locator('.combat-amount')).to_contain_text('전송 +0줄')
        expect(hit).not_to_have_class(re.compile(r'(^|\s)hit(\s|$)'))
        inject(a, event(1, player=2, target=1))
        expect(a.locator('.arc-me .meter')).to_have_class(re.compile('combat-receiving'))
        a.wait_for_timeout(950)
        expect(a.locator('.arc-me .meter')).not_to_have_class(re.compile('combat-receiving'))
        # Responsive portrait layout must accommodate the persistent panel as
        # well as the board and shared touch controls without horizontal overflow.
        a.set_viewport_size({'width': 390, 'height': 844})
        a.locator('.touch-controls').scroll_into_view_if_needed()
        a.evaluate("window.scrollBy(0, document.querySelector('#my-board').getBoundingClientRect().top - 4)")
        a.wait_for_function('document.documentElement.scrollWidth <= 391 && innerWidth <= 391')
        geometry = a.evaluate('''() => ({top:document.querySelector('#my-board').getBoundingClientRect().top,
          bottom:document.querySelector('.touch-controls').getBoundingClientRect().bottom, height:innerHeight})''')
        assert geometry['top'] >= 0 and geometry['bottom'] <= geometry['height'], geometry
        inject(a, event(35, combo=7, b2b=8, perfect_clear=True))
        expect(combo).to_have_text('7')
        a.screenshot(path=args.screenshot)
        a.get_by_role('button', name='▤ 업무 화면', exact=True).click()
        expect(a.locator('body')).to_have_class('theme-sheet')
        expect(a.locator('.combat-panel')).to_be_visible()
        expect(a.locator('.combat-panel .combo-counter b')).to_have_text('7')
        # Remounting the shared panel must not replay an old transient notice.
        expect(a.locator('.combat-panel .combat-tech')).to_have_count(0)
        inject(a, event(36, cleared=3, combo=8, b2b=9))
        expect(a.locator('.combat-panel .combat-tech')).to_have_text('T-SPIN TRIPLE')
        expect(a.locator('.combat-panel .combo-counter b')).to_have_text('8')
        a.screenshot(path=str(Path(args.screenshot).with_name(Path(args.screenshot).stem + '-sheet.png')), full_page=True)
        # MatchSummary closes that match's event window. Started resets transient
        # state while retaining the floor, so delayed previous-match messages are
        # rejected even when their piece ordinal is larger than any seen before.
        summary = [17] + uint(123456) + uint(0) + [0, 0, 0, 0]
        started = [3] + uint(7)
        inject(a, summary, started, event(999, match_id=123456, combo=99))
        expect(a.locator('.combat-panel .combat-tech')).to_have_count(0)
        expect(a.locator('.combat-panel .combo-counter b')).not_to_have_text('99')
        inject(a, event(1, match_id=123457, cleared=1, combo=1, b2b=1, back_to_back=False))
        expect(a.locator('.combat-panel .combat-tech')).to_have_text('T-SPIN SINGLE')
        expect(a.locator('.combat-panel .combo-counter b')).to_have_text('1')
        assert not errors, errors
        browser.close()
    print(f'PASS: real combat opt-in/V2; isolated UI fixtures verify labels, counters, duplicates/order, pulses, burst, mobile and sheet. Screenshot: {args.screenshot}')


if __name__ == '__main__':
    main()
