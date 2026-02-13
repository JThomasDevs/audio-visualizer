//! Audio Visualizer - Classic Windows Media Player style
//! Recreated in Rust using macroquad and cpal

use macroquad::prelude::*;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample};
use rustfft::num_complex::Complex;
use rustfft::{Fft, FftPlanner};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

// Configuration
const FFT_SIZE: usize = 512;
const BAR_COUNT: usize = 64;

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

struct VisualizerState {
    bar_heights: [f32; BAR_COUNT],
    peak_heights: [f32; BAR_COUNT],
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

        // FFT results: first FFT_SIZE/2+1 are unique (Nyquist), we use FFT_SIZE/2 for bars
        let bins = FFT_SIZE / 2;
        let bands_per_bar = bins / BAR_COUNT;

        // Update peak magnitude for adaptive gain (slow attack, slow release)
        let max_mag: f32 = self.fft_input[..bins]
            .iter()
            .map(|c: &Complex<f32>| c.norm())
            .fold(0.0f32, f32::max);
        self.peak_magnitude = self.peak_magnitude * 0.995 + max_mag * 0.005;
        let gain = if self.peak_magnitude > 0.0001 {
            0.3 / self.peak_magnitude
        } else {
            1000.0
        };

        for i in 0..BAR_COUNT {
            let start = i * bands_per_bar;
            let end = start + bands_per_bar;

            let avg_mag: f32 = self.fft_input[start..end]
                .iter()
                .map(|c: &Complex<f32>| c.norm())
                .sum::<f32>()
                / bands_per_bar as f32;

            let target_height = (avg_mag * gain).clamp(0.0, 1.0);
            self.bar_heights[i] = self.bar_heights[i] * 0.8 + target_height * 0.2;

            if target_height > self.peak_heights[i] {
                self.peak_heights[i] = target_height;
            } else {
                self.peak_heights[i] *= 0.95;
            }
        }
    }

    fn reset_bars(&mut self) {
        self.bar_heights = [0.0; BAR_COUNT];
        self.peak_heights = [0.0; BAR_COUNT];
        self.peak_magnitude = 0.01;
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
    let mut demo_mode = false;
    let mut demo_phase: f32 = 0.0;

    loop {
        if is_key_pressed(KeyCode::Space) {
            show_fps = !show_fps;
        }
        if is_key_pressed(KeyCode::D) {
            let was_demo = demo_mode;
            demo_mode = !demo_mode;
            // Reset bars when leaving demo so old data doesn't stick
            if was_demo && !demo_mode {
                state.reset_bars();
            }
        }

        if !demo_mode {
            while let Ok(data) = rx.try_recv() {
                state.update(&data);
            }
        } else {
            demo_phase += 0.05;
            let fake_data: Vec<f32> = (0..FFT_SIZE)
                .map(|i| {
                    let t = demo_phase + i as f32 * 0.15;
                    t.sin() * 0.4 + (t * 2.3).sin() * 0.3 + (t * 5.1 + i as f32 * 0.2).sin() * 0.2
                })
                .collect();
            state.update(&fake_data);
        }

        clear_background(BLACK);

        let screen_width = screen_width();
        let screen_height = screen_height();

        // Draw visualizer bars
        let bar_width = screen_width / BAR_COUNT as f32;
        let max_bar_height = screen_height * 0.8;
        let baseline = screen_height - 50.0;

        for (i, &height) in state.bar_heights.iter().enumerate() {
            let x = i as f32 * bar_width;
            let bar_height = height * max_bar_height;

            // Choose color based on height
            let color_index = (height * (COLORS.len() - 1) as f32) as usize;
            let color = COLORS[color_index.min(COLORS.len() - 1)];

            // Draw bar with gradient effect
            draw_rectangle(x + 1.0, baseline - bar_height, bar_width - 2.0, bar_height, color);

            // Draw peak indicator
            let peak_height = state.peak_heights[i] * max_bar_height;
            if peak_height > 5.0 {
                draw_rectangle(
                    x + 1.0,
                    baseline - peak_height,
                    bar_width - 2.0,
                    3.0,
                    WHITE,
                );
            }
        }

        // Draw baseline
        draw_line(0.0, baseline, screen_width, baseline, 2.0, GRAY);

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
            draw_text("SPACE: FPS | D: Demo mode", 10.0, y + 50.0, 14.0, DARKGRAY);
            if frames_received.load(Ordering::Relaxed) == 0 && !demo_mode {
                draw_text("No audio - set Recording to CABLE Input, Playback to CABLE Output", 10.0, y + 70.0, 11.0, ORANGE);
            }
        }
        if demo_mode {
            draw_text("DEMO MODE - Press D to use microphone", 10.0, screen_height - 20.0, 18.0, YELLOW);
        }

        next_frame().await
    }
}

