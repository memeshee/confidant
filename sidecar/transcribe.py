#!/usr/bin/env python3
"""Local speech-to-text for Confidant. CPU only, no network, no cloud.

Usage: transcribe.py <audio-file> [model-size]
Prints one JSON object on stdout: {"text":..., "ms":..., "model":..., "segments":N}

Runs faster-whisper. The caller deletes the audio file; this script never writes
a copy anywhere, and model weights are read from the local HF cache.
"""
import json
import os
import sys
import time


def main() -> int:
    if len(sys.argv) < 2:
        print(json.dumps({"error": "usage: transcribe.py <audio> [model]"}))
        return 2
    audio = sys.argv[1]
    model_size = sys.argv[2] if len(sys.argv) > 2 else os.environ.get("CONFIDANT_STT_MODEL", "small")
    started = time.time()
    try:
        from faster_whisper import WhisperModel
    except Exception as exc:  # pragma: no cover - environment problem, reported as JSON
        print(json.dumps({"error": f"faster-whisper unavailable: {exc}"}))
        return 3

    model = WhisperModel(model_size, device="cpu", compute_type="int8", cpu_threads=os.cpu_count() or 4)
    segments, _info = model.transcribe(audio, language=None, vad_filter=True, beam_size=1)
    parts = []
    for seg in segments:
        text = seg.text.strip()
        if text:
            parts.append(text)
    transcript = " ".join(parts).strip()
    print(json.dumps({
        "text": transcript,
        "ms": int((time.time() - started) * 1000),
        "model": f"faster-whisper:{model_size}",
        "segments": len(parts),
    }))
    return 0


if __name__ == "__main__":
    sys.exit(main())
