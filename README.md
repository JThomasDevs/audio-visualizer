# Audio Visualizer

Classic Windows Media Player visualizer, recreated in Rust.

## Quick Start

```bash
# Run with default microphone input
cargo run --release
```

## Controls

- **SPACE** - Toggle FPS display

## Dependencies

- **cpal** - Audio capture (microphone)
- **rustfft** - FFT (ready to integrate)
- **macroquad** - 2D rendering

## Next Steps

1. Add real FFT processing
2. Add audio file playback support
3. Add visualization modes (waveform, circular, etc.)
4. Add color scheme customization
