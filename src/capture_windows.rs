//! Windows loopback capture using wasapi - captures from default output (speakers)

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::thread;
use wasapi::{Direction, DeviceEnumerator, SampleType, StreamMode, WaveFormat};

const FFT_SIZE: usize = 2048;

/// Runs capture in a loop; on stream errors, reinitializes and continues.
pub fn capture_loopback(tx: mpsc::Sender<Vec<f32>>, frames_received: std::sync::Arc<AtomicU64>) {
    wasapi::initialize_mta().ok().expect("COM init");

    loop {
        if let Err(e) = run_capture_loop(&tx, &frames_received) {
            eprintln!("Loopback capture error: {:?}, reinitializing in 2s...", e);
            thread::sleep(std::time::Duration::from_secs(2));
        }
    }
}

fn run_capture_loop(
    tx: &mpsc::Sender<Vec<f32>>,
    frames_received: &AtomicU64,
) -> Result<(), wasapi::WasapiError> {
    let enumerator = DeviceEnumerator::new()?;
    let device = enumerator.get_default_device(&Direction::Render)?;
    let device_name = device.get_friendlyname().unwrap_or_else(|_| "Unknown".into());
    println!("Using audio device: {} (loopback)", device_name);

    let mut audio_client = device.get_iaudioclient()?;
    let desired_format = WaveFormat::new(32, 32, &SampleType::Float, 44100, 2, None);
    let blockalign = desired_format.get_blockalign() as usize;

    let (_def_time, min_time) = audio_client.get_device_period()?;
    let mode = StreamMode::EventsShared {
        autoconvert: true,
        buffer_duration_hns: min_time,
    };
    audio_client.initialize_client(&desired_format, &Direction::Capture, &mode)?;

    let h_event = audio_client.set_get_eventhandle()?;
    let buffer_frame_count = audio_client.get_buffer_size()?;
    let capture_client = audio_client.get_audiocaptureclient()?;

    let mut sample_queue: VecDeque<u8> = VecDeque::with_capacity(
        blockalign as usize * (1024 + 2 * buffer_frame_count as usize),
    );
    audio_client.start_stream()?;

    let channels = 2;

    loop {
        while sample_queue.len() >= blockalign as usize * FFT_SIZE {
            let mut chunk = vec![0u8; blockalign as usize * FFT_SIZE];
            for (_, v) in chunk.iter_mut().enumerate() {
                *v = sample_queue.pop_front().unwrap_or(0);
            }
            let samples: Vec<f32> = chunk
                .chunks(4)
                .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                .collect();
            let mono: Vec<f32> = samples
                .chunks(channels)
                .map(|c| c.iter().sum::<f32>() / channels as f32)
                .collect();
            let peak = mono.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
            if tx.send(mono).is_err() {
                return Ok(());
            }
            if peak >= 1e-6 {
                frames_received.fetch_add(1, Ordering::Relaxed);
            }
        }

        if let Err(e) = capture_client.read_from_device_to_deque(&mut sample_queue) {
            return Err(e);
        }
        if h_event.wait_for_event(100).is_err() {
            thread::sleep(std::time::Duration::from_millis(10));
            continue;
        }
    }
}
