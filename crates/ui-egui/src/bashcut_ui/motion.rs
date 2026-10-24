//! Light, immediate-mode motion (Agent panel redesign §3.11): no per-widget timers. Upstream's
//! Animations setting (`animation_time`) off means reduce motion: everything snaps to its end state
//! and the pulse is a steady dot.

use std::time::Duration;

/// Seconds an item takes to fade and slide in.
const APPEAR: f32 = 0.18;
/// Seconds a hover takes to blend in.
const HOVER: f32 = 0.08;
/// Repaint rate while something pulses.
const PULSE_FRAME: Duration = Duration::from_millis(33);

/// Motion is on: upstream's Animations setting.
pub fn on(ctx: &egui::Context) -> bool {
    ctx.global_style().animation_time > 0.0
}

/// 0→1 over 0.18 s the first time `id` is seen (1.0 at once with motion off). Use as
/// `ui.multiply_opacity(a)` plus `(1 - a) * 6` px of extra top space.
pub fn appear(ctx: &egui::Context, id: egui::Id) -> f32 {
    if !on(ctx) {
        return 1.0;
    }
    // egui returns the target on the first call, so call with false and then true in the first frame
    ctx.animate_bool_with_time(id, false, APPEAR);
    ctx.animate_bool_with_time(id, true, APPEAR)
}

/// `0.55 + 0.45 * sin` over `period` seconds; asks for a repaint every 33 ms, so call it only while
/// something runs. With motion off: a constant 1.0 and no repaint request.
pub fn pulse(ctx: &egui::Context, period: f32) -> f32 {
    if !on(ctx) || period <= 0.0 {
        return 1.0;
    }
    ctx.request_repaint_after(PULSE_FRAME);
    let time = ctx.input(|i| i.time) as f32;
    0.55 + 0.45 * (time * std::f32::consts::TAU / period).sin()
}

/// 0→1 while `hovered`, over 0.08 s (snaps with motion off): the share of `t.hover` to blend in.
pub fn hover_mix(ctx: &egui::Context, id: egui::Id, hovered: bool) -> f32 {
    if !on(ctx) {
        return f32::from(u8::from(hovered));
    }
    ctx.animate_bool_with_time(id, hovered, HOVER)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run one frame of a fresh context (`animation_time` set first) and give `f` its context.
    fn frame<R>(animation_time: f32, f: impl FnOnce(&egui::Context) -> R) -> R {
        let ctx = egui::Context::default();
        ctx.global_style_mut(|s| s.animation_time = animation_time);
        let mut out = None;
        let mut f = Some(f);
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            if let Some(f) = f.take() {
                out = Some(f(ui.ctx()));
            }
        });
        output.textures_delta.clear();
        out.expect("the frame ran")
    }

    #[test]
    fn appear_and_pulse() {
        let id = egui::Id::new("item");
        // motion on: a new id starts at 0 and has not arrived in its first frame
        assert!(frame(0.18, on));
        let first = frame(0.18, |ctx| appear(ctx, id));
        assert!((0.0..1.0).contains(&first), "first frame {first}");
        let p = frame(0.18, |ctx| pulse(ctx, 1.2));
        assert!((0.1..=1.0).contains(&p), "pulse {p}");
        // motion off (animation_time 0): at the end state at once, a steady dot
        assert!(!frame(0.0, on));
        assert_eq!(frame(0.0, |ctx| appear(ctx, id)), 1.0);
        assert_eq!(frame(0.0, |ctx| pulse(ctx, 1.2)), 1.0);
        assert_eq!(frame(0.0, |ctx| hover_mix(ctx, id, true)), 1.0);
        assert_eq!(frame(0.0, |ctx| hover_mix(ctx, id, false)), 0.0);
        // a period of 0 never divides: steady
        assert_eq!(frame(0.18, |ctx| pulse(ctx, 0.0)), 1.0);
    }
}
