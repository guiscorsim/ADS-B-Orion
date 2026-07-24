#!/usr/bin/env bash
# Produce sample.sc16 from sample.wav in this directory if missing (shared IQ fixture).
set -euo pipefail

BENCH_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$BENCH_DIR/.." && pwd)"
cd "$ROOT"

WAV="${SAMPLE_WAV:-$BENCH_DIR/sample.wav}"
SC16="${SAMPLE_SC16:-$BENCH_DIR/sample.sc16}"

if [[ -f "$SC16" ]]; then
  echo "already present: $SC16 ($(du -h "$SC16" | awk '{print $1}'))"
  exit 0
fi

if [[ ! -f "$WAV" ]]; then
  echo "error: missing $WAV (place the IQ WAV at $BENCH_DIR/sample.wav, or set SAMPLE_WAV)" >&2
  exit 1
fi

if ! command -v ffmpeg >/dev/null 2>&1; then
  echo "error: ffmpeg not on PATH (enter nix develop / direnv allow)" >&2
  exit 1
fi

echo "converting $WAV -> $SC16 (s16le stereo @ 2.4 MHz)..."
ffmpeg -y -i "$WAV" -f s16le -ac 2 -ar 2400000 "$SC16"
echo "wrote $SC16 ($(du -h "$SC16" | awk '{print $1}'))"
