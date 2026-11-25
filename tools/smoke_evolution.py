#!/usr/bin/env python3
"""Result/rematch, private room rules, teams and persistent personal records.
Run against an isolated built server or deployment; creates its own rooms.
"""
import argparse
import re
from playwright.sync_api import sync_playwright, expect

def new_room(page):
    expect(page.locator(".arc-room b")).to_have_text(re.compile(r"[A-Z0-9]{5}"))
    old = page.locator(".arc-room b").inner_text()
    page.get_by_role('button', name='방 찾기', exact=True).click()
    page.get_by_role('button', name='새 방 만들기', exact=True).click()
    expect(page.locator('.arc-room b')).not_to_have_text(old)
    expect(page.locator('#chat-input')).to_be_enabled()
    return page.locator('.arc-room b').inner_text()

def topout(page):
    for _ in range(30):
        page.keyboard.press('Space')
        page.wait_for_timeout(40)

def metric(page):
    return page.locator('.personal-records .record-metrics b').first

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--url', default='http://localhost:3031')
    parser.add_argument('--screenshot', default='/tmp/bs-evolution.png')
    args=parser.parse_args()
    with sync_playwright() as pw:
        browser=pw.chromium.launch(channel='chrome',headless=True)
        ca=browser.new_context(viewport={'width':1512,'height':950})
        cb=browser.new_context(viewport={'width':1512,'height':950})
        a,b=ca.new_page(),cb.new_page()
        errors=[]
        for p in (a,b): p.on('pageerror',lambda e: errors.append(str(e)))
        a.goto(args.url); new_room(a); b.goto(a.url)
        expect(a.locator('.arc-roster-player')).to_have_count(2)
        # Guests explicitly ready before the host starts the first match.
        b.locator('.room-controls .ready-button').click()
        # First result and locally persisted totals.
        a.get_by_role('button',name='준비하고 시작  →',exact=True).click()
        expect(a.locator('.arc-phase')).to_have_text('BATTLE IN PROGRESS',timeout=10000)
        topout(b)
        expect(a.locator('.match-results')).to_be_visible(timeout=10000)
        expect(a.locator('.match-results tbody tr')).to_have_count(2)
        expect(metric(a)).to_have_text('1')
        expect(metric(b)).to_have_text('1')
        # One ready cannot force rematch; both ready automatically start.
        a.locator('.match-results .ready-button').click()
        expect(a.locator('.arc-phase')).to_have_text('MATCH COMPLETE')
        b.locator('.match-results .ready-button').click()
        expect(a.locator('.arc-phase')).to_have_text('BATTLE IN PROGRESS',timeout=10000)
        expect(a.locator('.match-results')).to_be_hidden()
        topout(b)
        expect(a.locator('.match-results')).to_be_visible(timeout=10000)
        expect(metric(a)).to_have_text('2')
        expect(a.locator('.result-delta')).to_contain_text('직전 경기 대비')
        a.get_by_role('button',name='개인 기록',exact=True).click()
        expect(a.locator('.personal-records')).to_be_visible()
        expect(metric(a)).to_have_text('2')
        a.get_by_role('button',name='대전 아레나',exact=True).click()
        # Replayed summary on reload must not duplicate or record a spectator game.
        a.reload();expect(a.locator('#chat-input')).to_be_enabled()
        expect(metric(a)).to_have_text('2')
        expect(a.locator('.match-results')).to_be_visible()
        # New room, explicit private configuration and configurable clock/capacity.
        code=new_room(a)
        controls=a.locator('.room-controls')
        controls.locator('summary').click()
        controls.get_by_label('경기 시간',exact=True).select_option('60')
        controls.get_by_label('최대 인원',exact=True).select_option('4')
        controls.get_by_label('봇 난이도',exact=True).select_option('2')
        controls.get_by_role('button',name='방 설정 적용',exact=True).click()
        expect(a.locator('.match-clock strong')).to_have_text('1:00')
        b.get_by_role('button',name='방 찾기',exact=True).click()
        b.get_by_role('button',name='새로고침',exact=True).click()
        listed = b.locator('table.rooms tr').filter(has=b.get_by_text(code,exact=True))
        expect(listed).to_contain_text('1/4')
        expect(listed).to_contain_text('1분')
        controls.get_by_label('비공개 방',exact=True).check()
        controls.get_by_role('button',name='방 설정 적용',exact=True).click()
        expect(controls.locator('.feature-title')).to_contain_text('비공개')
        expect(a.locator('.match-clock strong')).to_have_text('1:00')
        expect(a.locator('.arc-rivals-area .arc-section-title')).to_contain_text('/ 4 PLAYERS')
        b.get_by_role('button',name='방 찾기',exact=True).click()
        b.get_by_role('button',name='새로고침',exact=True).click()
        expect(b.locator('table.rooms')).not_to_contain_text(code)
        # Invite URL still works, and a guest cannot modify host rules.
        b.goto(a.url);expect(b.locator('#chat-input')).to_be_enabled()
        expect(a.locator('.arc-roster-player')).to_have_count(2)
        expect(b.locator('.room-controls').get_by_label('경기 시간',exact=True)).to_be_disabled()
        controls.get_by_label('대결 방식',exact=True).select_option('teams')
        controls.get_by_role('button',name='방 설정 적용',exact=True).click()
        expect(controls.locator('.team-selector')).to_be_visible()
        # Incomplete teams cannot start; the game stays in the lobby.
        expect(a.get_by_role('button',name='준비하고 시작  →',exact=True)).to_be_disabled()
        expect(a.locator('.arc-phase')).to_have_text('WAITING FOR PLAYERS')
        # Choose opposing teams and fill 2v2 with bots.
        controls.get_by_role('button',name='A팀 선택',exact=True).click()
        b.locator('.room-controls').get_by_role('button',name='B팀 선택',exact=True).click()
        a.get_by_role('button',name='+ 연습봇',exact=True).click()
        expect(a.locator('.arc-roster-player')).to_have_count(3)
        a.get_by_role('button',name='+ 연습봇',exact=True).click()
        expect(a.locator('.arc-roster-player')).to_have_count(4)
        expect(a.get_by_role('button',name='+ 연습봇',exact=True)).to_be_disabled()
        expect(controls.locator('.ready-members')).to_contain_text('A팀')
        expect(controls.locator('.ready-members')).to_contain_text('B팀')
        # Membership and team configuration reset readiness. Ready after all changes.
        b.locator('.room-controls .ready-button').click()
        a.get_by_role('button',name='준비하고 시작  →',exact=True).click()
        expect(a.locator('.arc-phase')).to_have_text('BATTLE IN PROGRESS',timeout=10000)
        expect(a.locator('.match-clock strong')).to_have_text('1:00')
        expect(controls.get_by_label('경기 시간',exact=True)).to_be_disabled()
        a.screenshot(path=args.screenshot,full_page=True)
        # A new observer sees rules/teams but must not gain personal history.
        observer_context=browser.new_context(viewport={'width':390,'height':844},has_touch=True,is_mobile=True)
        observer=observer_context.new_page();observer.goto(a.url)
        # Room is full, so connection rejects the spectator rather than exceeding max capacity.
        expect(observer.locator('.personal-records .record-metrics b').first).to_have_text('0')
        assert not errors, errors
        browser.close()
    print('PASS: results/rematch, prior-record delta, persistent dedup, private listing/invite, host rules, custom clock/capacity/difficulty, 2v2 teams. Screenshot:',args.screenshot)
if __name__=='__main__': main()
