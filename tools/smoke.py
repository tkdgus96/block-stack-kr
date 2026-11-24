#!/usr/bin/env python3
"""Browser smoke test. Requires Python playwright and a built, running server.

Usage: python3 tools/smoke.py --url http://localhost:3017
Uses isolated headless Chrome contexts; does not touch personal browser data.
"""
import argparse
import uuid
import re
from pathlib import Path
from playwright.sync_api import sync_playwright, expect


def room(page):
    return page.locator('.arc-room b').inner_text()


def send(page, message):
    page.locator('#chat-input').fill(message)
    page.locator('.chat-form button').click()
    expect(page.locator('.chat-list')).to_contain_text(message)


def assert_identity(page):
    page.wait_for_function('''() => {
        const field = document.querySelector('.arc-me > .arc-plate .arc-name');
        const mine = document.querySelector('.arc-roster-player:has(.arc-avatar.mine) .arc-roster-name');
        return field && mine && field.textContent.trim() === mine.firstChild.textContent.trim();
    }''')


def assert_responsive(page, width, height):
    page.set_viewport_size({'width': width, 'height': height})
    expect(page.locator('#my-board')).to_be_visible()
    page.wait_for_function('document.documentElement.scrollWidth <= innerWidth + 1')
    board = page.locator('#my-board').bounding_box()
    assert board and board['width'] > 0 and board['x'] >= -1 and board['x'] + board['width'] <= width + 1, f'Board clips at {width}px: {board}'


