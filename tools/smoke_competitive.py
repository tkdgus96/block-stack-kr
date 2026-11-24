#!/usr/bin/env python3
"""경쟁 구조 4단계 확인: 빠른 대전 매칭과 봇 대체, 전체 리플레이와 공유 링크, 서버 순위표.

Usage: python3 tools/smoke_competitive.py --url http://localhost:3000
봇 대체 확인 때문에 약 20초 걸린다.
"""
import argparse
import random
from playwright.sync_api import sync_playwright, expect


def open_page(browser, url, name):
    context = browser.new_context(viewport={'width': 1440, 'height': 1000})
    context.add_init_script(f"localStorage.setItem('blockstack.settings.v1', 'theme=arcade\\nname={name}\\nsound=0')")
    page = context.new_page()
    errors = []
    page.on('pageerror', lambda e: errors.append(str(e)))
    page.goto(url)
    expect(page.locator('#chat-input')).to_be_enabled()
    return page, errors


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--url', default='http://localhost:3000')
    args = parser.parse_args()
    with sync_playwright() as pw:
        browser = pw.chromium.launch(channel='chrome', headless=True)
        runner = f'quick-{random.randrange(10**6):06d}'
        a, ea = open_page(browser, args.url, runner)
        b, eb = open_page(browser, args.url, 'quick-b')
        room_a, room_b = a.url, b.url
        assert room_a != room_b

        # 빠른 대전: 두 사람이 누르면 같은 비공개 방에서 바로 시작한다.
        a.get_by_role('button', name='빠른 대전 1:1').first.click()
        expect(a.get_by_role('button', name='상대 찾는 중… (취소)').first).to_be_visible()
        b.get_by_role('button', name='빠른 대전 1:1').first.click()
        expect(a.locator('.arc-phase')).to_have_text('BATTLE IN PROGRESS', timeout=15000)
        expect(b.locator('.arc-phase')).to_have_text('BATTLE IN PROGRESS', timeout=15000)
        a.wait_for_function('location.search.startsWith("?room=")')
        assert a.url == b.url and a.url != room_a, (a.url, b.url)
        expect(a.locator('.arc-roster-player')).to_have_count(2)

        # b 가 먼저 쌓아 지면 경기가 끝나고 전체 리플레이가 생긴다.
        b.locator('#my-board').click()
        for _ in range(40):
            b.keyboard.press('Space')
            b.wait_for_timeout(40)
        expect(a.locator('.arc-phase')).to_have_text('MATCH COMPLETE', timeout=15000)
        a.locator('.arc-nav').get_by_role('button', name='개인 기록', exact=True).click()
        panel = a.locator('.full-replay')
        expect(panel.locator('canvas')).to_have_count(2, timeout=10000)
        expect(panel.locator('figcaption').first).to_contain_text('quick-')
        panel.get_by_label('전체 리플레이 위치').fill('60')
        expect(panel.locator('.replay-controls span')).to_have_text('0:01')
        replay_id = a.evaluate("document.querySelector('.full-replay-status').textContent.split(' · ').pop()")
        assert len(replay_id) == 16, replay_id

        # 공유 링크로 들어오면 기록 화면에서 같은 리플레이를 연다.
        c, ec = open_page(browser, f'{args.url}/?replay={replay_id}', 'viewer')
        expect(c.locator('.full-replay canvas')).to_have_count(2, timeout=10000)
        expect(c.locator('.full-replay-status')).to_contain_text(replay_id)

        # 혼자 기다리면 15초 뒤 봇과 붙는다.
        c.locator('.arc-nav').get_by_role('button', name='대전 아레나', exact=True).click()
        c.get_by_role('button', name='빠른 대전 1:1').first.click()
        expect(c.locator('.arc-phase')).to_have_text('BATTLE IN PROGRESS', timeout=25000)
        expect(c.locator('.arc-roster-player')).to_have_count(2)
        expect(c.locator('.arc-roster-player').filter(has_text='빠른대전 봇')).to_have_count(1)

        # 순위표: 마라톤은 블록이 넘쳐도 점수를 기록하고, 서버가 다시 실행해 확인한 뒤 올린다.
        a.locator('.arc-nav').get_by_role('button', name='연습실', exact=True).click()
        lab = a.locator('#practice-lab')
        lab.get_by_role('button', name='마라톤', exact=True).click()
        expect(lab.locator('.practice-ranks')).to_be_visible()
        lab.get_by_role('button', name='시작 / 다시 도전', exact=True).click()
        for _ in range(40):
            a.keyboard.press('Space')
        expect(lab.locator('.practice-status')).to_contain_text('도전 종료')
        expect(lab.locator('.practice-ranks [role=status]')).to_have_text('서버가 기록을 확인하고 순위표에 반영했습니다.', timeout=10000)
        # 서버 데이터가 남아 있으면 다른 기록이 먼저 있을 수 있으므로 목록 안에 있는지만 본다.
        expect(lab.locator('.practice-ranks li').filter(has_text=runner)).to_have_count(1)

        errors = ea + eb + ec
        assert not errors, errors
        browser.close()
    print('PASS: quick match pairing, bot fallback, full replay + share link, verified leaderboard')


if __name__ == '__main__':
    main()
