//! Audio Visualizer - Classic Windows Media Player style

#[cfg(windows)]
mod capture_windows;

#[cfg(target_os = "macos")]
mod capture_macos_sck;

use macroquad::prelude::*;
#[cfg(not(windows))]
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
#[cfg(not(windows))]
use cpal::{FromSample, Sample};
use rustfft::num_complex::Complex;
use rustfft::{Fft, FftPlanner};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
#[cfg(not(windows))]
use std::time::Duration;

// Configuration
const FFT_SIZE: usize = 2048;
const BAR_COUNT: usize = 64;

/// Discrete speed levels: ... ccw, 0 (neutral), cw ... Tap Up/Down to step.
/// 21 levels: -90° to +90° in 9° increments
const SPEED_LEVELS: &[f32] = &[
    -std::f32::consts::FRAC_PI_2,       // -90°/s
    -std::f32::consts::FRAC_PI_2 * 0.9,
    -std::f32::consts::FRAC_PI_2 * 0.8,
    -std::f32::consts::FRAC_PI_2 * 0.7,
    -std::f32::consts::FRAC_PI_2 * 0.6,
    -std::f32::consts::FRAC_PI_2 * 0.5,
    -std::f32::consts::FRAC_PI_2 * 0.4,
    -std::f32::consts::FRAC_PI_2 * 0.3,
    -std::f32::consts::FRAC_PI_2 * 0.2,
    -std::f32::consts::FRAC_PI_2 * 0.1,
    0.0,                                 // neutral
    std::f32::consts::FRAC_PI_2 * 0.1,
    std::f32::consts::FRAC_PI_2 * 0.2,
    std::f32::consts::FRAC_PI_2 * 0.3,
    std::f32::consts::FRAC_PI_2 * 0.4,
    std::f32::consts::FRAC_PI_2 * 0.5,
    std::f32::consts::FRAC_PI_2 * 0.6,
    std::f32::consts::FRAC_PI_2 * 0.7,
    std::f32::consts::FRAC_PI_2 * 0.8,
    std::f32::consts::FRAC_PI_2 * 0.9,
    std::f32::consts::FRAC_PI_2,        // 90°/s
];

fn next_speed(current: f32, step: i32) -> f32 {
    let idx = SPEED_LEVELS
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| (*a - current).abs().partial_cmp(&(*b - current).abs()).unwrap())
        .map(|(i, _)| i)
        .unwrap_or(2);
    let new_idx = (idx as i32 + step).clamp(0, SPEED_LEVELS.len() as i32 - 1);
    SPEED_LEVELS[new_idx as usize]
}

fn hsv_to_color(h: f32, s: f32, v: f32) -> Color {
    let h = ((h % 360.0) + 360.0) % 360.0;
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (r, g, b) = if h < 60.0 {
        (c, x, 0.0)
    } else if h < 120.0 {
        (x, c, 0.0)
    } else if h < 180.0 {
        (0.0, c, x)
    } else if h < 240.0 {
        (0.0, x, c)
    } else if h < 300.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    Color::new(r + m, g + m, b + m, 1.0)
}

// Color palette (Windows classic vibe)
const COLORS: [Color; 8] = [
    Color::new(0.2, 0.8, 0.2, 1.0),  // Green
    Color::new(0.4, 0.8, 0.2, 1.0),
    Color::new(0.6, 0.8, 0.2, 1.0),
    Color::new(0.8, 0.8, 0.2, 1.0),  // Yellow
    Color::new(0.8, 0.6, 0.2, 1.0),
    Color::new(0.8, 0.4, 0.2, 1.0),  // Orange
    Color::new(0.8, 0.2, 0.2, 1.0),  // Red
    Color::new(0.8, 0.1, 0.5, 1.0),  // Purple
];

struct Projectile {
    x: f32,
    y: f32,
    dx: f32,
    dy: f32,
    hue: f32,
    size: f32,
    trail: Vec<(f32, f32)>,
    birth_time: f32,
}

struct VisualizerState {
    bar_heights: [f32; BAR_COUNT],
    peak_heights: [f32; BAR_COUNT],
    peak_fired: Vec<usize>,
    fire_cooldown: [u8; BAR_COUNT],
    fft_input: Vec<Complex<f32>>,
    fft: Arc<dyn Fft<f32>>,
    peak_magnitude: f32,
}

