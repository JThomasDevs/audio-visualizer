#!/usr/bin/env bash
# Build and run the release binary directly (so Screen Recording permission is
# granted to the app, not Cursor/Terminal). Sets Swift dylib path for SCK.
set -e
cd "$(dirname "$0")"
cargo build --release
SWIFT_LIB=""
for candidate in \
  "$(xcrun --find swift 2>/dev/null | sed 's|/bin/swift$||')/lib/swift/macosx" \
  "$(xcrun --find swift 2>/dev/null | sed 's|/bin/swift$||')/lib/swift-5.5/macosx" \
  "/Library/Developer/CommandLineTools/usr/lib/swift-5.5/macosx" \
  "/Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/lib/swift-5.5/macosx"; do
  if [[ -n "$candidate" && -f "${candidate}/libswift_Concurrency.dylib" ]]; then
    SWIFT_LIB="$candidate"
    break
  fi
done
if [[ -z "$SWIFT_LIB" ]]; then
  echo "Could not find libswift_Concurrency.dylib (Xcode required for macOS capture)."
  exit 1
fi
export DYLD_LIBRARY_PATH="$SWIFT_LIB${DYLD_LIBRARY_PATH:+:$DYLD_LIBRARY_PATH}"
cargo build --release
exec "$(dirname "$0")/target/release/audio-visualizer" "$@"
