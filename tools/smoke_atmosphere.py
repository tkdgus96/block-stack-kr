#!/usr/bin/env python3
"""Authoritative sound cue deduplication, clock crossing, and reduced-motion checks."""
import argparse
from playwright.sync_api import sync_playwright, expect
from smoke_combat import INIT, event, inject, status, uint

AUDIO = '''(() => {
  localStorage.setItem('blockstack.settings.v1', 'theme=arcade\\nanim=1\\nsound=1\\n');
  window.__tones = [];
  const create = AudioContext.prototype.createOscillator;
  AudioContext.prototype.createOscillator = function(...args) {
    const oscillator = create.apply(this, args);
    window.__tones.push(oscillator);
    return oscillator;
  };
})();'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--url', default='http://localhost:3034')
    args = parser.parse_args()
    with sync_playwright() as pw:
        browser = pw.chromium.launch(channel='chrome', headless=True)
        ca, cb = browser.new_context(), browser.new_context()
        ca.add_init_script(INIT.replace('[19,20,21]', '[13,19,20,21]'))
        ca.add_init_script(AUDIO)
        a, b = ca.new_page(), cb.new_page()
        errors = []
        a.on('pageerror', lambda e: errors.append(str(e)))
        a.goto(args.url)
        a.get_by_role('button', name='방 찾기', exact=True).click()
        a.get_by_role('button', name='새 방 만들기', exact=True).click()
        expect(a.locator('#chat-input')).to_be_enabled()
        b.goto(a.url)
        expect(b.locator('.arc-roster-player')).to_have_count(2)
        b.locator('.room-controls .ready-button').click()
        a.get_by_role('button', name='준비하고 시작  →', exact=True).click()
        expect(a.locator('.arc-phase')).to_have_text('BATTLE IN PROGRESS', timeout=15000)
        a.wait_for_function('window.__combatV2 > 0')
        a.evaluate('window.__combatFixtureMode = true; window.__tones = []')
        glow = a.locator('.arc-atmosphere')
        expect(glow).to_be_visible()
        inject(a, event(1, spin=0, combo=1))
        a.wait_for_function('window.__tones.length === 1')
        low = a.evaluate('window.__tones.at(-1).frequency.value')
        inject(a, event(2, spin=0, combo=7), status([(1, 7, 3), (2, -1, 0)]))
        a.wait_for_function('window.__tones.length === 2')
        high = a.evaluate('window.__tones.at(-1).frequency.value')
        assert high > low
        expect(glow).to_have_class('arc-atmosphere enabled combo')
        inject(a, event(3, spin=2))
        a.wait_for_function('window.__tones.length === 5')
        inject(a, event(3, spin=2))
        a.wait_for_timeout(100)
        assert a.evaluate('window.__tones.length') == 5
        inject(a, [13] + uint(31) + uint(5340) + [1])
        inject(a, [13] + uint(30) + uint(5400) + [1])
        a.wait_for_function('window.__tones.length === 7')
        inject(a, [13] + uint(29) + uint(5460) + [1])
        a.wait_for_timeout(100)
        assert a.evaluate('window.__tones.length') == 7
        a.emulate_media(reduced_motion='reduce')
        expect(glow).to_be_hidden()
        assert not errors, errors
        browser.close()
        print('PASS: combo pitch, T-spin cue, duplicate rejection, final-30s crossing, reduced motion')


if __name__ == '__main__':
    main()
