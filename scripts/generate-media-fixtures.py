#!/usr/bin/env python3
"""Generate original synthetic fixtures locally; downloads no copyrighted assets."""
import math
import pathlib
import shutil
import struct
import subprocess
import sys
import wave

root = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else "target/media-fixtures")
root.mkdir(parents=True, exist_ok=True)
ffmpeg = shutil.which("ffmpeg")
if not ffmpeg:
    raise SystemExit("Install FFmpeg from https://ffmpeg.org/download.html first")
rate, duration = 24000, 6
wav = root / "tone.wav"
with wave.open(str(wav), "wb") as out:
    out.setnchannels(1)
    out.setsampwidth(2)
    out.setframerate(rate)
    samples = (int(1600 * math.sin(2 * math.pi * 440 * i / rate)) for i in range(rate * duration))
    out.writeframes(b"".join(struct.pack("<h", sample) for sample in samples))
video = root / "test-pattern.mkv"
subprocess.run([
    ffmpeg, "-hide_banner", "-v", "error", "-f", "lavfi", "-i",
    "testsrc=size=320x180:rate=24", "-i", str(wav.resolve()), "-t", str(duration),
    "-c:v", "ffv1", "-threads", "1", "-c:a", "pcm_s16le", "-y", str(video.resolve()),
], check=True, timeout=30)
print(f"Generated {wav.resolve()} and {video.resolve()}")
