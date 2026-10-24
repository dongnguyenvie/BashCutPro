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
    #[test]
    #[ignore = "test ticket: appear goes 0 to 1 and is 1.0 at once with animation_time 0; pulse is 1.0 with motion off"]
    fn appear_and_pulse() {}
}
