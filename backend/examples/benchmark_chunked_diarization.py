#!/usr/bin/env python3
"""
Non-invasive benchmark for chunked parallel Deepgram diarization.

Purpose:
- Evaluate whether chunk + overlap + parallel calls can reduce wall time
  for long recordings (without changing production diarization path).

Example:
  python backend/examples/benchmark_chunked_diarization.py \
    --audio /path/to/meeting.wav \
    --chunk-minutes 10 \
    --overlap-seconds 30 \
    --concurrency 8
"""

from __future__ import annotations

import argparse
import asyncio
import json
import os
import tempfile
import time
from dataclasses import dataclass
from pathlib import Path
from typing import List
import re


@dataclass
class ChunkMeta:
    index: int
    start_sec: float
    end_sec: float
    path: Path


def _tokenize(text: str) -> set[str]:
    cleaned = re.sub(r"[^a-zA-Z0-9\s]", " ", (text or "").lower())
    return {w for w in cleaned.split() if len(w) > 2}


def _jaccard(a: set[str], b: set[str]) -> float:
    if not a or not b:
        return 0.0
    inter = len(a & b)
    union = len(a | b)
    return inter / union if union else 0.0


def _sec_to_hhmmss(seconds: float) -> str:
    h = int(seconds // 3600)
    m = int((seconds % 3600) // 60)
    s = seconds % 60
    return f"{h:02d}:{m:02d}:{s:06.3f}"


async def _run_cmd(*cmd: str) -> str:
    proc = await asyncio.create_subprocess_exec(
        *cmd,
        stdout=asyncio.subprocess.PIPE,
        stderr=asyncio.subprocess.PIPE,
    )
    out, err = await proc.communicate()
    if proc.returncode != 0:
        raise RuntimeError(
            f"Command failed ({proc.returncode}): {' '.join(cmd)}\n{err.decode('utf-8', errors='ignore')}"
        )
    return out.decode("utf-8", errors="ignore").strip()


async def _probe_duration_seconds(audio_path: Path) -> float:
    output = await _run_cmd(
        "ffprobe",
        "-v",
        "error",
        "-show_entries",
        "format=duration",
        "-of",
        "default=noprint_wrappers=1:nokey=1",
        str(audio_path),
    )
    return float(output)


async def _make_chunk(
    source: Path, target: Path, start_sec: float, duration_sec: float
) -> None:
    await _run_cmd(
        "ffmpeg",
        "-y",
        "-v",
        "error",
        "-ss",
        str(start_sec),
        "-t",
        str(duration_sec),
        "-i",
        str(source),
        "-ac",
        "1",
        "-ar",
        "16000",
        "-c:a",
        "pcm_s16le",
        str(target),
    )


async def _build_chunks(
    source: Path, chunk_minutes: float, overlap_seconds: float, workdir: Path
) -> List[ChunkMeta]:
    duration = await _probe_duration_seconds(source)
    chunk_sec = chunk_minutes * 60.0
    step = max(1.0, chunk_sec - overlap_seconds)

    chunks: List[ChunkMeta] = []
    start = 0.0
    idx = 0
    while start < duration:
        end = min(duration, start + chunk_sec)
        path = workdir / f"chunk_{idx:04d}.wav"
        await _make_chunk(source, path, start, max(0.1, end - start))
        chunks.append(ChunkMeta(index=idx, start_sec=start, end_sec=end, path=path))
        idx += 1
        start += step

    return chunks


async def _diarize_chunk(
    chunk: ChunkMeta,
    api_key: str,
):
    # Import here so this script remains standalone and non-invasive.
    try:
        from app.services.audio.diarization import DiarizationService
    except (ImportError, ValueError):
        from services.audio.diarization import DiarizationService

    service = DiarizationService(provider="deepgram")
    data = chunk.path.read_bytes()
    t0 = time.perf_counter()
    segments = await service._diarize_with_deepgram(
        audio_data=data,
        meeting_id=f"benchmark-chunk-{chunk.index}",
        api_key=api_key,
    )
    t1 = time.perf_counter()
    return chunk, segments, (t1 - t0)


def _stitch_segments(chunk_results):
    stitched = []
    for chunk, segments, _ in sorted(chunk_results, key=lambda x: x[0].index):
        for seg in segments:
            shifted = {
                "speaker": seg.speaker,
                "local_speaker": seg.speaker,
                "chunk_index": chunk.index,
                "start": seg.start_time + chunk.start_sec,
                "end": seg.end_time + chunk.start_sec,
                "text": seg.text,
            }
            stitched.append(shifted)

    stitched.sort(key=lambda x: x["start"])
    # Basic overlap dedupe: keep non-empty forward-progress segments.
    deduped = []
    last_end = -1.0
    for seg in stitched:
        if seg["end"] <= last_end:
            continue
        if seg["start"] < last_end:
            seg["start"] = last_end
        if seg["end"] - seg["start"] < 0.05:
            continue
        deduped.append(seg)
        last_end = seg["end"]
    return deduped


def _build_overlap_text(
    segments: list[dict], start: float, end: float, speaker: str
) -> str:
    parts = []
    for seg in segments:
        if seg["speaker"] != speaker:
            continue
        if seg["end"] <= start or seg["start"] >= end:
            continue
        parts.append(seg.get("text", ""))
    return " ".join(parts).strip()


def _reconcile_speakers(
    chunk_results,
    stitched_segments: list[dict],
    overlap_seconds: float,
    similarity_threshold: float = 0.08,
):
    """
    Map chunk-local speaker labels to global speaker IDs using overlap text continuity.
    This is a lightweight heuristic for PoC benchmarking.
    """
    chunks = sorted((c for c, _, _ in chunk_results), key=lambda c: c.index)
    if not chunks:
        return stitched_segments, []

    # Group by chunk index
    by_chunk: dict[int, list[dict]] = {}
    for seg in stitched_segments:
        by_chunk.setdefault(seg["chunk_index"], []).append(seg)

    mapping_rows = []
    next_global_id = 0
    global_names: list[str] = []
    chunk_local_to_global: dict[tuple[int, str], str] = {}

    # First chunk initializes global speaker map
    first_chunk_segments = by_chunk.get(chunks[0].index, [])
    first_locals = sorted({s["local_speaker"] for s in first_chunk_segments})
    for local in first_locals:
        gname = f"Speaker {next_global_id}"
        next_global_id += 1
        global_names.append(gname)
        chunk_local_to_global[(chunks[0].index, local)] = gname
        mapping_rows.append(
            {
                "chunk_index": chunks[0].index,
                "local_speaker": local,
                "global_speaker": gname,
                "confidence": 1.0,
                "strategy": "init",
            }
        )

    # Reconcile subsequent chunks against previous chunk overlap
    for i in range(1, len(chunks)):
        cur = chunks[i]
        prev = chunks[i - 1]
        cur_segments = by_chunk.get(cur.index, [])
        prev_segments = by_chunk.get(prev.index, [])
        cur_locals = sorted({s["local_speaker"] for s in cur_segments})

        # Overlap window around current chunk start
        overlap_start = max(0.0, cur.start_sec - overlap_seconds)
        overlap_end = cur.start_sec + overlap_seconds
        assigned_globals = set()

        for local in cur_locals:
            cur_text = _build_overlap_text(cur_segments, overlap_start, overlap_end, local)
            cur_tokens = _tokenize(cur_text)

            best_global = None
            best_score = 0.0
            for prev_local in sorted({s["local_speaker"] for s in prev_segments}):
                prev_global = chunk_local_to_global.get((prev.index, prev_local))
                if not prev_global or prev_global in assigned_globals:
                    continue
                prev_text = _build_overlap_text(
                    prev_segments, overlap_start, overlap_end, prev_local
                )
                prev_tokens = _tokenize(prev_text)
                score = _jaccard(cur_tokens, prev_tokens)
                if score > best_score:
                    best_score = score
                    best_global = prev_global

            if best_global and best_score >= similarity_threshold:
                mapped_global = best_global
                strategy = "overlap_text"
                confidence = round(best_score, 4)
            else:
                mapped_global = f"Speaker {next_global_id}"
                next_global_id += 1
                global_names.append(mapped_global)
                strategy = "new_global"
                confidence = round(best_score, 4)

            assigned_globals.add(mapped_global)
            chunk_local_to_global[(cur.index, local)] = mapped_global
            mapping_rows.append(
                {
                    "chunk_index": cur.index,
                    "local_speaker": local,
                    "global_speaker": mapped_global,
                    "confidence": confidence,
                    "strategy": strategy,
                }
            )

    reconciled = []
    for seg in stitched_segments:
        g = chunk_local_to_global.get((seg["chunk_index"], seg["local_speaker"]))
        rec = dict(seg)
        rec["speaker"] = g or seg["speaker"]
        reconciled.append(rec)

    return reconciled, mapping_rows


async def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--audio", required=True, help="Path to source audio file")
    parser.add_argument("--chunk-minutes", type=float, default=10.0)
    parser.add_argument("--overlap-seconds", type=float, default=30.0)
    parser.add_argument("--concurrency", type=int, default=8)
    parser.add_argument(
        "--out-json",
        default="diarization_benchmark_output.json",
        help="Path to write stitched diarization JSON output",
    )
    parser.add_argument(
        "--preview",
        type=int,
        default=20,
        help="Number of stitched segments to print in preview",
    )
    parser.add_argument(
        "--reconcile-speakers",
        action="store_true",
        help="Apply chunk-to-global speaker mapping using overlap text continuity",
    )
    args = parser.parse_args()

    api_key = "e28812c8f408ba940ededc9e9656584eea9b8cda"
    if not api_key:
        raise SystemExit("DEEPGRAM_API_KEY is required in environment")

    audio_path = Path(args.audio).expanduser().resolve()
    if not audio_path.exists():
        raise SystemExit(f"Audio file not found: {audio_path}")

    total_start = time.perf_counter()
    with tempfile.TemporaryDirectory(prefix="dg-benchmark-") as td:
        workdir = Path(td)
        chunks = await _build_chunks(
            source=audio_path,
            chunk_minutes=args.chunk_minutes,
            overlap_seconds=args.overlap_seconds,
            workdir=workdir,
        )

        print(
            f"Prepared {len(chunks)} chunks "
            f"(chunk={args.chunk_minutes}m overlap={args.overlap_seconds}s concurrency={args.concurrency})"
        )

        sem = asyncio.Semaphore(max(1, args.concurrency))

        async def runner(chunk: ChunkMeta):
            async with sem:
                return await _diarize_chunk(chunk, api_key)

        chunk_results = await asyncio.gather(*(runner(c) for c in chunks))
        stitched_raw = _stitch_segments(chunk_results)
        if args.reconcile_speakers:
            stitched, speaker_mapping = _reconcile_speakers(
                chunk_results=chunk_results,
                stitched_segments=stitched_raw,
                overlap_seconds=args.overlap_seconds,
            )
        else:
            stitched = stitched_raw
            speaker_mapping = []

        total_end = time.perf_counter()

        per_chunk = [t for _, _, t in chunk_results]
        print("\n=== Benchmark Result ===")
        print(f"Wall time: {total_end - total_start:.2f}s")
        print(f"Chunk API avg: {sum(per_chunk)/len(per_chunk):.2f}s")
        print(f"Chunk API max: {max(per_chunk):.2f}s")
        print(f"Chunk API min: {min(per_chunk):.2f}s")
        print(f"Stitched segments: {len(stitched)}")
        if stitched:
            print(
                "Timeline: "
                f"{_sec_to_hhmmss(stitched[0]['start'])} -> {_sec_to_hhmmss(stitched[-1]['end'])}"
            )

        output_payload = {
            "input_audio": str(audio_path),
            "chunk_minutes": args.chunk_minutes,
            "overlap_seconds": args.overlap_seconds,
            "concurrency": args.concurrency,
            "wall_time_seconds": total_end - total_start,
            "chunk_latency_seconds": {
                "avg": sum(per_chunk) / len(per_chunk),
                "max": max(per_chunk),
                "min": min(per_chunk),
            },
            "speaker_reconciliation_enabled": bool(args.reconcile_speakers),
            "speaker_mapping_count": len(speaker_mapping),
            "speaker_mapping": speaker_mapping,
            "segments_raw": stitched_raw,
            "stitched_segment_count": len(stitched),
            "segments": stitched,
        }

        out_path = Path(args.out_json).expanduser().resolve()
        out_path.write_text(json.dumps(output_payload, indent=2), encoding="utf-8")
        print(f"Saved stitched output: {out_path}")

        preview_n = max(0, int(args.preview))
        if preview_n > 0 and stitched:
            print(f"\n=== Segment Preview (first {min(preview_n, len(stitched))}) ===")
            for seg in stitched[:preview_n]:
                print(
                    f"[{_sec_to_hhmmss(seg['start'])} -> {_sec_to_hhmmss(seg['end'])}] "
                    f"{seg['speaker']}: {seg['text']}"
                )


if __name__ == "__main__":
    asyncio.run(main())