/// Audio capture - uses default input (set to CABLE Input for desktop audio via VB-CABLE)
fn capture_audio(tx: mpsc::Sender<Vec<f32>>, frames_received: Arc<AtomicU64>) {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .expect("No default input device available");

    let config = device.default_input_config().unwrap_or_else(|_| {
        device
            .supported_input_configs()
            .expect("Failed to query configs")
            .next()
            .expect("No supported input config")
            .with_max_sample_rate()
    });
    let device_name = device.name().unwrap_or_else(|_| "Unknown".into());
    println!("Using audio device: {} ({:?})", device_name, config.sample_format());

    let sample_format = config.sample_format();
    let channels = config.channels() as usize;
    let stream_config: cpal::StreamConfig = config.into();

    let mut sample_buffer: Vec<f32> = Vec::with_capacity(1024);
    const SAMPLES_NEEDED: usize = FFT_SIZE;

    let err_fn = |err| eprintln!("Audio error: {}", err);

    match sample_format {
        cpal::SampleFormat::F32 => {
            let frames = Arc::clone(&frames_received);
            let stream = device
                .build_input_stream(
                    &stream_config,
                    move |data: &[f32], _: &cpal::InputCallbackInfo| {
                        let mut samples = stereo_to_mono_f32(data, channels);
                        sample_buffer.append(&mut samples);
                        while sample_buffer.len() >= SAMPLES_NEEDED {
                            let chunk: Vec<f32> = sample_buffer.drain(..SAMPLES_NEEDED).collect();
                            let _ = tx.send(chunk);
                            frames.fetch_add(1, Ordering::Relaxed);
                        }
                    },
                    err_fn,
                    None,
                )
                .expect("Failed to build audio stream");
            stream.play().expect("Failed to start audio stream");
        }
        cpal::SampleFormat::I16 => {
            let frames = Arc::clone(&frames_received);
            let stream = device
                .build_input_stream(
                    &stream_config,
                    move |data: &[i16], _: &cpal::InputCallbackInfo| {
                        let f32_samples: Vec<f32> = data
                            .iter()
                            .map(|&s| f32::from_sample(s))
                            .collect();
                        let mut samples = stereo_to_mono_f32(&f32_samples, channels);
                        sample_buffer.append(&mut samples);
                        while sample_buffer.len() >= SAMPLES_NEEDED {
                            let chunk: Vec<f32> = sample_buffer.drain(..SAMPLES_NEEDED).collect();
                            let _ = tx.send(chunk);
                            frames.fetch_add(1, Ordering::Relaxed);
                        }
                    },
                    err_fn,
                    None,
                )
                .expect("Failed to build audio stream");
            stream.play().expect("Failed to start audio stream");
        }
        cpal::SampleFormat::U16 => {
            let frames = Arc::clone(&frames_received);
            let stream = device
                .build_input_stream(
                    &stream_config,
                    move |data: &[u16], _: &cpal::InputCallbackInfo| {
                        let f32_samples: Vec<f32> = data
                            .iter()
                            .map(|&s| f32::from_sample(s))
                            .collect();
                        let mut samples = stereo_to_mono_f32(&f32_samples, channels);
                        sample_buffer.append(&mut samples);
                        while sample_buffer.len() >= SAMPLES_NEEDED {
                            let chunk: Vec<f32> = sample_buffer.drain(..SAMPLES_NEEDED).collect();
                            let _ = tx.send(chunk);
                            frames.fetch_add(1, Ordering::Relaxed);
                        }
                    },
                    err_fn,
                    None,
                )
                .expect("Failed to build audio stream");
            stream.play().expect("Failed to start audio stream");
        }
        cpal::SampleFormat::I32 => {
            let frames = Arc::clone(&frames_received);
            let stream = device
                .build_input_stream(
                    &stream_config,
                    move |data: &[i32], _: &cpal::InputCallbackInfo| {
                        let f32_samples: Vec<f32> = data
                            .iter()
                            .map(|&s| f32::from_sample(s))
                            .collect();
                        let mut samples = stereo_to_mono_f32(&f32_samples, channels);
                        sample_buffer.append(&mut samples);
                        while sample_buffer.len() >= SAMPLES_NEEDED {
                            let chunk: Vec<f32> = sample_buffer.drain(..SAMPLES_NEEDED).collect();
                            let _ = tx.send(chunk);
                            frames.fetch_add(1, Ordering::Relaxed);
                        }
                    },
                    err_fn,
                    None,
                )
                .expect("Failed to build audio stream");
            stream.play().expect("Failed to start audio stream");
        }
        fmt => {
            panic!("Unsupported sample format: {:?}", fmt);
        }
    }

    loop {
        thread::sleep(Duration::from_millis(100));
    }
}

/// Convert stereo (or multi-channel) to mono by averaging channels
fn stereo_to_mono_f32(samples: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return samples.to_vec();
    }
    samples
        .chunks(channels)
        .map(|chunk| chunk.iter().sum::<f32>() / channels as f32)
        .collect()
}

