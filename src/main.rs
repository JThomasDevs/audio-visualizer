//! Audio Visualizer - Classic Windows Media Player style
//! Recreated in Rust using macroquad and cpal

use macroquad::prelude::*;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

// Configuration
const FFT_SIZE: usize = 512;        // Number of samples for FFT
const BAR_COUNT: usize = 64;         // Number of visualizer bars
const SAMPLE_RATE: u32 = 44100;
const BIN_SIZE: f32 = SAMPLE_RATE as f32 / FFT_SIZE as f32;

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
}

impl VisualizerState {
    fn new() -> Self {
        Self {
            bar_heights: [0.0; BAR_COUNT],
            peak_heights: [0.0; BAR_COUNT],
        }
    }

    fn update(&mut self, audio_data: &[f32]) {
        // Simple frequency band mapping (placeholder for real FFT)
        let bands_per_bar = FFT_SIZE / 2 / BAR_COUNT;

        for i in 0..BAR_COUNT {
            let start = i * bands_per_bar;
            let end = start + bands_per_bar;

            // Calculate average amplitude for this frequency band
            let avg: f32 = audio_data[start..end]
                .iter()
                .map(|&x| x.abs())
                .sum::<f32>() / bands_per_bar as f32;

            // Scale and smooth the bar height
            let target_height = (avg * 1000.0).clamp(0.0, 1.0);
            self.bar_heights[i] = self.bar_heights[i] * 0.85 + target_height * 0.15;

            // Update peak with decay
            if target_height > self.peak_heights[i] {
                self.peak_heights[i] = target_height;
            } else {
                self.peak_heights[i] *= 0.95;
            }
        }
    }
}

#[macroquad::main("Audio Visualizer")]
async fn main() {
    // Create channel for audio data
    let (tx, rx) = mpsc::channel::<Vec<f32>>();

    // Start audio capture thread
    thread::spawn(move || {
        capture_audio(tx);
    });

    let mut state = VisualizerState::new();
    let mut show_fps = true;

    loop {
        // Handle input
        if is_key_pressed(KeyCode::Space) {
            show_fps = !show_fps;
        }

        // Receive audio data if available
        while let Ok(data) = rx.try_recv() {
            state.update(&data);
        }

        // Rendering
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
            draw_text(
                &format!("FPS: {:.0}", get_fps()),
                10.0,
                30.0,
                20.0,
                GREEN,
            );
            draw_text("Press SPACE to toggle FPS", 10.0, 55.0, 16.0, GRAY);
        }

        next_frame().await
    }
}

/// Audio capture using cpal - tries desktop audio (monitor) first, falls back to mic
fn capture_audio(tx: mpsc::Sender<Vec<f32>>) {
    let host = cpal::default_host();

    // Try to find a monitor/sink input device (desktop audio)
    let device = find_desktop_audio_device(&host)
        .or_else(|| host.default_input_device())
        .expect("No audio input device available");

    println!("Using audio device: {}", device.name().unwrap_or("Unknown".to_string()));

    let config = cpal::StreamConfig {
        channels: 1,
        sample_rate: cpal::SampleRate(SAMPLE_RATE),
        buffer_size: cpal::BufferSize::Default,
    };

    let err_fn = |err| eprintln!("Audio error: {}", err);

    let stream = device
        .build_input_stream(&config, move |data: &[f32], _: &cpal::InputCallbackInfo| {
            let _ = tx.send(data.to_vec());
        }, err_fn, None)
        .expect("Failed to build audio stream");

    stream.play().expect("Failed to start audio stream");

    loop {
        thread::sleep(Duration::from_millis(100));
    }
}

/// Try to find a desktop audio monitor device
fn find_desktop_audio_device(host: &cpal::Host) -> Option<cpal::Device> {
    // On Linux with PulseAudio, look for "monitor" devices
    // These capture system audio output
    let devices = host.devices().ok()?;

    for device in devices {
        if let Ok(name) = device.name() {
            // Look for monitor/sink input patterns
            if name.to_lowercase().contains("monitor")
                || name.to_lowercase().contains("sink")
                || name.to_lowercase().contains("virtual") {
                println!("Found desktop audio device: {}", name);
                return Some(device);
            }
        }
    }

    None
}
