#!/usr/bin/env python3
"""Headless battle/mobile regressions. Usage: python3 tools/smoke_battle.py --url http://localhost:3030"""
import argparse
from pathlib import Path
from playwright.sync_api import sync_playwright, expect

# Observe outgoing ClientMsg::Input (postcard variant 4) without changing gameplay.
# Its final enum field maps Left/Right/CW/CCW/SoftDrop/HardDrop/Hold to 0..6.
SPY = '''(() => {
  window.__battleInputs = [];
  const send = WebSocket.prototype.send;
  WebSocket.prototype.send = function(data) {
    const bytes = Array.from(new Uint8Array(data.buffer || data, data.byteOffset || 0, data.byteLength));
    if (bytes[0] === 4) window.__battleInputs.push(bytes[bytes.length - 1]);
    return send.call(this, data);
  };
  window.__touchIds = {};
  document.addEventListener('pointerdown', e => {
    const button = e.target.closest('.touch-button');
    if (button) window.__touchIds[button.getAttribute('aria-label')] = e.pointerId;
  }, true);
})();'''


def count(page, action):
    return page.evaluate('(action) => window.__battleInputs.filter(x => x === action).length', action)


def point(locator, identity=1):
    box = locator.bounding_box()
    assert box
    return {'x': box['x'] + box['width'] / 2, 'y': box['y'] + box['height'] / 2, 'id': identity, 'radiusX': 2, 'radiusY': 2}


def touch(session, kind, points=()):
    session.send('Input.dispatchTouchEvent', {'type': kind, 'touchPoints': list(points)})


def active_center(page):
    # Before the first lock, bright saturated pixels in the top 65% are the
    # active piece; the dark ghost, grid and board background are excluded.
    return page.locator('#my-board canvas').evaluate('''canvas => {
      const width = canvas.width, height = Math.floor(canvas.height * .65);
      const rgba = canvas.getContext('2d').getImageData(0, 0, width, height).data;
      let total = 0, count = 0;
      for (let i = 0; i < rgba.length; i += 4) {
        const channels = [rgba[i], rgba[i+1], rgba[i+2]];
        if (Math.max(...channels) > 100 && Math.max(...channels) - Math.min(...channels) > 50) {
          total += (i / 4) % width; count++;
        }
      }
      return count ? total / count / width : null;
    }''')


