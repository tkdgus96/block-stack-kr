#!/usr/bin/env python3
"""Check unambiguous identities, readiness, host transfer and mobile layout in isolated rooms."""
import argparse
import re
from playwright.sync_api import sync_playwright, expect


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--url', default='http://localhost:3034')
    parser.add_argument('--screenshot', default='/tmp/bs-lobby.png')
    args = parser.parse_args()
    with sync_playwright() as pw:
        browser = pw.chromium.launch(channel='chrome', headless=True)
        contexts = [browser.new_context(viewport={'width': 1512, 'height': 950}) for _ in range(2)]
        a, b = [c.new_page() for c in contexts]
        errors = []
        for page in (a, b):
            page.on('pageerror', lambda error: errors.append(str(error)))
        a.goto(args.url)
        expect(a.locator('.arc-room b')).to_have_text(re.compile(r'[A-Z0-9]{5}'))
        old = a.locator('.arc-room b').inner_text()
        a.get_by_role('button', name='방 찾기', exact=True).click()
        a.get_by_role('button', name='새 방 만들기', exact=True).click()
        expect(a.locator('.arc-room b')).not_to_have_text(old)
        expect(a.locator('#chat-input')).to_be_enabled()
        b.goto(a.url)
        controls = [p.locator('.room-controls') for p in (a, b)]
        for panel in controls:
            expect(panel.locator('.arc-roster-player')).to_have_count(2)
            expect(panel.locator('.arc-roster-player.is-me')).to_have_count(1)
            expect(panel.locator('.lobby-role-tags .lobby-host')).to_have_count(1)
            expect(panel.locator('.lobby-status.waiting')).to_have_count(2)
            expect(panel.locator('.lobby-roster-heading')).to_contain_text('준비 0 / 2명')
        # Unique room-local identifiers are shown consistently to both clients.
        a_id = controls[0].locator('.is-me .arc-avatar').inner_text()
        b_id = controls[1].locator('.is-me .arc-avatar').inner_text()
        assert a_id != b_id
        expect(controls[0].locator('.lobby-identity')).to_contain_text(a_id)
        expect(controls[1].locator('.lobby-identity')).to_contain_text(b_id)
        start = a.get_by_role('button', name='준비하고 시작  →', exact=True)
        expect(start).to_be_disabled()
        controls[1].locator('.ready-button').click()
        expect(controls[0].locator('.lobby-roster-heading')).to_contain_text('준비 1 / 2명')
        expect(controls[0].locator('.lobby-status.ready')).to_have_count(1)
        expect(start).to_be_enabled()
        # Cancelling readiness prevents the host from starting.
        controls[1].locator('.ready-button').click()
        expect(start).to_be_disabled()
        controls[1].locator('.ready-button').click()
        start.click()
        expect(a.locator('.arc-phase')).to_have_text('BATTLE IN PROGRESS', timeout=10000)
        expect(controls[0].locator('.lobby-status')).to_have_text(['경기 중', '경기 중'])
        expect(controls[0].locator('.lobby-status.ready')).to_have_count(0)
        # Host departure is reflected immediately in the remaining client's identity.
        contexts[0].close()
        expect(controls[1].locator('.lobby-identity .lobby-host')).to_be_visible()
        b.set_viewport_size({'width': 390, 'height': 844})
        expect(controls[1].locator('.is-me')).to_be_visible()
        assert b.evaluate('document.documentElement.scrollWidth <= innerWidth + 1')
        b.screenshot(path=args.screenshot, full_page=True)
        assert not errors, errors
        browser.close()
        print('PASS: lobby identity, unique tags, readiness, start gating, phase state, host transfer, mobile')


if __name__ == '__main__':
    main()
