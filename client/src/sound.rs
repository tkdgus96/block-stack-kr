//! 선택적 효과음. 기본값은 OFF 이며, 켜져 있을 때만 AudioContext 를 만든다.
//! 짧은 합성음을 재생하고 지속적인 오디오 루프는 없다.

use std::cell::RefCell;
use web_sys::{AudioContext, OscillatorType};
use bs_engine::{game::SpinKind, protocol::CombatEvent};

thread_local! {
    static CTX: RefCell<Option<AudioContext>> = const { RefCell::new(None) };
}

#[derive(Clone, Copy)]
pub enum Sfx {
    Lock,
    Clear,
    Garbage,
    GameOver,
}

pub fn play(sfx: Sfx) {
    let (freq, ms) = match sfx {
        Sfx::Lock => (220.0, 25.0),
        Sfx::Clear => (440.0, 70.0),
        Sfx::Garbage => (150.0, 60.0),
        Sfx::GameOver => (262.0, 180.0),
    };
    tone(freq, ms, 0.0);
}

fn tone(freq: f32, ms: f64, delay: f64) {
    CTX.with(|c| {
        let mut c = c.borrow_mut();
        if c.is_none() {
            *c = AudioContext::new().ok();
        }
        let Some(ctx) = c.as_ref() else { return };
        let (Ok(osc), Ok(gain)) = (ctx.create_oscillator(), ctx.create_gain()) else { return };
        osc.set_type(OscillatorType::Sine);
        osc.frequency().set_value(freq);
        let _ = ctx.resume();
        let t = ctx.current_time() + delay;
        let g = gain.gain();
        let _ = g.set_value_at_time(0.04, t);
        let _ = g.linear_ramp_to_value_at_time(0.0, t + ms / 1000.0);
        let _ = osc.connect_with_audio_node(&gain);
        let _ = gain.connect_with_audio_node(&ctx.destination());
        let _ = osc.start_with_when(t);
        let _ = osc.stop_with_when(t + ms / 1000.0 + 0.01);
    });
}

/// Extra cue only for a technique/chain; ordinary predicted lock/clear stays unchanged.
pub(crate) fn combat(event: &CombatEvent) {
    if event.spin != SpinKind::None {
        tone(523.25, 80.0, 0.0);
        tone(783.99, 110.0, 0.065);
        if event.spin == SpinKind::Full { tone(1046.5, 110.0, 0.13); }
    } else if event.combo >= 1 && event.cleared > 0 {
        // Semitone steps keep long combos musical while bounding shrill frequencies.
        let semitones = (event.combo as u8).min(12) as f32;
        tone(440.0 * 2.0_f32.powf(semitones / 12.0), 90.0, 0.0);
    } else if event.perfect_clear {
        tone(659.25, 100.0, 0.0);
        tone(987.77, 130.0, 0.08);
    }
}

pub(crate) fn final_seconds() {
    tone(659.25, 75.0, 0.0);
    tone(659.25, 75.0, 0.16);
}

pub(crate) fn danger() { tone(130.81, 130.0, 0.0); }