impl VisualizerState {
    fn new() -> Self {
        let mut planner = FftPlanner::new();
        let fft = planner.plan_fft_forward(FFT_SIZE);
        Self {
            bar_heights: [0.0; BAR_COUNT],
            peak_heights: [0.0; BAR_COUNT],
            peak_fired: Vec::new(),
            fire_cooldown: [0; BAR_COUNT],
            fft_input: vec![Complex::new(0.0, 0.0); FFT_SIZE],
            fft,
            peak_magnitude: 0.01,
        }
    }

    fn update(&mut self, audio_data: &[f32]) {
        if audio_data.len() < FFT_SIZE {
            return;
        }
        let data = &audio_data[..FFT_SIZE];

        for (i, &s) in data.iter().enumerate() {
            let window = 0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / (FFT_SIZE - 1) as f32).cos());
            self.fft_input[i] = Complex::new(s * window, 0.0);
        }

        self.fft.process(&mut self.fft_input);

        const SAMPLE_RATE: f32 = 44100.0;
        let bins = FFT_SIZE / 2;
        let nyquist = SAMPLE_RATE / 2.0;
        let freq_per_bin = SAMPLE_RATE / FFT_SIZE as f32;

        // Vocal-centric: mids dominate the circle, extremes (bass/highs) at edges.
        // Bass+Low 20-500Hz (10) | Mids 500-3.5kHz (44) | High 3.5k-10kHz (10)
        const BASS_LOW_BARS: usize = 10;
        const MIDS_BARS: usize = 44;
        const HIGH_BARS: usize = 10;
        let (f_bass_low_lo, f_bass_low_hi) = (20.0, 500.0);
        let (f_mids_lo, f_mids_hi) = (500.0, 3500.0);
        let (f_high_lo, f_high_hi) = (3500.0, 15000.0_f32.min(nyquist));

        // Use max only from 800 Hz+ so bass doesn't crush gain for percussive highs
        let bass_cutoff_bin = (800.0 / freq_per_bin) as usize;
        let max_mag: f32 = self.fft_input[bass_cutoff_bin.min(bins)..bins]
            .iter()
            .map(|c: &Complex<f32>| c.norm())
            .fold(0.0f32, f32::max);
        self.peak_magnitude = self.peak_magnitude * 0.995 + max_mag * 0.005;
        let gain = if self.peak_magnitude > 0.0001 {
            0.21 / self.peak_magnitude
        } else {
            500.0
        };

        for i in 0..BAR_COUNT {
            let (f_start, f_end) = if i < BASS_LOW_BARS {
                let j = i as f32;
                let t0 = j / BASS_LOW_BARS as f32;
                let t1 = (j + 1.0) / BASS_LOW_BARS as f32;
                (
                    f_bass_low_lo * (f_bass_low_hi / f_bass_low_lo as f32).powf(t0),
                    f_bass_low_lo * (f_bass_low_hi / f_bass_low_lo as f32).powf(t1),
                )
            } else if i < BASS_LOW_BARS + MIDS_BARS {
                let j = (i - BASS_LOW_BARS) as f32;
                let t0 = j / MIDS_BARS as f32;
                let t1 = (j + 1.0) / MIDS_BARS as f32;
                (
                    f_mids_lo * (f_mids_hi / f_mids_lo as f32).powf(t0),
                    f_mids_lo * (f_mids_hi / f_mids_lo as f32).powf(t1),
                )
            } else {
                let j = (i - BASS_LOW_BARS - MIDS_BARS) as f32;
                let t0 = j / HIGH_BARS as f32;
                let t1 = (j + 1.0) / HIGH_BARS as f32;
                (
                    f_high_lo * (f_high_hi / f_high_lo as f32).powf(t0),
                    f_high_lo * (f_high_hi / f_high_lo as f32).powf(t1),
                )
            };

            let start = ((f_start / freq_per_bin) as usize).min(bins.saturating_sub(1));
            let end = ((f_end / freq_per_bin) as usize).min(bins).max(start + 1);

            let band_max: f32 = self.fft_input[start..end]
                .iter()
                .map(|c: &Complex<f32>| c.norm())
                .fold(0.0f32, f32::max);
            let band_avg: f32 = self.fft_input[start..end]
                .iter()
                .map(|c: &Complex<f32>| c.norm())
                .sum::<f32>()
                / (end - start) as f32;
            let mag = band_max * 0.4 + band_avg * 0.6;

            // Attenuate bass/low, boost mids (center), boost highs; extra for vocal + percussive presence (2-5 kHz)
            let tilt = if i < BASS_LOW_BARS {
                0.22 + 0.15 * (i as f32 / BASS_LOW_BARS as f32)
            } else if i < BASS_LOW_BARS + MIDS_BARS {
                let mid_j = (i - BASS_LOW_BARS) as f32;
                let base = 1.0 + 0.4 * (mid_j / MIDS_BARS as f32);
                let vocal_boost = if mid_j >= 28.0 && mid_j <= 43.0 {
                    1.9
                } else if mid_j >= 12.0 && mid_j <= 40.0 {
                    1.75
                } else {
                    1.0
                };
                base * vocal_boost
            } else {
                let high_j = (i - BASS_LOW_BARS - MIDS_BARS) as f32;
                2.4 + 2.2 * (high_j / HIGH_BARS as f32)
            };

            let f_center = (f_start + f_end) / 2.0;
            let guitar_cut = if (180.0..520.0).contains(&f_center) {
                0.72
            } else if (800.0..4200.0).contains(&f_center) {
                0.78
            } else {
                1.0
            };
            let tilt = tilt * guitar_cut;

            let target_height = (mag * gain * tilt).clamp(0.0, 1.0);
            self.bar_heights[i] = self.bar_heights[i] * 0.8 + target_height * 0.2;

            const PEAK_HYSTERESIS: f32 = 0.05;
            if target_height > self.peak_heights[i] + PEAK_HYSTERESIS {
                self.peak_heights[i] = self.peak_heights[i] * 0.6 + target_height * 0.4;
                let is_transient = target_height > self.bar_heights[i] + 0.02;
                let is_bass = i < 10;
                if self.fire_cooldown[i] == 0 && (is_transient || is_bass) {
                    self.peak_fired.push(i);
                    self.fire_cooldown[i] = 8;
                }
            }
        }
    }

    fn reset_bars(&mut self) {
        self.bar_heights = [0.0; BAR_COUNT];
        self.peak_heights = [0.0; BAR_COUNT];
        self.peak_fired.clear();
        self.fire_cooldown = [0; BAR_COUNT];
        self.peak_magnitude = 0.01;
    }

    fn tick_cooldowns(&mut self) {
        for c in &mut self.fire_cooldown {
            *c = c.saturating_sub(1);
        }
    }

    fn decay_peaks(&mut self) {
        const BASS_LOW_BARS: usize = 10;
        for (i, p) in self.peak_heights.iter_mut().enumerate() {
            let decay = if i < BASS_LOW_BARS {
                0.985
            } else {
                0.98
            };
            *p = (*p * decay).max(self.bar_heights[i]);
        }
    }
}

