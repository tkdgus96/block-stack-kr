#!/usr/bin/env python3
"""Anonymous server profile creation/recovery and API authority smoke.
Run against an isolated game server. Never prints bearer recovery codes.
"""
import argparse
from playwright.sync_api import sync_playwright, expect


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--url', default='http://localhost:3031')
    args = parser.parse_args()
    with sync_playwright() as pw:
        browser = pw.chromium.launch(channel='chrome', headless=True)
        a_ctx, b_ctx = browser.new_context(), browser.new_context()
        a, b = a_ctx.new_page(), b_ctx.new_page()
        errors = []
        for p in (a, b):
            p.on('pageerror', lambda e: errors.append(str(e)))
            p.goto(args.url)
            p.get_by_role('button', name='개인 기록', exact=True).click()
            expect(p.locator('.profile-status')).to_contain_text('서버 저장 완료', timeout=15000)
            expect(p.locator('.profile-totals')).to_contain_text('0경기')
            expect(p.locator('.profile-secret')).to_have_count(0)
        token = a.evaluate("localStorage.getItem('bs-profile-token-v1')")
        assert len(token) == 64
        # No missing/short bearer may read a profile. A POST cannot forge results.
        endpoint = a.evaluate("""async () => {
            const token=localStorage.getItem('bs-profile-token-v1');
            const urls=performance.getEntriesByType('resource').map(e=>e.name);
            const url=urls.find(u=>u.endsWith('/api/profile'));
            const noAuth=await fetch(url);
            const forged=await fetch(url,{method:'POST',headers:{Authorization:'Bearer '+token},body:JSON.stringify({rounds:999,wins:999,sent:999})});
            return {status:noAuth.status,forged:await forged.json(),url};
        }""")
        assert endpoint['status'] == 401
        assert endpoint['forged']['rounds'] == endpoint['forged']['wins'] == 0
        # Cross-device recovery requires deliberate entry; bad code cannot replace identity.
        b.locator('.profile-recovery summary').click()
        b.get_by_placeholder('64자리 복구 코드').fill('wrong')
        b.get_by_role('button', name='이 프로필로 전환', exact=True).click()
        expect(b.locator('.profile-status')).to_contain_text('64자리')
        b.get_by_placeholder('64자리 복구 코드').fill(token)
        b.get_by_role('button', name='이 프로필로 전환', exact=True).click()
        expect(b.locator('.profile-status')).to_contain_text('서버 저장 완료')
        assert b.evaluate("localStorage.getItem('bs-profile-token-v1')") == token
        b.reload()
        b.get_by_role('button', name='개인 기록', exact=True).click()
        expect(b.locator('.profile-status')).to_contain_text('서버 저장 완료')
        assert b.evaluate("localStorage.getItem('bs-profile-token-v1')") == token
        expect(b.locator('.profile-themes button').nth(1)).to_be_disabled()
        # A real authoritative two-player result updates progression, not browser input.
        a.get_by_role('button', name='방 찾기', exact=True).click()
        a.get_by_role('button', name='새 방 만들기', exact=True).click()
        expect(a.locator('#chat-input')).to_be_enabled()
        rival_ctx = browser.new_context()
        rival = rival_ctx.new_page()
        rival.on('pageerror', lambda e: errors.append(str(e)))
        rival.goto(a.url)
        expect(rival.locator('.profile-status')).to_contain_text('서버 저장 완료', timeout=15000)
        expect(a.locator('.room-controls .arc-roster-player')).to_have_count(2)
        rival.locator('.room-controls .ready-button').click()
        a.get_by_role('button', name='준비하고 시작  →', exact=True).click()
        expect(a.locator('.arc-phase')).to_have_text('BATTLE IN PROGRESS', timeout=10000)
        rival_token = rival.evaluate("localStorage.getItem('bs-profile-token-v1')")
        a.get_by_role('button', name='개인 기록', exact=True).click()
        a.locator('.profile-recovery summary').click()
        a.get_by_placeholder('64자리 복구 코드').fill(rival_token)
        a.get_by_role('button', name='이 프로필로 전환', exact=True).click()
        expect(a.locator('.profile-status')).to_contain_text('경기 중에는 프로필을 바꿀 수 없습니다')
        assert a.evaluate("localStorage.getItem('bs-profile-token-v1')") == token
        a.get_by_role('button', name='대전 아레나', exact=True).click()
        for _ in range(35):
            rival.keyboard.press('Space')
            rival.wait_for_timeout(50)
        expect(a.locator('.match-results')).to_be_visible(timeout=10000)
        a.get_by_role('button', name='개인 기록', exact=True).click()
        expect(a.locator('.profile-totals')).to_contain_text('1경기 · 1승', timeout=10000)
        expect(a.locator('.mission-list')).to_contain_text('대전 1승 1/1')
        expect(a.locator('.profile-themes button').nth(1)).to_be_enabled()
        a.locator('.profile-themes button').nth(1).click()
        expect(a.locator('body')).to_have_attribute('data-profile-theme', 'ocean')
        # Recovered device reads the same durable result and newly unlocked reward.
        b.get_by_role('button', name='전적 새로고침', exact=True).click()
        expect(b.locator('.profile-totals')).to_contain_text('1경기 · 1승', timeout=10000)
        expect(b.locator('.profile-themes button').nth(1)).to_be_enabled()
        a.reload()
        a.get_by_role('button', name='개인 기록', exact=True).click()
        expect(a.locator('.profile-totals')).to_contain_text('1경기 · 1승', timeout=10000)
        expect(a.locator('body')).to_have_attribute('data-profile-theme', 'ocean')
        # A failed write must keep the imported secret in memory ahead of stale storage.
        b.evaluate("""() => { const original=Storage.prototype.setItem;
            Storage.prototype.setItem=function(key,value){if(key==='bs-profile-token-v1')throw new DOMException('blocked','SecurityError');return original.call(this,key,value);}; }""")
        b.locator('.profile-recovery summary').click()
        b.get_by_placeholder('64자리 복구 코드').fill(rival_token)
        b.get_by_role('button', name='이 프로필로 전환', exact=True).click()
        expect(b.locator('.profile-status')).to_contain_text('브라우저 저장이 차단')
        assert b.evaluate("localStorage.getItem('bs-profile-token-v1')") == token
        b.get_by_role('button', name='내 복구 코드 표시', exact=True).click()
        expect(b.locator('.profile-secret')).to_have_text(rival_token)
        # The same fallback makes a newly created secret recoverable with storage blocked.
        blocked_ctx = browser.new_context()
        blocked_ctx.add_init_script("""const original=Storage.prototype.setItem;
            Storage.prototype.setItem=function(key,value){if(key==='bs-profile-token-v1')throw new DOMException('blocked','SecurityError');return original.call(this,key,value);};""")
        blocked = blocked_ctx.new_page()
        blocked.on('pageerror', lambda e: errors.append(str(e)))
        blocked.goto(args.url)
        blocked.get_by_role('button', name='개인 기록', exact=True).click()
        expect(blocked.locator('.profile-status')).to_contain_text('브라우저 저장이 차단', timeout=15000)
        blocked.locator('.profile-recovery summary').click()
        blocked.get_by_role('button', name='내 복구 코드 표시', exact=True).click()
        expect(blocked.locator('.profile-secret')).to_have_text(__import__('re').compile(r'^[0-9a-f]{64}$'))
        assert not errors, errors
        browser.close()
    print('PASS profile auth, forged-stat rejection, recovery, authoritative battle progression, mission reward, theme, blocked-storage recovery and in-match import guard')


if __name__ == '__main__':
    main()
