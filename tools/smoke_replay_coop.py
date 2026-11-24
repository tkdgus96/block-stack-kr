#!/usr/bin/env python3
"""Actual-game replay persistence and cooperative survival, without injected state."""
import argparse
from playwright.sync_api import sync_playwright, expect


def room(page):
    page.get_by_role('button', name='방 찾기', exact=True).click()
    old = page.url
    page.get_by_role('button', name='새 방 만들기', exact=True).click()
    page.wait_for_url(lambda url: url != old)
    expect(page.locator('#chat-input')).to_be_enabled()


def topout(page):
    # Focus the game rather than a control; browser keyboard events use real inputs.
    page.locator('.arc-section-title').first.click()
    for _ in range(28):
        page.keyboard.press('Space')
        page.wait_for_timeout(40)


def totals(page):
    return page.evaluate("JSON.parse(localStorage.getItem('blockstack.records.v1') || '{}').rounds || 0")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--url', default='http://localhost:3031')
    parser.add_argument('--screenshot', default='/tmp/bs-replay-coop.png')
    args = parser.parse_args()
    with sync_playwright() as pw:
        browser = pw.chromium.launch(channel='chrome', headless=True)
        ca, cb = browser.new_context(), browser.new_context()
        a, b = ca.new_page(), cb.new_page()
        errors = []
        for page in (a, b):
            page.on('pageerror', lambda error: errors.append(str(error)))
        a.goto(args.url)
        room(a)
        b.goto(a.url)
        expect(b.locator('.arc-roster-player')).to_have_count(2)
        b.locator('.room-controls .ready-button').click()
        a.get_by_role('button', name='준비하고 시작  →', exact=True).click()
        expect(a.locator('.arc-phase')).to_have_text('BATTLE IN PROGRESS', timeout=10000)
        # Let a real authoritative history exceed the 15-second recording window.
        a.wait_for_timeout(16500)
        topout(a)
        expect(a.locator('.match-results')).to_be_visible(timeout=10000)
        assert totals(a) == 1 and totals(b) == 1
        clip = a.evaluate("JSON.parse(localStorage.getItem('bs-last-replay'))")
        assert 2 <= len(clip['frames']) <= 152, len(clip['frames'])
        first, last = clip['frames'][0]['tick'], clip['frames'][-1]['tick']
        assert first + 900 >= last and last > 900, (first, last)
        assert all(len(frame['cells']) == 200 for frame in clip['frames'])
        assert clip['events'], 'Real hard drops should produce authoritative lock events'
        a.get_by_role('button', name='개인 기록', exact=True).click()
        panel = a.locator('.replay-panel')
        expect(panel.locator('.review-board i')).to_have_count(200)
        slider = panel.get_by_label('리플레이 위치')
        slider.fill(slider.get_attribute('max'))
        end = panel.locator('.replay-controls > span').inner_text()
        slider.fill('0')
        start = panel.locator('.replay-controls > span').inner_text()
        assert start != end
        panel.get_by_role('button', name='재생', exact=True).click()
        expect(panel.get_by_role('button', name='일시 정지', exact=True)).to_be_visible()
        a.wait_for_timeout(1200)
        assert panel.locator('.replay-controls > span').inner_text() != start
        a.reload()
        expect(a.locator('#chat-input')).to_be_enabled()
        a.get_by_role('button', name='개인 기록', exact=True).click()
        expect(a.locator('.replay-panel .review-board')).to_be_visible()
        assert a.evaluate("JSON.parse(localStorage.getItem('bs-last-replay')).frames.length") == len(clip['frames'])
        a.get_by_role('button', name='대전 아레나', exact=True).click()
        room(a)
        a.get_by_label('협동 생존전 사용').check()
        b.goto(a.url)
        expect(b.get_by_label('협동 생존전 사용')).to_be_checked()
        expect(b.get_by_label('협동 생존전 사용')).to_be_disabled()
        expect(b.locator('.coop-panel')).to_contain_text('공동 공격 0 / 80줄')
        counts = (totals(a), totals(b))
        b.locator('.room-controls .ready-button').click()
        a.get_by_role('button', name='준비하고 시작  →', exact=True).click()
        expect(a.locator('.arc-phase')).to_have_text('BATTLE IN PROGRESS', timeout=10000)
        expect(a.locator('.arc-opp.targeted')).to_have_count(0)
        expect(b.locator('.arc-opp.targeted')).to_have_count(0)
        topout(a)
        expect(a.locator('.room-controls .is-me .lobby-status')).to_have_text('탈락')
        expect(a.locator('.arc-phase')).to_have_text('BATTLE IN PROGRESS')
        expect(a.locator('.match-results')).to_be_hidden()
        topout(b)
        expect(a.locator('.match-results')).to_be_visible(timeout=10000)
        expect(a.locator('.coop-panel')).to_contain_text('도전 종료')
        expect(a.locator('.result-headline')).to_have_text('협동 도전 종료')
        assert (totals(a), totals(b)) == counts, 'Coop must not increment battle records'
        assert a.evaluate("JSON.parse(localStorage.getItem('bs-last-replay')).frames.length") == len(clip['frames'])
        a.screenshot(path=args.screenshot, full_page=True)
        assert not errors, errors
        ca.close()
        cb.close()
        print('PASS: real replay bounds, scrub, playback, reload; coop snapshot, no targets, survivor continuation, shared failure, records excluded', flush=True)
        browser.close()


if __name__ == '__main__':
    main()