def new_room(page):
    old = room(page)
    page.get_by_role('button', name='방 찾기', exact=True).click()
    page.get_by_role('button', name='새 방 만들기', exact=True).click()
    expect(page.locator('.arc-room b')).not_to_have_text(old)
    expect(page.locator('#chat-input')).to_be_enabled()
    assert_identity(page)
    return room(page)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--url', default='http://localhost:3017')
    parser.add_argument('--screenshot', default='/tmp/block-stack-smoke.png')
    parser.add_argument('--ignore-https-errors', action='store_true', help='Local TLS fixtures only; omit for production verification')
    args = parser.parse_args()
    token = uuid.uuid4().hex[:10]
    errors = []
    with sync_playwright() as pw:
        browser = pw.chromium.launch(channel='chrome', headless=True)
        contexts = [browser.new_context(viewport={'width': 1600, 'height': 1050}, ignore_https_errors=args.ignore_https_errors) for _ in range(2)]
        a, b = [context.new_page() for context in contexts]
        for page in (a, b):
            page.on('pageerror', lambda error: errors.append(str(error)))
            page.goto(args.url)
            expect(page.locator('body')).to_have_class('theme-arcade')
            expect(page.locator('.arc-room b')).to_have_text(re.compile(r'[A-Z0-9]+'))
            expect(page.locator('#chat-input')).to_be_enabled()
        # Explicitly create our own room to avoid relying on the default room policy.
        code = new_room(a)
        first = f'이전 대화 {token}'
        send(a, first)
        b.get_by_role('button', name='방 찾기', exact=True).click()
        row = b.locator('table.rooms tr').filter(has=b.get_by_text(code, exact=True))
        expect(row).to_contain_text('1/6')
        row.get_by_role('button', name='참가', exact=True).click()
        expect(b.locator('.arc-room b')).to_have_text(code)
        expect(b.locator('.chat-list')).to_contain_text(first)
        assert_identity(b)
        broadcast = f'안녕하세요 {token}'
        send(b, broadcast)
        expect(a.locator('.chat-list')).to_contain_text(broadcast)
        a.locator('#chat-input').fill('   ')
        expect(a.locator('.chat-form button')).to_be_disabled()
        ime = f'조합중 {token}'
        a.locator('#chat-input').fill(ime)
        prevented = a.locator('#chat-input').evaluate('''input => {
            const event = new KeyboardEvent('keydown', {
                key: 'Enter', code: 'Enter', isComposing: true, keyCode: 229,
                bubbles: true, cancelable: true
            });
            input.dispatchEvent(event);
            return event.defaultPrevented;
        }''')
        assert prevented, 'Composing Enter must prevent the default form submission'
        expect(a.locator('#chat-input')).to_have_value(ime)
        expect(b.locator('.chat-list')).not_to_contain_text(ime)
        # Real Enter must still send after composition ends.
        a.locator('#chat-input').press('Enter')
        expect(b.locator('.chat-list')).to_contain_text(ime)
        other = new_room(b)
        assert other != code
        expect(b.locator('.chat-list')).not_to_contain_text(first)
        private = f'다른 방 {token}'
        send(b, private)
        expect(a.locator('.chat-list')).not_to_contain_text(private)
        # Verify direct code join and history replay on returning.
        b.get_by_role('button', name='방 찾기', exact=True).click()
        b.get_by_role('textbox', name='참가할 방 코드').fill(code.lower())
        b.get_by_role('button', name='코드로 참가', exact=True).click()
        expect(b.locator('.arc-room b')).to_have_text(code)
        expect(b.locator('.chat-list')).to_contain_text(first)
        expect(b.locator('.chat-list')).not_to_contain_text(private)
        assert_identity(b)
        # A reconnect replays history once, without appending duplicate messages.
        b.reload()
        expect(b.locator('#chat-input')).to_be_enabled()
        expect(b.locator('.chat-list .chat-text').filter(has_text=first)).to_have_count(1)
        expect(b.locator('.chat-list .chat-text').filter(has_text=broadcast)).to_have_count(1)
        assert_identity(b)
        # Theme changes remount the UI while preserving the room and chat signals.
        b.get_by_role('button', name='▤ 업무 화면', exact=True).click()
        expect(b.locator('body')).to_have_class('theme-sheet')
        expect(b.locator('.roomcode b')).to_have_text(code)
        expect(b.locator('.chat-list')).to_contain_text(first)
        b.get_by_role('button', name='아케이드 모드', exact=True).click()
        expect(b.locator('body')).to_have_class('theme-arcade')
        expect(b.locator('.arc-room b')).to_have_text(code)
        expect(b.locator('.chat-list')).to_contain_text(first)
        a.get_by_role('button', name='+ 연습봇', exact=True).click()
        expect(a.locator('.ready-members')).to_contain_text('연습봇')
        b.locator('.room-controls .ready-button').click()
        a.get_by_role('button', name='준비하고 시작  →', exact=True).click()
        expect(a.locator('.arc-phase')).to_have_text('BATTLE IN PROGRESS', timeout=15000)
        expect(a.locator('#my-board canvas')).to_be_visible()
        assert a.locator('#my-board canvas').evaluate('''canvas => {
            if (!canvas.width || !canvas.height) return false;
            const pixels = canvas.getContext('2d').getImageData(0, 0, canvas.width, canvas.height).data;
            const colors = new Set();
            for (let i = 0; i < pixels.length; i += 4) {
                colors.add(`${pixels[i]},${pixels[i + 1]},${pixels[i + 2]},${pixels[i + 3]}`);
                if (colors.size > 3) return true;
            }
            return false;
        }'''), 'Game board canvas must contain rendered pixels'
        expect(a.locator('#next-0 canvas')).to_be_visible()
        a.locator('#chat-input').focus()
        a.locator('#chat-input').press('Escape')
        a.keyboard.press('Space')
        # Leaving an active match must discard scores, result overlays and previews.
        new_room(a)
        expect(a.locator('.arc-phase')).to_have_text('WAITING FOR PLAYERS')
        expect(a.locator('.arc-stats .arc-big')).to_have_text('0')
        expect(a.locator('.arc-overlay.ready')).to_have_text('READY')
        expect(a.locator('.arc-overlay.ko, .arc-overlay.win, .arc-overlay.lose')).to_have_count(0)
        expect(a.locator('#hold')).to_be_empty()
        for index in range(5):
            expect(a.locator(f'#next-{index}')).to_be_empty()
        # Full room layout: five opponents should fit in at most two rows on desktop.
        for count in range(1, 6):
            a.get_by_role('button', name='+ 연습봇', exact=True).click()
            expect(a.locator('.arc-roster-player')).to_have_count(count + 1)
        assert_responsive(a, 1512, 850)
        tops = a.locator('.arc-opps .arc-opp').evaluate_all('(cards) => cards.map(card => Math.round(card.getBoundingClientRect().top))')
        assert len(tops) == 5 and len(set(tops)) <= 2, f'Five opponents exceed two rows: {tops}'
        a.screenshot(path=args.screenshot, full_page=True)
        assert_responsive(a, 390, 844)
        a.screenshot(path=str(Path(args.screenshot).with_name(Path(args.screenshot).stem + '-mobile.png')), full_page=True)
        assert not errors, f'Browser errors: {errors}'
        browser.close()
    print(f'PASS: room create/list/join, chat broadcast/history/isolation/IME, theme switch, identity, reconnect, match reset, responsive full room, bot start and canvas. Screenshot: {Path(args.screenshot)}')


if __name__ == '__main__':
    main()