def stopped(page, action):
    page.wait_for_timeout(90)  # Let already-sent input acknowledgements settle.
    before = count(page, action)
    page.wait_for_timeout(300)
    assert count(page, action) == before, f'Action {action} kept repeating after release/cancel'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--url', default='http://localhost:3030')
    parser.add_argument('--screenshot', default='/tmp/block-stack-battle.png')
    args = parser.parse_args()
    with sync_playwright() as pw:
        browser = pw.chromium.launch(channel='chrome', headless=True)
        mobile = browser.new_context(viewport={'width': 390, 'height': 844}, is_mobile=True, has_touch=True)
        mobile.add_init_script(SPY)
        a = mobile.new_page()
        errors = []
        a.on('pageerror', lambda error: errors.append(str(error)))
        a.goto(args.url)
        expect(a.locator('#chat-input')).to_be_enabled()
        expect(a.locator('.touch-controls')).to_be_visible()
        expect(a.get_by_role('button', name='왼쪽 이동', exact=True)).to_be_disabled()
        a.screenshot(path=str(Path(args.screenshot).with_name(Path(args.screenshot).stem + '-lobby.png')))
        a.get_by_role('button', name='방 찾기', exact=True).click()
        a.get_by_role('button', name='새 방 만들기', exact=True).click()
        expect(a.locator('#chat-input')).to_be_enabled()
        desktop = browser.new_context(viewport={'width': 1512, 'height': 850})
        b = desktop.new_page()
        b.goto(a.url)
        expect(b.locator('#chat-input')).to_be_enabled()
        expect(b.locator('.touch-controls')).to_be_hidden()
        expect(a.locator('.arc-roster-player')).to_have_count(2)
        b.locator('.room-controls .ready-button').click()
        a.get_by_role('button', name='준비하고 시작  →', exact=True).click()
        expect(a.locator('.arc-phase')).to_have_text('BATTLE IN PROGRESS', timeout=15000)
        expect(a.locator('.match-clock strong')).to_have_text('2:00')
        expect(a.locator('.match-clock strong')).to_have_text('1:59', timeout=4000)
        controls = a.locator('.touch-controls')
        controls.scroll_into_view_if_needed()
        a.evaluate("window.scrollBy(0, document.querySelector('#my-board').getBoundingClientRect().top - 8)")
        a.wait_for_function('document.documentElement.scrollWidth <= 391 && innerWidth <= 391')
        geometry = a.evaluate('''() => {
          const board = document.querySelector('#my-board').getBoundingClientRect();
          const controls = document.querySelector('.touch-controls').getBoundingClientRect();
          return {boardTop:board.top, controlsBottom:controls.bottom, height:innerHeight};
        }''')
        assert geometry['boardTop'] >= 0 and geometry['controlsBottom'] <= geometry['height'], f'Board and controls not together in viewport: {geometry}'
        session = mobile.new_cdp_session(a)
        left = a.get_by_role('button', name='왼쪽 이동', exact=True)
        right = a.get_by_role('button', name='오른쪽 이동', exact=True)
        expect(left).to_be_enabled()
        before = count(a, 0)
        touch(session, 'touchStart', [point(left)])
        a.wait_for_timeout(500)
        assert count(a, 0) >= before + 3, 'Long press did not repeat left inputs'
        touch(session, 'touchEnd')
        stopped(a, 0)
        left_center = active_center(a)
        before = count(a, 1)
        touch(session, 'touchStart', [point(right)])
        a.wait_for_timeout(350)
        assert count(a, 1) >= before + 2
        touch(session, 'touchCancel')
        stopped(a, 1)
        right_center = active_center(a)
        assert left_center is not None and right_center is not None and right_center > left_center + .2, f'Canvas did not reflect movement: {left_center}, {right_center}'
        # Releasing real pointer capture must stop repetition before pointerup.
        touch(session, 'touchStart', [point(right)])
        a.wait_for_timeout(220)
        moved = point(right)
        moved['x'] += 2
        touch(session, 'touchMove', [moved])
        right.evaluate("button => button.releasePointerCapture(window.__touchIds['오른쪽 이동'])")
        a.wait_for_timeout(30)
        moved['x'] += 2
        touch(session, 'touchMove', [moved])
        stopped(a, 1)
        touch(session, 'touchEnd')
        # Two simultaneous fingers must independently move and rotate.
        rotate = a.get_by_role('button', name='오른쪽 회전', exact=True)
        before_left, before_rotate = count(a, 0), count(a, 2)
        points = [point(left, 1), point(rotate, 2)]
        touch(session, 'touchStart', points)
        a.wait_for_timeout(300)
        assert count(a, 0) > before_left and count(a, 2) > before_rotate
        touch(session, 'touchEnd')
        stopped(a, 0)
        score = a.locator('.arc-stats .arc-big')
        previous_score = score.inner_text()
        a.get_by_role('button', name='바로 내리기', exact=True).tap()
        expect(score).not_to_have_text(previous_score)
        a.screenshot(path=args.screenshot, full_page=True)
        a.screenshot(path=str(Path(args.screenshot).with_name(Path(args.screenshot).stem + '-viewport.png')))
        # Chat remains editable and keyboard arrows in it do not move the game.
        a.locator('#chat-input').fill('모바일 채팅 확인')
        before = count(a, 0)
        a.locator('#chat-input').press('ArrowLeft')
        assert count(a, 0) == before
        a.locator('.chat-form button').click()
        expect(b.locator('.chat-list')).to_contain_text('모바일 채팅 확인')
        spectator_context = browser.new_context(viewport={'width': 390, 'height': 844}, is_mobile=True, has_touch=True)
        spectator = spectator_context.new_page()
        spectator.goto(a.url)
        expect(spectator.locator('#chat-input')).to_be_enabled()
        expect(spectator.get_by_role('button', name='바로 내리기', exact=True)).to_be_disabled()
        # Top out the other player to complete the round without waiting 2 minutes.
        for _ in range(28):
            b.keyboard.press('Space')
            b.wait_for_timeout(45)
        expect(a.locator('.arc-phase')).to_have_text('MATCH COMPLETE', timeout=10000)
        own = a.locator('.battle-stats tbody tr.mine')
        expect(own.locator('td').nth(3)).to_have_text('1')
        expect(own.locator('td').nth(2)).to_have_text('0.0')
        # Same touch controls and statistics are available in spreadsheet mode.
        a.get_by_role('button', name='▤ 업무 화면', exact=True).click()
        expect(a.locator('body')).to_have_class('theme-sheet')
        a.wait_for_function('document.documentElement.scrollWidth <= 391 && innerWidth <= 391')
        expect(a.locator('.touch-controls')).to_be_visible()
        expect(a.locator('.battle-stats')).to_be_visible()
        expect(a.get_by_role('button', name='왼쪽 이동', exact=True)).to_be_disabled()
        # A fresh one-player room has an elapsed clock, not the battle countdown.
        a.get_by_role('button', name='아케이드 모드', exact=True).click()
        a.get_by_role('button', name='방 찾기', exact=True).click()
        a.get_by_role('button', name='새 방 만들기', exact=True).click()
        expect(a.locator('.arc-roster-player')).to_have_count(1)
        a.get_by_role('button', name='준비하고 시작  →', exact=True).click()
        expect(a.locator('.arc-phase')).to_have_text('BATTLE IN PROGRESS', timeout=15000)
        expect(a.locator('.match-clock small')).to_have_text('1인 연습')
        expect(a.locator('.match-clock strong')).to_have_text('0:01', timeout=4000)
        assert not errors, errors
        browser.close()
    print(f'PASS: mobile touch/repeat/cancel/capture/multitouch, clock, stats, spectator/chat, themes, practice. Screenshot: {Path(args.screenshot)}')


if __name__ == '__main__':
    main()
