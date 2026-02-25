#!/usr/bin/env python3
"""
Benchmark overlap merge strategies for streaming transcript hypotheses.

Input formats:
1) JSON array of strings:
   ["hello team today we discuss q1", "today we discuss q1 targets and owners", ...]
2) JSON array of objects:
   [{"hypothesis": "...", "reference_delta": "..."}, ...]
3) JSONL with one object per line using the same object shape.

Usage:
  PYTHONPATH=. python backend/examples/benchmark_streaming_overlap_merge.py \
    --input path/to/hypotheses.json \
    --out-json overlap_merge_benchmark.json
"""

from __future__ import annotations

import argparse
import json
import time
from pathlib import Path
from typing import Any, Dict, List, Tuple


def word_similarity(words1: List[str], words2: List[str]) -> float:
    if not words1 or not words2:
        return 0.0
    set1 = set(w.lower() for w in words1)
    set2 = set(w.lower() for w in words2)
    intersection = len(set1 & set2)
    union = len(set1 | set2)
    return intersection / union if union > 0 else 0.0


def remove_overlap_heuristic(last_final_text: str, new_text: str) -> Tuple[str, int]:
    if not last_final_text:
        return new_text, 0
    final_words = last_final_text.split()
    new_words = new_text.split()
    if len(new_words) < 4:
        return new_text, 0

    max_overlap_check = min(20, len(new_words) // 2 + 5)
    search_window = min(50, len(final_words))
    final_tail_words = final_words[-search_window:]
    best_overlap = 0
    similarity_threshold = 0.5
    for overlap_size in range(max_overlap_check, 2, -1):
        new_head = new_words[:overlap_size]
        for start in range(0, min(15, search_window)):
            if start + overlap_size > search_window:
                break
            final_segment = final_tail_words[start : start + overlap_size]
            similarity = word_similarity(new_head, final_segment)
            if similarity >= similarity_threshold:
                best_overlap = max(best_overlap, overlap_size)
                break
        if overlap_size <= search_window:
            final_end = final_tail_words[-overlap_size:]
            similarity = word_similarity(new_head, final_end)
            if similarity >= similarity_threshold:
                best_overlap = max(best_overlap, overlap_size)
        if best_overlap > 0:
            break

    if best_overlap > 0:
        return " ".join(new_words[best_overlap:]).strip(), best_overlap
    return new_text, 0


def normalize_tokens(text: str) -> List[str]:
    return [w.lower().strip(".,!?;:\"'()[]{}") for w in text.split() if w.strip()]


def levenshtein_distance(a: List[str], b: List[str]) -> int:
    if not a:
        return len(b)
    if not b:
        return len(a)
    prev = list(range(len(b) + 1))
    for i, ta in enumerate(a, start=1):
        cur = [i]
        for j, tb in enumerate(b, start=1):
            ins = cur[j - 1] + 1
            delete = prev[j] + 1
            sub = prev[j - 1] + (0 if ta == tb else 1)
            cur.append(min(ins, delete, sub))
        prev = cur
    return prev[-1]


def best_alignment_overlap(
    last_final_text: str, new_text: str, min_overlap_tokens: int, min_score: float
) -> Tuple[int, float]:
    prev_tokens = normalize_tokens(last_final_text)
    new_tokens = normalize_tokens(new_text)
    if len(prev_tokens) < min_overlap_tokens or len(new_tokens) < min_overlap_tokens:
        return 0, 0.0
    tail_window = prev_tokens[-min(60, len(prev_tokens)) :]
    max_overlap = min(len(tail_window), len(new_tokens), 24)
    best_overlap = 0
    best_score = 0.0
    for overlap_size in range(max_overlap, min_overlap_tokens - 1, -1):
        prev_suffix = tail_window[-overlap_size:]
        new_prefix = new_tokens[:overlap_size]
        dist = levenshtein_distance(prev_suffix, new_prefix)
        score = 1.0 - (dist / max(overlap_size, 1))
        if score > best_score:
            best_score = score
            best_overlap = overlap_size
        if score >= min_score:
            return overlap_size, score
    return best_overlap, best_score


def remove_overlap_alignment(
    last_final_text: str, new_text: str, min_overlap_tokens: int, min_score: float
) -> Tuple[str, int, float]:
    if not last_final_text:
        return new_text, 0, 0.0
    overlap, score = best_alignment_overlap(
        last_final_text, new_text, min_overlap_tokens=min_overlap_tokens, min_score=min_score
    )
    if overlap >= min_overlap_tokens and score >= min_score:
        words = new_text.split()
        if overlap < len(words):
            return " ".join(words[overlap:]).strip(), overlap, score
        return "", overlap, score
    heuristic_text, heuristic_overlap = remove_overlap_heuristic(last_final_text, new_text)
    return heuristic_text, heuristic_overlap, score


def token_f1(pred: str, ref: str) -> float:
    p = normalize_tokens(pred)
    r = normalize_tokens(ref)
    if not p and not r:
        return 1.0
    if not p or not r:
        return 0.0
    p_set = set(p)
    r_set = set(r)
    tp = len(p_set & r_set)
    precision = tp / len(p_set) if p_set else 0.0
    recall = tp / len(r_set) if r_set else 0.0
    if precision + recall == 0:
        return 0.0
    return 2 * precision * recall / (precision + recall)


def load_items(path: Path) -> List[Dict[str, Any]]:
    raw = path.read_text(encoding="utf-8")
    if path.suffix.lower() == ".jsonl":
        items: List[Dict[str, Any]] = []
        for line in raw.splitlines():
            line = line.strip()
            if not line:
                continue
            obj = json.loads(line)
            items.append(
                {
                    "hypothesis": str(obj.get("hypothesis", "")).strip(),
                    "reference_delta": str(obj.get("reference_delta", "")).strip()
                    if obj.get("reference_delta")
                    else None,
                }
            )
        return items

    parsed = json.loads(raw)
    items = []
    if isinstance(parsed, list):
        for entry in parsed:
            if isinstance(entry, str):
                items.append({"hypothesis": entry.strip(), "reference_delta": None})
            elif isinstance(entry, dict):
                items.append(
                    {
                        "hypothesis": str(entry.get("hypothesis", "")).strip(),
                        "reference_delta": str(entry.get("reference_delta", "")).strip()
                        if entry.get("reference_delta")
                        else None,
                    }
                )
    return items


def run_strategy(items: List[Dict[str, Any]], strategy: str, min_overlap: int, min_score: float) -> Dict[str, Any]:
    last_final = ""
    emitted: List[str] = []
    duplicate_like = 0
    total_overlap_removed = 0
    f1_scores: List[float] = []

    t0 = time.perf_counter()
    for item in items:
        hyp = item["hypothesis"]
        if not hyp:
            continue
        if strategy == "heuristic":
            deduped, overlap = remove_overlap_heuristic(last_final, hyp)
            total_overlap_removed += overlap
        else:
            deduped, overlap, _ = remove_overlap_alignment(
                last_final, hyp, min_overlap_tokens=min_overlap, min_score=min_score
            )
            total_overlap_removed += overlap

        if not deduped or len(deduped.split()) < 2:
            duplicate_like += 1
            continue
        emitted.append(deduped)
        if item.get("reference_delta"):
            f1_scores.append(token_f1(deduped, item["reference_delta"]))
        last_final = (last_final + " " + deduped).strip()
    elapsed = time.perf_counter() - t0

    return {
        "strategy": strategy,
        "input_items": len(items),
        "emitted_segments": len(emitted),
        "suppressed_segments": duplicate_like,
        "avg_overlap_tokens_removed": (
            total_overlap_removed / max(len(items), 1)
        ),
        "processing_time_ms": elapsed * 1000.0,
        "reference_token_f1_avg": (
            sum(f1_scores) / len(f1_scores) if f1_scores else None
        ),
        "final_text_word_count": len(" ".join(emitted).split()),
        "preview_tail": " ".join(emitted)[-240:],
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--input", required=True, help="Path to JSON/JSONL hypotheses file")
    parser.add_argument(
        "--out-json",
        default="overlap_merge_benchmark.json",
        help="Output report path",
    )
    parser.add_argument("--alignment-min-overlap", type=int, default=3)
    parser.add_argument("--alignment-min-score", type=float, default=0.62)
    args = parser.parse_args()

    path = Path(args.input).expanduser().resolve()
    if not path.exists():
        raise SystemExit(f"Input file not found: {path}")

    items = load_items(path)
    if not items:
        raise SystemExit("No benchmark items found in input file.")

    heuristic = run_strategy(
        items, "heuristic", min_overlap=args.alignment_min_overlap, min_score=args.alignment_min_score
    )
    alignment = run_strategy(
        items, "alignment", min_overlap=args.alignment_min_overlap, min_score=args.alignment_min_score
    )
    report = {
        "input_file": str(path),
        "alignment_min_overlap": args.alignment_min_overlap,
        "alignment_min_score": args.alignment_min_score,
        "heuristic": heuristic,
        "alignment_first": alignment,
    }

    out_path = Path(args.out_json).expanduser().resolve()
    out_path.write_text(json.dumps(report, indent=2), encoding="utf-8")
    print(json.dumps(report, indent=2))
    print(f"\nSaved benchmark report: {out_path}")


if __name__ == "__main__":
    main()

