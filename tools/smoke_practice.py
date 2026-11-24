#!/usr/bin/env python3
"""Offline lab browser checks using real keyboard and touch inputs only."""
import argparse
from playwright.sync_api import sync_playwright, expect


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--url', default='http://localhost:3031')
    parser.add_argument('--screenshot', default='/tmp/bs-practice.png')
    args = parser.parse_args()
    with sync_playwright() as pw:
        browser = pw.chromium.launch(channel='chrome', headless=True)
        context = browser.new_context(viewport={'width': 1440, 'height': 1000})
        page = context.new_page()
        errors = []
        page.on('pageerror', lambda e: errors.append(str(e)))
        page.goto(args.url)
        page.get_by_role('button', name='연습실', exact=True).click()
        lab = page.locator('#practice-lab')
        start = lab.get_by_role('button', name='시작 / 다시 도전', exact=True)
        status = lab.locator('.practice-status')
        expect(lab).to_be_visible()
        # A non-rotated lock fails and must not create a best record.
        start.click()
        page.keyboard.press('Space')
        expect(status).to_contain_text('도전 종료')
        assert page.evaluate("localStorage.getItem('blockstack.practice.v1.spin')") is None
        start.click()
        page.keyboard.press('ArrowUp')
        page.keyboard.press('Space')
        expect(status).to_have_text('도전 성공!')
        assert int(page.evaluate("localStorage.getItem('blockstack.practice.v1.spin')")) >= 1
        lab.get_by_role('button', name='콤보 연습', exact=True).click()
        start.click()
        for _ in range(4):
            page.keyboard.press('ArrowLeft')
        page.keyboard.press('Space')
        expect(status).to_have_text('플레이 중')
        for _ in range(4):
            page.keyboard.press('ArrowLeft')
        page.keyboard.press('Space')
        expect(status).to_have_text('도전 성공!')
        lab.get_by_role('button', name='공격 상쇄 연습', exact=True).click()
        start.click()
        page.keyboard.press('ArrowUp')
        for _ in range(6):
            page.keyboard.press('ArrowLeft')
        page.keyboard.press('Space')
        expect(status).to_have_text('도전 성공!')
        expect(lab.locator('dd').nth(3)).to_have_text('4 / 0')
        lab.get_by_role('button', name='40줄 스프린트', exact=True).click()
        start.click()
        for _ in range(30):
            page.keyboard.press('Space')
        expect(status).to_contain_text('도전 종료')
        assert page.evaluate("localStorage.getItem('blockstack.practice.v1.sprint')") is None
        lab.get_by_role('button', name='2분 점수 도전', exact=True).click()
        start.click()
        expect(lab.locator('dd').first).not_to_have_text('2:00.00', timeout=3000)
        # Leaving the lab stops its interval; records survive a remount.
        page.get_by_role('button', name='대전 아레나', exact=True).click()
        page.get_by_role('button', name='연습실', exact=True).click()
        expect(lab.locator('dd').last).not_to_have_text('아직 없음')
        page.set_viewport_size({'width': 390, 'height': 844})
        lab.get_by_role('button', name='공격 상쇄 연습', exact=True).click()
        start.click()
        lab.get_by_role('button', name='회전 ↷', exact=True).click()
        # Pointer hold repeatedly moves left and pointer-up releases the action.
        left = lab.get_by_role('button', name='◀ 이동', exact=True)
        left.scroll_into_view_if_needed()
        rect = left.bounding_box()
        page.mouse.move(rect['x'] + rect['width']/2, rect['y'] + rect['height']/2)
        page.mouse.down()
        page.wait_for_timeout(750)
        page.mouse.up()
        lab.get_by_role('button', name='⤓ 고정', exact=True).click()
        expect(status).to_have_text('도전 성공!')
        assert page.evaluate('document.documentElement.scrollWidth <= window.innerWidth')
        page.screenshot(path=args.screenshot, full_page=True)
        assert not errors, errors
        browser.close()
    print('PASS: practice puzzles, failed-record exclusion, timer, persistence, mobile held input')


if __name__ == '__main__':
    main()