#[macroquad::main("Audio Visualizer")]
async fn main() {
    let (tx, rx) = mpsc::channel::<Vec<f32>>();
    let frames_received = Arc::new(AtomicU64::new(0));

    thread::spawn({
        let frames = Arc::clone(&frames_received);
        move || capture_audio(tx, frames)
    });

    let mut state = VisualizerState::new();
    let mut show_fps = true;
    let mut projectiles: Vec<Projectile> = Vec::new();
    let mut fullscreen = false;
    let mut saved_window_size: Option<(f32, f32)> = None;
    let mut rainbow_phase: f32 = 0.0;
    let mut rotating = false;
    let mut circle_rotation: f32 = 0.0;
    let mut rotation_speed: f32 = 0.0;
    let mut prev_screen_size: (f32, f32) = (screen_width(), screen_height());
    let mut projectile_decel_mode = false;
    let mut projectile_distance_based = false;
    let mut game_time: f32 = 0.0;
    let mut last_input_peak: f32 = 0.0;

    loop {
        let (w, h) = (screen_width(), screen_height());
        if (w, h) != prev_screen_size {
            projectiles.clear();
            prev_screen_size = (w, h);
        }
        state.peak_fired.clear();
        if is_key_pressed(KeyCode::F11) {
            if fullscreen {
                set_fullscreen(false);
                if let Some((w, h)) = saved_window_size {
                    request_new_screen_size(w, h);
                }
            } else {
                saved_window_size = Some((screen_width(), screen_height()));
                set_fullscreen(true);
            }
            fullscreen = !fullscreen;
        }
        if is_key_pressed(KeyCode::Escape) && fullscreen {
            set_fullscreen(false);
            if let Some((w, h)) = saved_window_size {
                request_new_screen_size(w, h);
            }
            fullscreen = false;
        }
        if is_key_pressed(KeyCode::Space) {
            show_fps = !show_fps;
        }
        if is_key_pressed(KeyCode::S) {
            if rotating {
                circle_rotation = 0.0;
            }
            rotating = !rotating;
        }
        if is_key_pressed(KeyCode::A) {
            projectile_decel_mode = !projectile_decel_mode;
        }
        if is_key_pressed(KeyCode::D) {
            projectile_distance_based = !projectile_distance_based;
        }
        if rotating {
            if is_key_pressed(KeyCode::Up) {
                rotation_speed = next_speed(rotation_speed, 1);
            }
            if is_key_pressed(KeyCode::Down) {
                rotation_speed = next_speed(rotation_speed, -1);
            }
        }

        while let Ok(mut data) = rx.try_recv() {
            // Normalize level so low tap output (e.g. macOS aggregate) still shows bars
            let peak = data
                .iter()
                .map(|&s| s.abs())
                .fold(0.0f32, f32::max);
            if peak > 1e-8 {
                let target = 0.4f32;
                let scale = (target / peak).min(1000.0);
                for s in &mut data {
                    *s *= scale;
                }
            }
            last_input_peak = peak;
            state.update(&data);
        }
        state.tick_cooldowns();
        state.decay_peaks();

        let screen_width = screen_width();
        let screen_height = screen_height();
        let cx = screen_width / 2.0;
        let cy = screen_height / 2.0;
        let inner_radius = 40.0;
        let max_bar_length = (screen_width.min(screen_height) * 0.5) - inner_radius;
        let angle_step = std::f32::consts::TAU / BAR_COUNT as f32;
        let gap = angle_step * 0.04;
        if rotating {
            circle_rotation += rotation_speed * get_frame_time();
        }
        let rotation = std::f32::consts::FRAC_PI_4 + circle_rotation;

        let dt = get_frame_time();
        game_time += dt;

        for bar_i in state.peak_fired.drain(..) {
            let peak_length = state.peak_heights[bar_i] * max_bar_length;
            if peak_length > 3.0 {
                let mid_angle = -std::f32::consts::FRAC_PI_2 + rotation + (bar_i as f32 + 0.5) * angle_step;
                let dx = mid_angle.cos();
                let dy = mid_angle.sin();
                let peak = state.peak_heights[bar_i];
                let size = (1.0 + peak * 7.0).clamp(1.0, 8.0);
                projectiles.push(Projectile {
                    x: cx + inner_radius * dx,
                    y: cy + inner_radius * dy,
                    dx: dx * 180.0,
                    dy: dy * 180.0,
                    hue: rainbow_phase,
                    size,
                    trail: Vec::new(),
                    birth_time: game_time,
                });
                rainbow_phase = (rainbow_phase + 3.0) % 360.0;
            }
        }
        rainbow_phase = (rainbow_phase + 1.5) % 360.0;

        // Distance mode: slowdown only in this many px before edge; curve keeps min speed until closer
        const PROXIMITY_RANGE: f32 = 200.0;
        const PROXIMITY_POWER: f32 = 1.8;
        // Time mode: seconds alive before proximity reaches 1 (min speed); longer = stay fast until nearer edge
        const TIME_RAMP: f32 = 6.0;
        const SPEED_RATE: f32 = 2.5;
        const MIN_SPEED: f32 = 90.0;
        let margin = 80.0;
        projectiles.retain_mut(|p| {
            p.trail.push((p.x, p.y));
            if p.trail.len() > 12 {
                p.trail.remove(0);
            }
            let proximity = if projectile_distance_based {
                let speed = (p.dx * p.dx + p.dy * p.dy).sqrt();
                if speed > 0.0001 {
                    let ux = p.dx / speed;
                    let uy = p.dy / speed;
                    let mut edge_dist = f32::INFINITY;
                    if ux > 0.001 {
                        let t = (screen_width - p.x) / ux;
                        if t > 0.0 {
                            edge_dist = edge_dist.min(t);
                        }
                    }
                    if ux < -0.001 {
                        let t = -p.x / ux;
                        if t > 0.0 {
                            edge_dist = edge_dist.min(t);
                        }
                    }
                    if uy > 0.001 {
                        let t = (screen_height - p.y) / uy;
                        if t > 0.0 {
                            edge_dist = edge_dist.min(t);
                        }
                    }
                    if uy < -0.001 {
                        let t = -p.y / uy;
                        if t > 0.0 {
                            edge_dist = edge_dist.min(t);
                        }
                    }
                    if edge_dist < PROXIMITY_RANGE {
                        let raw = 1.0 - edge_dist / PROXIMITY_RANGE;
                        raw.powf(PROXIMITY_POWER)
                    } else {
                        0.0
                    }
                } else {
                    0.0
                }
            } else {
                let time_alive = game_time - p.birth_time;
                (time_alive / TIME_RAMP).min(1.0)
            };
            let rate = SPEED_RATE * proximity * dt;
            let speed_mult = if projectile_decel_mode {
                1.0 / (1.0 + rate)
            } else {
                1.0 + rate
            };
            p.dx *= speed_mult;
            p.dy *= speed_mult;
            let new_speed = (p.dx * p.dx + p.dy * p.dy).sqrt();
            if new_speed > 0.0 && new_speed < MIN_SPEED {
                let scale = MIN_SPEED / new_speed;
                p.dx *= scale;
                p.dy *= scale;
            }
            p.x += p.dx * dt;
            p.y += p.dy * dt;
            let (min_x, max_x) = p.trail.iter().fold((p.x, p.x), |(lo, hi), &(tx, _)| (lo.min(tx), hi.max(tx)));
            let (min_y, max_y) = p.trail.iter().fold((p.y, p.y), |(lo, hi), &(_, ty)| (lo.min(ty), hi.max(ty)));
            let outside_left = max_x < -margin;
            let outside_right = min_x > screen_width + margin;
            let outside_top = max_y < -margin;
            let outside_bottom = min_y > screen_height + margin;
            !outside_left && !outside_right && !outside_top && !outside_bottom
        });

        clear_background(BLACK);

        let perspective_ref = (screen_width.max(screen_height) * 0.55).max(400.0);
        for p in projectiles.iter() {
            let dist = ((p.x - cx).powi(2) + (p.y - cy).powi(2)).sqrt();
            let perspective = 0.5 + 1.2 * (dist / perspective_ref).min(1.0);
            let len = p.trail.len() as f32;
            for (i, &(tx, ty)) in p.trail.iter().enumerate() {
                let t = i as f32 / len.max(1.0);
                let alpha = 0.04 + 0.7 * t * t;
                let mut c = hsv_to_color(p.hue, 0.9, 1.0);
                c.a = alpha;
                let trail_dist = ((tx - cx).powi(2) + (ty - cy).powi(2)).sqrt();
                let trail_perspective = 0.5 + 1.2 * (trail_dist / perspective_ref).min(1.0);
                let trail_size = p.size * (0.3 + 0.7 * t) * trail_perspective;
                let half = trail_size / 2.0;
                draw_rectangle(tx - half, ty - half, trail_size, trail_size, c);
            }
            let c = hsv_to_color(p.hue, 0.95, 1.0);
            let head_size = p.size * perspective;
            let half = head_size / 2.0;
            draw_rectangle(p.x - half, p.y - half, head_size, head_size, c);
        }

        for (i, &height) in state.bar_heights.iter().enumerate() {
            let bar_length = height * max_bar_length;
            let start_angle = -std::f32::consts::FRAC_PI_2 + rotation + i as f32 * angle_step + gap;
            let end_angle = -std::f32::consts::FRAC_PI_2 + rotation + (i + 1) as f32 * angle_step - gap;

            let color_index = (height * (COLORS.len() - 1) as f32) as usize;
            let color = COLORS[color_index.min(COLORS.len() - 1)];

            let outer_radius = inner_radius + bar_length;

            let v1 = Vec2::new(cx + inner_radius * start_angle.cos(), cy + inner_radius * start_angle.sin());
            let v2 = Vec2::new(cx + outer_radius * start_angle.cos(), cy + outer_radius * start_angle.sin());
            let v3 = Vec2::new(cx + outer_radius * end_angle.cos(), cy + outer_radius * end_angle.sin());
            let v4 = Vec2::new(cx + inner_radius * end_angle.cos(), cy + inner_radius * end_angle.sin());

            draw_triangle(v1, v2, v3, color);
            draw_triangle(v1, v3, v4, color);

            let peak_length = state.peak_heights[i] * max_bar_length;
            if peak_length > 3.0 {
                let peak_radius = inner_radius + peak_length;
                let mid_angle = start_angle + angle_step * 0.5;
                let px = cx + peak_radius * mid_angle.cos();
                let py = cy + peak_radius * mid_angle.sin();
                let perp_x = -mid_angle.sin() * 4.0;
                let perp_y = mid_angle.cos() * 4.0;
                draw_line(px - perp_x, py - perp_y, px + perp_x, py + perp_y, 2.0, WHITE);
            }
        }

        draw_circle_lines(cx, cy, inner_radius, 2.0, GRAY);

        // Draw FPS
        if show_fps {
            let y = 30.0;
            draw_text(&format!("FPS: {:.0}", get_fps()), 10.0, y, 20.0, GREEN);
            draw_text(
                &format!("Audio: {} frames", frames_received.load(Ordering::Relaxed)),
                10.0,
                y + 25.0,
                16.0,
                if frames_received.load(Ordering::Relaxed) > 0 {
                    GREEN
                } else {
                    RED
                },
            );
            draw_text(
                &format!("Level: {:.4} (before norm)", last_input_peak),
                10.0,
                y + 44.0,
                14.0,
                if last_input_peak > 1e-6 { GREEN } else { ORANGE },
            );
            draw_text("SPACE: FPS | S: Rotate | ↑↓: Speed | A: Accel/Decel | D: Dist/Time | F11: Fullscreen", 10.0, y + 63.0, 14.0, DARKGRAY);
            if frames_received.load(Ordering::Relaxed) == 0 {
                draw_text("No audio", 10.0, y + 99.0, 12.0, ORANGE);
            }
        }

        next_frame().await
    }
}

