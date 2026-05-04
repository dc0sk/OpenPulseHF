/// Plasma colormap: maps normalised intensity 0–255 to RGBA.
///
/// Approximated from the matplotlib plasma palette using 8 control points
/// with linear interpolation.
pub fn plasma(t: u8) -> egui::Color32 {
    const STOPS: &[(f32, f32, f32, f32)] = &[
        (0.000, 0.050, 0.030, 0.527), // dark blue-violet
        (0.143, 0.459, 0.017, 0.655), // purple
        (0.286, 0.679, 0.008, 0.736), // violet
        (0.429, 0.839, 0.152, 0.706), // pink-magenta
        (0.571, 0.953, 0.325, 0.592), // warm pink
        (0.714, 0.989, 0.553, 0.349), // salmon-orange
        (0.857, 0.992, 0.761, 0.141), // amber
        (1.000, 0.940, 0.975, 0.131), // yellow
    ];
    let v = t as f32 / 255.0;
    let i = ((v * (STOPS.len() - 1) as f32) as usize).min(STOPS.len() - 2);
    let t0 = &STOPS[i];
    let t1 = &STOPS[i + 1];
    let frac = (v - t0.0) / (t1.0 - t0.0).max(1e-9);
    let r = lerp(t0.1, t1.1, frac);
    let g = lerp(t0.2, t1.2, frac);
    let b = lerp(t0.3, t1.3, frac);
    egui::Color32::from_rgb((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0.0, 1.0)
}
