#!/usr/bin/env python3
"""Optional board gestures: observe actual outgoing inputs without app test hooks."""
import argparse
import re
from playwright.sync_api import sync_playwright, expect
from smoke_battle import SPY, count, touch


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--url',default='http://localhost:3031')
    args=parser.parse_args()
    with sync_playwright() as pw:
        browser=pw.chromium.launch(channel='chrome',headless=True)
        context=browser.new_context(viewport={'width':390,'height':844},is_mobile=True,has_touch=True)
        context.add_init_script(SPY)
        page=context.new_page()
        errors=[]
        page.on('pageerror',lambda e:errors.append(str(e)))
        page.goto(args.url)
        expect(page.locator('#chat-input')).to_be_enabled()
        page.locator('.arc-nav').get_by_role('button',name='설정',exact=True).click()
        checkbox=page.get_by_role('checkbox',name='보드 제스처 조작',exact=True)
        expect(checkbox).not_to_be_checked()
        checkbox.check()
        page.reload()
        page.locator('.arc-nav').get_by_role('button',name='설정',exact=True).click()
        expect(checkbox).to_be_checked()
        page.locator('.arc-nav').get_by_role('button',name='방 찾기',exact=True).click()
        page.get_by_role('button',name='새 방 만들기',exact=True).click()
        expect(page.locator('#chat-input')).to_be_enabled()
        board=page.locator('#my-board')
        # Gestures are disabled in the lobby despite the opted-in preference.
        board.click()
        assert count(page,2)==0
        page.get_by_role('button',name=re.compile('(?:준비하고|게임) 시작')).click()
        expect(page.locator('.arc-phase')).to_have_text('BATTLE IN PROGRESS',timeout=15000)
        expect(board).to_have_class(re.compile('gesture-active'))
        board.scroll_into_view_if_needed()
        rect=board.bounding_box()
        x,y=rect['x']+rect['width']/2,rect['y']+rect['height']/2
        client=context.new_cdp_session(page)
        def point(dx=0,dy=0,identity=1):
            return {'x':x+dx,'y':y+dy,'id':identity,'radiusX':2,'radiusY':2}
        before=count(page,2)
        touch(client,'touchStart',[point()]);touch(client,'touchEnd')
        page.wait_for_timeout(100)
        assert count(page,2)==before+1
        before=count(page,0)
        touch(client,'touchStart',[point()]);touch(client,'touchMove',[point(-rect['width']*.3)]);touch(client,'touchEnd')
        page.wait_for_timeout(100)
        assert count(page,0)>=before+2
        # Small downward jitter neither rotates nor accidentally harddrops.
        before=count(page,5)
        touch(client,'touchStart',[point()]);touch(client,'touchMove',[point(0,20)]);touch(client,'touchEnd')
        page.wait_for_timeout(100)
        assert count(page,5)==before
        touch(client,'touchStart',[point()]);touch(client,'touchMove',[point(0,90)]);touch(client,'touchEnd')
        page.wait_for_timeout(100)
        assert count(page,5)==before+1
        # Cancelled swipes and multitouch do not drop on release.
        before=count(page,5)
        touch(client,'touchStart',[point()]);touch(client,'touchMove',[point(0,90)]);touch(client,'touchCancel')
        touch(client,'touchStart',[point()]);touch(client,'touchStart',[point(),point(30,0,2)])
        touch(client,'touchEnd')
        page.wait_for_timeout(100)
        assert count(page,5)==before
        assert not errors,errors
        browser.close()
    print('PASS: opt-in persistence, lobby gate, tap, drag, drop threshold, cancellation, multitouch')


if __name__=='__main__':main()
