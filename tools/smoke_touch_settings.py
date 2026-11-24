#!/usr/bin/env python3
"""Isolated mobile settings regression. Requires playwright + headless Chrome.

python3 tools/smoke_touch_settings.py --url http://localhost:3030
"""
import argparse
import re
from pathlib import Path
from playwright.sync_api import sync_playwright, expect

INIT = '''(() => {
  if (!localStorage.getItem('blockstack.settings.v1')) {
    localStorage.setItem('blockstack.settings.v1', 'theme=arcade\\nname=mobile-smoke\\nsound=0\\ndas=200');
  }
  window.__vibrations = [];
  Object.defineProperty(navigator, 'vibrate', {configurable:true, value: duration => {
    window.__vibrations.push(duration); return true;
  }});
})();'''


def settings(page):
    page.locator('.arc-nav').get_by_role('button', name='설정', exact=True).click()
    expect(page.locator('.mobile-settings')).to_be_visible()


def arena(page):
    page.locator('.arc-nav').get_by_role('button', name='대전 아레나', exact=True).click()
    expect(page.locator('.touch-controls')).to_be_visible()


def slider(page, name, value):
    page.get_by_role('slider', name=name, exact=True).evaluate('''(input, value) => {
      input.value = value; input.dispatchEvent(new Event('input', {bubbles:true}));
    }''', str(value))


def saved(page):
    return dict(line.split('=', 1) for line in page.evaluate("localStorage.getItem('blockstack.settings.v1')").splitlines() if '=' in line)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--url', default='http://localhost:3030')
    parser.add_argument('--screenshot', default='/tmp/block-stack-touch-settings.png')
    args = parser.parse_args()
    errors = []
    with sync_playwright() as pw:
        browser = pw.chromium.launch(channel='chrome', headless=True)
        context = browser.new_context(viewport={'width': 390, 'height': 844}, is_mobile=True, has_touch=True)
        context.add_init_script(INIT)
        page = context.new_page()
        page.on('pageerror', lambda error: errors.append(str(error)))
        page.goto(args.url)
        expect(page.locator('#chat-input')).to_be_enabled()
        settings(page)
        expect(page.get_by_role('slider', name='터치 버튼 크기', exact=True)).to_have_value('100')
        expect(page.get_by_role('slider', name='터치 버튼 불투명도', exact=True)).to_have_value('100')
        # Settings from the earlier storage format still load and survive updates.
        slider(page, '터치 버튼 크기', 125)
        slider(page, '터치 버튼 불투명도', 40)
        page.get_by_role('checkbox', name=re.compile('왼손 모드')).check()
        page.get_by_role('combobox', name='터치 버튼 배치', exact=True).select_option('1')
        page.get_by_role('checkbox', name=re.compile('짧은 진동')).check()
        snapshot = saved(page)
        assert {key: snapshot[key] for key in ['touch_size', 'touch_opacity', 'touch_left_handed', 'touch_layout', 'touch_vibration']} == {
            'touch_size': '125', 'touch_opacity': '40', 'touch_left_handed': '1', 'touch_layout': '1', 'touch_vibration': '1'}
        assert snapshot['name'] == 'mobile-smoke' and snapshot['das'] == '200' and snapshot['sound'] == '0'
        page.screenshot(path=str(Path(args.screenshot).with_name(Path(args.screenshot).stem + '-options.png')), full_page=True)
        page.reload()
        expect(page.locator('#chat-input')).to_be_enabled()
        settings(page)
        expect(page.get_by_role('slider', name='터치 버튼 크기', exact=True)).to_have_value('125')
        expect(page.get_by_role('slider', name='터치 버튼 불투명도', exact=True)).to_have_value('40')
        expect(page.get_by_role('checkbox', name=re.compile('왼손 모드'))).to_be_checked()
        expect(page.get_by_role('checkbox', name=re.compile('짧은 진동'))).to_be_checked()
        expect(page.get_by_role('combobox', name='터치 버튼 배치', exact=True)).to_have_value('1')
        arena(page)
        controls = page.locator('.touch-controls')
        expect(controls).to_have_class(re.compile('touch-left-handed'))
        expect(controls).to_have_class(re.compile('touch-movement-first'))
        hard = controls.get_by_role('button', name='바로 내리기', exact=True)
        hold = controls.get_by_role('button', name='보관', exact=True)
        left = controls.get_by_role('button', name='왼쪽 이동', exact=True)
        right = controls.get_by_role('button', name='오른쪽 이동', exact=True)
        assert hard.bounding_box()['x'] < hold.bounding_box()['x']
        assert left.bounding_box()['x'] < right.bounding_box()['x']
        assert left.bounding_box()['y'] < hard.bounding_box()['y']
        large_height = left.bounding_box()['height']
        assert large_height >= 44
        # The minimum setting retains accessible targets and changes live, without reload.
        settings(page)
        slider(page, '터치 버튼 크기', 80)
        arena(page)
        assert 44 <= left.bounding_box()['height'] < large_height
        settings(page)
        slider(page, '터치 버튼 크기', 125)
        arena(page)
        # Create an independent practice room so existing server rooms cannot interfere.
        page.locator('.arc-nav').get_by_role('button', name='방 찾기', exact=True).click()
        page.get_by_role('button', name='새 방 만들기', exact=True).click()
        expect(page.locator('.arc-roster-player')).to_have_count(1)
        page.get_by_role('button', name='준비하고 시작  →', exact=True).click()
        expect(left).to_be_enabled(timeout=15000)
        expect(left).to_have_css('opacity', '0.4')
        # Test the maximum size while the board and controls share a portrait viewport.
        controls.scroll_into_view_if_needed()
        page.evaluate("window.scrollBy(0, document.querySelector('#my-board').getBoundingClientRect().top - 5)")
        page.wait_for_function('document.documentElement.scrollWidth <= 391 && innerWidth <= 391')
        geometry = page.evaluate('''() => ({top:document.querySelector('#my-board').getBoundingClientRect().top,
          bottom:document.querySelector('.touch-controls').getBoundingClientRect().bottom, height:innerHeight})''')
        assert geometry['top'] >= 0 and geometry['bottom'] <= geometry['height'], geometry
        session = context.new_cdp_session(page)
        rect = left.bounding_box()
        point = {'x': rect['x'] + rect['width']/2, 'y': rect['y'] + rect['height']/2, 'id': 1}
        before = page.evaluate('window.__vibrations.length')
        session.send('Input.dispatchTouchEvent', {'type': 'touchStart', 'touchPoints': [point]})
        page.wait_for_timeout(450)
        session.send('Input.dispatchTouchEvent', {'type': 'touchEnd', 'touchPoints': []})
        assert page.evaluate('window.__vibrations.length') == before + 1, 'Held repeat must not vibrate repeatedly'
        score = page.locator('.arc-stats .arc-big')
        old = score.inner_text()
        hard.tap()
        expect(score).not_to_have_text(old)
        assert page.evaluate('window.__vibrations.length') == before + 2
        # Unsupported API must silently skip vibration while gameplay still works.
        page.evaluate("Object.defineProperty(navigator, 'vibrate', {configurable:true, value:undefined})")
        old = score.inner_text()
        hard.tap()
        expect(score).not_to_have_text(old)
        assert not errors, errors
        page.screenshot(path=args.screenshot)
        browser.close()
    print(f'PASS: old settings compatibility, persistence, size/opacity, handedness/layout, minimum targets, max-size portrait, one-shot/unsupported vibration. Screenshot: {args.screenshot}')


if __name__ == '__main__':
    main()
