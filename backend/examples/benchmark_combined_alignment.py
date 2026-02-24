#!/usr/bin/env python3
"""
Non-invasive benchmark for combined Groq+Deepgram alignment quality.

What it does:
1) Runs Groq full transcription (segment timeline).
2) Runs Deepgram diarization (speaker timeline).
3) Aligns Groq segments to speakers (raw).
4) Compacts Groq micro-segments.
5) Aligns again (compacted) and compares metrics.

Usage:
  PYTHONPATH=. python examples/benchmark_combined_alignment.py \
    --audio recording.wav \
    --out-json combined_alignment_benchmark.json
"""

from __future__ import annotations

import argparse
import asyncio
import json
import os
import time
from pathlib import Path
from typing import Dict, List


def _word_count(text: str) -> int:
    return len((text or "").strip().split())


def compact_transcript_segments(
    segments: List[Dict],
    max_gap_seconds: float = 0.4,
    max_duration_seconds: float = 8.0,
    min_segment_seconds: float = 0.6,
    min_words: int = 3,
) -> List[Dict]:
    """
    Merge micro-segments to reduce alignment fragmentation.
    """
    if not segments:
        return []

    normalized = []
    for s in segments:
        start = float(s.get("start", 0.0))
        end = float(s.get("end", start))
        text = (s.get("text", "") or "").strip()
        if end <= start:
            continue
        normalized.append({"start": start, "end": end, "text": text})

    normalized.sort(key=lambda x: x["start"])
    if not normalized:
        return []

    merged: List[Dict] = []
    current = dict(normalized[0])

    def flush():
        if current["end"] > current["start"]:
            merged.append(dict(current))

    for nxt in normalized[1:]:
        cur_duration = current["end"] - current["start"]
        nxt_duration = nxt["end"] - nxt["start"]
        gap = nxt["start"] - current["end"]
        combined_duration = nxt["end"] - current["start"]
        cur_words = _word_count(current["text"])
        nxt_words = _word_count(nxt["text"])

        micro_cur = cur_duration < min_segment_seconds or cur_words < min_words
        micro_nxt = nxt_duration < min_segment_seconds or nxt_words < min_words

        should_merge = False
        if gap <= max_gap_seconds and combined_duration <= max_duration_seconds:
            should_merge = True
        if gap <= max_gap_seconds and (micro_cur or micro_nxt):
            should_merge = True

        if should_merge:
            current["end"] = max(current["end"], nxt["end"])
            if current["text"] and nxt["text"]:
                current["text"] = f"{current['text']} {nxt['text']}".strip()
            elif nxt["text"]:
                current["text"] = nxt["text"]
        else:
            flush()
            current = dict(nxt)

    flush()
    return merged


async def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--audio", required=True, help="Path to WAV recording")
    parser.add_argument(
        "--out-json",
        default="combined_alignment_benchmark.json",
        help="Output report path",
    )
    parser.add_argument("--max-gap-seconds", type=float, default=0.4)
    parser.add_argument("--max-duration-seconds", type=float, default=8.0)
    parser.add_argument("--min-segment-seconds", type=float, default=0.6)
    parser.add_argument("--min-words", type=int, default=3)
    args = parser.parse_args()

    audio_path = Path(args.audio).expanduser().resolve()
    if not audio_path.exists():
        raise SystemExit(f"Audio file not found: {audio_path}")

    deepgram_key = os.getenv("DEEPGRAM_API_KEY")
    groq_key = os.getenv("GROQ_API_KEY")
    if not deepgram_key:
        raise SystemExit("Missing DEEPGRAM_API_KEY")
    if not groq_key:
        raise SystemExit("Missing GROQ_API_KEY")

    audio_data = audio_path.read_bytes()
    if not audio_data.startswith(b"RIFF"):
        raise SystemExit(
            "This benchmark expects WAV input for parity with production parallel diarization path."
        )

    try:
        from app.services.audio.diarization import DiarizationService
        from app.services.audio.alignment import AlignmentEngine
    except (ImportError, ValueError):
        from services.audio.diarization import DiarizationService
        from services.audio.alignment import AlignmentEngine

    # Ensure API keys are discoverable in service internals.
    os.environ["DEEPGRAM_API_KEY"] = deepgram_key
    os.environ["GROQ_API_KEY"] = groq_key

    diarization_service = DiarizationService(provider="deepgram", groq_api_key=groq_key)
    alignment_engine = AlignmentEngine()

    t0 = time.perf_counter()
    groq_task = asyncio.create_task(diarization_service.transcribe_with_whisper(audio_data))
    diar_task = asyncio.create_task(
        diarization_service.diarize_meeting(
            meeting_id="benchmark-combined",
            provider="deepgram",
            audio_data=audio_data,
            audio_url=None,
            user_email=None,
        )
    )
    groq_segments, diar_result = await asyncio.gather(groq_task, diar_task)
    t1 = time.perf_counter()

    if diar_result.status != "completed":
        raise SystemExit(f"Diarization failed: {diar_result.error}")

    speaker_segments = [
        {
            "speaker": s.speaker,
            "start_time": s.start_time,
            "end_time": s.end_time,
            "text": s.text,
            "confidence": s.confidence,
        }
        for s in diar_result.segments
    ]

    raw_input_segments = [
        {"start": s.get("start", 0.0), "end": s.get("end", 0.0), "text": s.get("text", "")}
        for s in (groq_segments or [])
        if s.get("end", 0.0) > s.get("start", 0.0)
    ]

    aligned_raw, metrics_raw = alignment_engine.align_batch(raw_input_segments, speaker_segments)

    compacted = compact_transcript_segments(
        raw_input_segments,
        max_gap_seconds=args.max_gap_seconds,
        max_duration_seconds=args.max_duration_seconds,
        min_segment_seconds=args.min_segment_seconds,
        min_words=args.min_words,
    )
    aligned_compact, metrics_compact = alignment_engine.align_batch(compacted, speaker_segments)
    t2 = time.perf_counter()

    result = {
        "input_audio": str(audio_path),
        "timing_seconds": {
            "groq_plus_deepgram_parallel": t1 - t0,
            "alignment_and_compaction": t2 - t1,
            "total": t2 - t0,
        },
        "counts": {
            "deepgram_speaker_segments": len(speaker_segments),
            "groq_segments_raw": len(raw_input_segments),
            "groq_segments_compacted": len(compacted),
        },
        "metrics_raw": metrics_raw,
        "metrics_compacted": metrics_compact,
        "sample": {
            "raw_aligned_first_20": aligned_raw[:20],
            "compacted_aligned_first_20": aligned_compact[:20],
        },
        "compaction_params": {
            "max_gap_seconds": args.max_gap_seconds,
            "max_duration_seconds": args.max_duration_seconds,
            "min_segment_seconds": args.min_segment_seconds,
            "min_words": args.min_words,
        },
    }

    out_path = Path(args.out_json).expanduser().resolve()
    out_path.write_text(json.dumps(result, indent=2), encoding="utf-8")

    print("=== Combined Benchmark ===")
    print(f"Deepgram speaker segments: {len(speaker_segments)}")
    print(f"Groq raw segments: {len(raw_input_segments)}")
    print(f"Groq compacted segments: {len(compacted)}")
    print(
        "Raw confident/total:",
        f"{metrics_raw.get('confident_count', 0)}/{metrics_raw.get('total_segments', 0)}",
    )
    print(
        "Compacted confident/total:",
        f"{metrics_compact.get('confident_count', 0)}/{metrics_compact.get('total_segments', 0)}",
    )
    print(f"Saved report: {out_path}")


if __name__ == "__main__":
    asyncio.run(main())
