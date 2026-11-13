//! 선택적 효과음. 기본값은 OFF 이며, 켜져 있을 때만 AudioContext 를 만든다.
//! 짧은 사인파 한 번만 재생하고 지속적인 오디오 루프는 없다.

use std::cell::RefCell;
use web_sys::{AudioContext, OscillatorType};

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
    CTX.with(|c| {
        let mut c = c.borrow_mut();
        if c.is_none() {
            *c = AudioContext::new().ok();
        }
        let Some(ctx) = c.as_ref() else { return };
        let (Ok(osc), Ok(gain)) = (ctx.create_oscillator(), ctx.create_gain()) else { return };
        osc.set_type(OscillatorType::Sine);
        osc.frequency().set_value(freq);
        let t = ctx.current_time();
        let g = gain.gain();
        let _ = g.set_value_at_time(0.04, t);
        let _ = g.linear_ramp_to_value_at_time(0.0, t + ms / 1000.0);
        let _ = osc.connect_with_audio_node(&gain);
        let _ = gain.connect_with_audio_node(&ctx.destination());
        let _ = osc.start();
        let _ = osc.stop_with_when(t + ms / 1000.0 + 0.01);
    });
}
