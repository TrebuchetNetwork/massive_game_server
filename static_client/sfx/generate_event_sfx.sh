#!/usr/bin/env bash
set -euo pipefail

# Regenerates layered event SFX used by the client runtime.
# Requires ffmpeg 6+ with lavfi support.

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
cd "$ROOT_DIR"

echo "Generating bullet_whiz.wav"
ffmpeg -y \
  -f lavfi -i "sine=frequency=2100:duration=0.22:sample_rate=48000,afade=t=out:st=0.14:d=0.08,volume=0.32" \
  -f lavfi -i "anoisesrc=color=white:duration=0.22:sample_rate=48000,highpass=f=2200,lowpass=f=8200,afade=t=out:st=0.10:d=0.12,volume=0.09" \
  -filter_complex "[0:a][1:a]amix=inputs=2:normalize=0,acompressor=threshold=-20dB:ratio=2.8:attack=5:release=45,alimiter=limit=0.85" \
  -c:a pcm_s16le bullet_whiz.wav

echo "Generating dash_whoosh.wav"
ffmpeg -y \
  -f lavfi -i "anoisesrc=color=pink:duration=0.34:sample_rate=48000,highpass=f=180,lowpass=f=2200,volume=0.4" \
  -f lavfi -i "sine=frequency=120:duration=0.34:sample_rate=48000,afade=t=out:st=0.22:d=0.12,volume=0.17" \
  -filter_complex "[0:a]afade=t=in:st=0:d=0.015,afade=t=out:st=0.24:d=0.10[a0];[a0][1:a]amix=inputs=2:normalize=0,aecho=0.8:0.22:32:0.18,alimiter=limit=0.85" \
  -c:a pcm_s16le dash_whoosh.wav

echo "Generating dodge_whoosh.wav"
ffmpeg -y \
  -f lavfi -i "anoisesrc=color=white:duration=0.28:sample_rate=48000,highpass=f=450,lowpass=f=4300,volume=0.3" \
  -f lavfi -i "sine=frequency=260:duration=0.28:sample_rate=48000,afade=t=out:st=0.18:d=0.1,volume=0.12" \
  -filter_complex "[0:a]afade=t=in:st=0:d=0.01,afade=t=out:st=0.16:d=0.12[a0];[a0][1:a]amix=inputs=2:normalize=0,aecho=0.72:0.18:24:0.16,alimiter=limit=0.85" \
  -c:a pcm_s16le dodge_whoosh.wav

echo "Generating spawn_chime.wav"
ffmpeg -y \
  -f lavfi -i "sine=frequency=660:duration=0.52:sample_rate=48000,volume=0.23" \
  -f lavfi -i "sine=frequency=990:duration=0.52:sample_rate=48000,volume=0.18" \
  -f lavfi -i "sine=frequency=1320:duration=0.52:sample_rate=48000,volume=0.14" \
  -filter_complex "[0:a][1:a][2:a]amix=inputs=3:normalize=0,afade=t=in:st=0:d=0.01,afade=t=out:st=0.38:d=0.14,aecho=0.85:0.35:68:0.28,alimiter=limit=0.86" \
  -c:a pcm_s16le spawn_chime.wav

echo "Generating flag_fanfare.wav"
ffmpeg -y \
  -f lavfi -i "sine=frequency=740:duration=0.84:sample_rate=48000,volume=0.23" \
  -f lavfi -i "sine=frequency=988:duration=0.84:sample_rate=48000,volume=0.19" \
  -f lavfi -i "sine=frequency=1244:duration=0.84:sample_rate=48000,volume=0.15" \
  -f lavfi -i "anoisesrc=color=white:duration=0.84:sample_rate=48000,highpass=f=1800,lowpass=f=6000,volume=0.03" \
  -filter_complex "[0:a][1:a][2:a][3:a]amix=inputs=4:normalize=0,afade=t=in:st=0:d=0.01,afade=t=out:st=0.66:d=0.18,aecho=0.82:0.36:75:0.3,alimiter=limit=0.86" \
  -c:a pcm_s16le flag_fanfare.wav

echo "Done."
