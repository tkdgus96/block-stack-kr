#!/usr/bin/env python3
"""혼자 하는 모드 3단계 확인: 마라톤, Zen, 퍼즐, 키 입력 효율 표시.

Usage: python3 tools/smoke_practice_modes.py --url http://localhost:3000
"""
import argparse
from playwright.sync_api import sync_playwright, expect


def stat(lab, label):
    return lab.locator('dt', has_text=label).locator('xpath=following-sibling::dd[1]')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--url', default='http://localhost:3000')
    args = parser.parse_args()
    with sync_playwright() as pw:
        browser = pw.chromium.launch(channel='chrome', headless=True)
        page = browser.new_context(viewport={'width': 1440, 'height': 1000}).new_page()
        errors = []
        page.on('pageerror', lambda e: errors.append(str(e)))
        page.goto(args.url)
        page.get_by_role('button', name='연습실', exact=True).click()
        lab = page.locator('#practice-lab')
        start = lab.get_by_role('button', name='시작 / 다시 도전', exact=True)
        status = lab.locator('.practice-status')

        # 마라톤: 150줄 목표와 레벨 표시, 넘치면 점수를 기록한다.
        lab.get_by_role('button', name='마라톤', exact=True).click()
        expect(stat(lab, '지운 줄')).to_have_text('0 / 150')
        expect(stat(lab, '레벨')).to_have_text('1')
        start.click()
        for _ in range(40):
            page.keyboard.press('Space')
        expect(status).to_contain_text('도전 종료')
        assert page.evaluate("localStorage.getItem('blockstack.practice.v1.marathon')") is not None

        # Zen: 넘쳐도 계속한다.
        lab.get_by_role('button', name='Zen', exact=True).click()
        start.click()
        for _ in range(40):
            page.keyboard.press('Space')
        expect(status).to_have_text('플레이 중')
        # 키 입력 효율: 왼쪽 벽까지 한 번 누르고 고정하면 실수가 없다.
        page.keyboard.down('ArrowLeft')
        page.wait_for_timeout(400)
        page.keyboard.up('ArrowLeft')
        page.keyboard.press('Space')
        expect(stat(lab, '키 입력 실수')).to_have_text('0 / 41')

        # 퍼즐: 목록에서 고르고 풀면 풀었음 표시가 붙는다.
        lab.get_by_role('button', name='퍼즐', exact=True).click()
        picker = lab.get_by_label('퍼즐 선택')
        expect(picker).to_be_visible()
        assert picker.locator('option').count() == 10
        picker.select_option(label='7. 퍼펙트 클리어 1')
        start.click()
        page.keyboard.press('Space')
        expect(status).to_contain_text('도전 종료')
        start.click()
        for _ in range(4):
            page.keyboard.press('ArrowRight')
        page.keyboard.press('Space')
        expect(status).to_have_text('도전 성공!')
        expect(picker.locator('option').nth(6)).to_have_text('7. 퍼펙트 클리어 1 (풀었음)')

        assert not errors, errors
        browser.close()
    print('PASS: marathon record, zen continues, finesse stats, puzzle picker and solved mark')


if __name__ == '__main__':
    main()
