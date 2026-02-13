//! macOS system audio capture using ScreenCaptureKit (macOS 12.3+, audio from 13.0).
//! Captures display + system audio; we use only the audio. No third-party apps.

use screencapturekit::cm::CMSampleBuffer;
use screencapturekit::prelude::*;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

const FFT_SIZE: usize = 2048;

struct CaptureState {
    tx: mpsc::Sender<Vec<f32>>,
    buffer: Mutex<Vec<f32>>,
    frames_received: Arc<AtomicU64>,
}

struct AudioHandler {
    state: Arc<CaptureState>,
}

impl SCStreamOutputTrait for AudioHandler {
    fn did_output_sample_buffer(&self, sample: CMSampleBuffer, output_type: SCStreamOutputType) {
        if output_type != SCStreamOutputType::Audio {
            return;
        }
        let Some(abl) = sample.audio_buffer_list() else {
            return;
        };
        let mut mono = Vec::with_capacity(4096);
        let n_bufs = abl.num_buffers();
        if n_bufs == 0 {
            return;
        }
        let bytes_per_channel = abl.get(0).map(|b| b.data_byte_size()).unwrap_or(0);
        if bytes_per_channel == 0 {
            return;
        }
        let n_frames = bytes_per_channel / 4;
        if n_bufs >= 2 {
            let b0 = abl.get(0).unwrap().data();
            let b1 = abl.get(1).unwrap().data();
            if b0.len() >= n_frames * 4 && b1.len() >= n_frames * 4 {
                let s0 = unsafe { std::slice::from_raw_parts(b0.as_ptr() as *const f32, n_frames) };
                let s1 = unsafe { std::slice::from_raw_parts(b1.as_ptr() as *const f32, n_frames) };
                for i in 0..n_frames {
                    mono.push((s0[i] + s1[i]) * 0.5);
                }
            }
        } else {
            let b = abl.get(0).unwrap();
            let ch = b.number_channels as usize;
            let data = b.data();
            let n_samps = data.len() / 4;
            if ch <= 1 {
                let s = unsafe { std::slice::from_raw_parts(data.as_ptr() as *const f32, n_samps) };
                mono.extend_from_slice(s);
            } else {
                let s = unsafe { std::slice::from_raw_parts(data.as_ptr() as *const f32, n_samps) };
                for c in s.chunks(ch) {
                    let v: f32 = c.iter().sum::<f32>() / ch as f32;
                    mono.push(v);
                }
            }
        }
        if mono.is_empty() {
            return;
        }
        let mut guard = match self.state.buffer.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        guard.extend(mono);
        while guard.len() >= FFT_SIZE {
            let chunk: Vec<f32> = guard.drain(..FFT_SIZE).collect();
            drop(guard);
            if self.state.tx.send(chunk).is_err() {
                return;
            }
            self.state.frames_received.fetch_add(1, Ordering::Relaxed);
            guard = match self.state.buffer.lock() {
                Ok(g) => g,
                Err(_) => return,
            };
        }
    }
}

/// Runs system audio capture via ScreenCaptureKit; reinitializes on error.
pub fn capture_loopback(tx: mpsc::Sender<Vec<f32>>, frames_received: Arc<AtomicU64>) {
    loop {
        if let Err(e) = run_capture(tx.clone(), &frames_received) {
            eprintln!("ScreenCaptureKit capture error: {:?}, reinitializing in 2s...", e);
            thread::sleep(std::time::Duration::from_secs(2));
        }
    }
}

fn run_capture(
    tx: mpsc::Sender<Vec<f32>>,
    frames_received: &AtomicU64,
) -> Result<(), Box<dyn std::error::Error>> {
    let content = SCShareableContent::get()?;
    let display = content
        .displays()
        .into_iter()
        .next()
        .ok_or("No displays found")?;

    let filter = SCContentFilter::create()
        .with_display(&display)
        .with_excluding_windows(&[])
        .build();

    let config = SCStreamConfiguration::new()
        .with_width(64)
        .with_height(64)
        .with_captures_audio(true)
        .with_sample_rate(44100)
        .with_channel_count(2);

    let state = Arc::new(CaptureState {
        tx,
        buffer: Mutex::new(Vec::with_capacity(FFT_SIZE * 2)),
        frames_received: Arc::new(AtomicU64::new(0)),
    });
    state
        .frames_received
        .store(frames_received.load(Ordering::Relaxed), Ordering::Relaxed);
    let handler = AudioHandler {
        state: Arc::clone(&state),
    };

    let mut stream = SCStream::new(&filter, &config);
    stream.add_output_handler(handler.clone(), SCStreamOutputType::Screen);
    stream.add_output_handler(handler, SCStreamOutputType::Audio);

    eprintln!("Using macOS ScreenCaptureKit (display + system audio)");
    stream.start_capture()?;

    loop {
        thread::sleep(std::time::Duration::from_millis(100));
        frames_received.store(state.frames_received.load(Ordering::Relaxed), Ordering::Relaxed);
    }
}

impl Clone for AudioHandler {
    fn clone(&self) -> Self {
        Self {
            state: Arc::clone(&self.state),
        }
    }
}