fn capture_audio(tx: mpsc::Sender<Vec<f32>>, frames_received: Arc<AtomicU64>) {
    #[cfg(windows)]
    capture_windows::capture_loopback(tx, frames_received);

    #[cfg(target_os = "macos")]
    capture_macos_sck::capture_loopback(tx, frames_received);

    #[cfg(all(not(windows), not(target_os = "macos")))]
    capture_audio_cpal(tx, frames_received);
}

#[cfg(all(not(windows), not(target_os = "macos")))]
fn capture_audio_cpal(tx: mpsc::Sender<Vec<f32>>, frames_received: Arc<AtomicU64>) {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .expect("No default output device available");
    let config = device
        .default_output_config()
        .expect("Failed to get output config");
    let sample_format = config.sample_format();
    let channels = config.channels() as usize;
    let stream_config: cpal::StreamConfig = config.into();
    let mut sample_buffer: Vec<f32> = Vec::with_capacity(1024);
    const SAMPLES_NEEDED: usize = FFT_SIZE;
    let err_fn = |err| eprintln!("Audio error: {}", err);

    macro_rules! build_stream {
        ($fmt:ty, $convert:expr) => {{
            let frames = Arc::clone(&frames_received);
            let stream = device
                .build_input_stream(
                    &stream_config,
                    move |data: &[$fmt], _: &cpal::InputCallbackInfo| {
                        let f32_samples: Vec<f32> = data.iter().map($convert).collect();
                        let mut samples = stereo_to_mono_f32(&f32_samples, channels);
                        sample_buffer.append(&mut samples);
                        while sample_buffer.len() >= SAMPLES_NEEDED {
                            let chunk: Vec<f32> =
                                sample_buffer.drain(..SAMPLES_NEEDED).collect();
                            let peak = chunk.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
                            let _ = tx.send(chunk);
                            if peak >= 1e-6 {
                                frames.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                    },
                    err_fn,
                    None,
                )
                .expect("Failed to build audio stream");
            stream.play().expect("Failed to start audio stream");
        }};
    }

    match sample_format {
        cpal::SampleFormat::F32 => build_stream!(f32, |&s| f32::from_sample(s)),
        cpal::SampleFormat::I16 => build_stream!(i16, |&s| f32::from_sample(s)),
        cpal::SampleFormat::U16 => build_stream!(u16, |&s| f32::from_sample(s)),
        cpal::SampleFormat::I32 => build_stream!(i32, |&s| f32::from_sample(s)),
        fmt => panic!("Unsupported sample format: {:?}", fmt),
    }

    loop {
        thread::sleep(Duration::from_millis(100));
    }
}

#[cfg(all(not(windows), not(target_os = "macos")))]
fn stereo_to_mono_f32(samples: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return samples.to_vec();
    }
    samples
        .chunks(channels)
        .map(|chunk| chunk.iter().sum::<f32>() / channels as f32)
        .collect()
}