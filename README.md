# Audio Visualizer

Classic Windows Media Player–style circular visualizer in Rust. The bars react to system/playback audio and animate in real time.

## Quick Start

```bash
cargo run --release
```

On **macOS**, the release build links to Swift (ScreenCaptureKit). If you see `Library not loaded: libswift_Concurrency.dylib`, run instead:

```bash
./run-release.sh
```

Or use a debug run (no Swift path needed): `cargo run`.

- **Windows**: Captures default playback device (system audio) via WASAPI loopback.
- **macOS**: Uses **ScreenCaptureKit (SCK)** for system audio. Requires **Screen Recording** permission—see [macOS (ScreenCaptureKit)](#macos-screencapturekit) for details.
- **Linux**: Uses default capture device (e.g. microphone) via CPAL.

## Controls

- **SPACE** – Toggle FPS display  
- **S** – Rotate  
- **↑/↓** – Speed  
- **A** – Accel/Decel  
- **D** – Distance/Time  
- **F11** – Fullscreen  

## Dependencies

- **macroquad** – 2D rendering  
- **rustfft** – FFT  
- **cpal** – Audio (Linux)  
- **wasapi** – Windows loopback  
- **screencapturekit** (macOS) – display + system audio capture  

## macOS (ScreenCaptureKit)

On macOS the app uses **ScreenCaptureKit (SCK)** to capture system audio. A minimal display stream (64×64) is created so SCK delivers audio; only the audio is used for the visualizer. No BlackHole or other third-party audio drivers.

**Permissions:** You must grant the following in **System Settings → Privacy & Security** when prompted (or before running):

- **Screen Recording** – Required for SCK to capture display and system audio. Enable for Terminal (or your IDE) if you run via `cargo`/`./run-release.sh`, or for the `audio-visualizer` binary if you run the built executable directly.

After granting access, the visualizer shows dancing bars that react to whatever is playing through the system output.

