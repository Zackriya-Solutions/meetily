# Pnyx Real-Time Transcription and Meeting Cognition Strategy

## Executive Summary

Pnyx should optimize for reliable, semantically stable transcript units that power reasoning during live meetings, not for word-by-word caption speed.
The target experience is:

- Stable transcript segments in 5-8 seconds
- Rare latency excursions above 10 seconds
- Low drift and low correction churn
- Transcript quality reliable enough for live Q&A, catch-up, and downstream notes

This strategy evolves the current pipeline from heuristic real-time dedup into a confidence-aware, alignment-driven transcript engine with explicit stability gates.

## Problem Definition

Current behavior provides useful live transcript output, but still has failure modes that affect cognition use cases:

- Chunk boundary drift and overlap inconsistency
- Over-reliance on heuristic dedup when reprocessing overlapping windows
- Unstable segments sometimes reaching downstream AI too early
- Reconnect/network disruptions causing timeline instability and quality variance
- Limited online quality observability during active meetings

The current architecture is directionally correct (rolling overlap, smart triggers, final-only output), but we need stronger alignment, scoring, and quality controls.

## Core Product Philosophy

- Pnyx is a meeting cognition engine, not a caption ticker.
- Accuracy and semantic completeness are higher priority than earliest possible token display.
- Finalized transcript units must be trustworthy enough for mid-meeting AI reasoning.
- Speed is important, but only to the point where transcript quality remains stable.

## What We Are Optimizing For

1. Stable sentence-level transcript units
2. High context reliability for downstream AI features
3. Low drift across overlapping windows and chunk boundaries
4. User trust through cognitive clarity during meetings
5. Predictable latency envelope (5-8s target, <10s sustained)

## Target Architecture Improvements

### 1) Overlap Alignment Strategy

Move from heuristic overlap trimming alone to alignment-first merging:

- Build overlap candidate region between previous finalized tail and new window hypothesis
- Run token-level alignment (Levenshtein/edit-distance or dynamic programming alignment)
- Keep high-confidence consensus span
- Emit only net-new stable span

Why:

- Reduces repeated text and semantic jumps
- Handles minor wording shifts in re-transcribed overlap better than hash/ngram heuristics alone

### 2) Rolling Buffer Logic Improvements

- Keep adaptive window policy:
  - Base window: 5-6s
  - Expand to 7-8s when speech density is high or topic boundary is unclear
  - Shrink near clear sentence boundaries
- Preserve overlap ratio guardrail (for example 30-50%) rather than static-only behavior
- Explicitly track speech-density and speech-run duration as trigger inputs

Why:

- Better WER/context trade-off across different speaking styles
- Fewer abrupt boundary cuts in fast discussions

### 3) Silence-Triggered Buffer Flushing

- Maintain silence-triggered finalization
- Add minimum-content guard:
  - If detected text is low-information or unstable, delay emission and wait one more overlap cycle
- Add forced flush on sustained silence with confidence threshold

Why:

- Avoids premature weak segments
- Keeps latency bounded when meeting naturally pauses

### 4) Stability Scoring Before Emission

Introduce a segment stability score (0-1) computed from:

- Alignment consistency across overlap cycles
- Per-token or segment confidence proxy
- Repetition/duplication risk
- Hallucination risk
- Boundary completeness score (punctuation + syntactic closure)

Emit rule:

- Emit to "stable transcript stream" only when stability score crosses threshold
- Keep below-threshold candidates in "volatile buffer" (internal only)

Why:

- Prevents unstable text from contaminating live Q&A context

### 5) Semantic Boundary Detection

- Keep current punctuation/silence triggers
- Add boundary classifier signals:
  - sentence closure confidence
  - discourse connector transitions
  - abrupt topic shift hints
- Finalize at sentence-level whenever possible, not arbitrary time slices

Why:

- Improves readability and semantic coherence for AI consumers

## Latency Strategy

## Latency Principles

- We optimize for "usable stable segment latency," not first raw token latency.
- Every latency reduction experiment must be evaluated against semantic stability regression.

## Configuration Philosophy

- Start with window ~6s and slide ~2s baseline
- Evaluate 4-8s window range experimentally
- Maintain overlap for contextual refinement, but cap overlap reprocessing cost

## Trade-Offs

- Smaller window:
  - lower latency
  - higher drift/WER risk
- Larger window:
  - better context accuracy
  - slower emission and higher compute cost
- More overlap:
  - better correction potential
  - more dedup/merge complexity

## Benchmark Plan

Run offline + shadow-online experiments with representative meeting audio:

- Window sizes: 4s, 5s, 6s, 7s, 8s
- Slides: 1s, 2s, 3s
- Overlap ratios: 25%, 35%, 50%
- Compare current heuristic merge vs alignment-based merge

Select configuration by Pareto frontier (quality vs latency), not a single metric.

## Metrics to Track

- First transcript latency (first stable segment)
- Finalization latency (segment close to emit)
- Correction rate (how often previous meaning changes)
- Semantic drift frequency
- Duplicate emission rate
- Hallucination incidence

## Transcript Quality Strategy

### Hallucination Detection

- Keep rule-based blacklist filters
- Add model-behavior signatures:
  - repetitive phrase loops
  - out-of-domain canned patterns
- Score and quarantine suspicious candidates before stable emission

### Duplicate Suppression

- Replace heuristic-only dedup with alignment-backed delta extraction
- Retain hash/ngram checks as secondary guardrails

### Confidence Scoring

- Build composite confidence:
  - model confidence proxy
  - overlap agreement score
  - boundary completeness
  - anomaly penalties

### Context Window Integrity

- Store transcript as ordered stable segments with timestamps and confidence metadata
- Never feed volatile candidates directly into reasoning context

## AI Context Readiness

Structure transcript output for live intelligence:

- Stable Segment Object:
  - `segment_id`
  - `start_time`
  - `end_time`
  - `text`
  - `stability_score`
  - `confidence_score`
  - `revision_generation`
  - `topic_id`

- Context views:
  - Last N seconds stable-only
  - Topic-bounded recent context
  - Meeting-global stable context

Use cases:

- Mid-meeting Q&A reads stable-only stream
- Catch-up summarizes last N seconds from stable segments
- Topic shifts handled by topic_id segmentation to reduce cross-topic contamination

## Reliability and Production Hardening

### Monitoring

Real-time dashboards and alerts for:

- WebSocket reconnect rate
- Heartbeat timeout rate
- Stable-segment latency p50/p95
- Drift/correction anomalies
- Hallucination and duplicate rates

### Drift and Instability Detection

- Alert when correction rate crosses threshold
- Alert when stability score distribution collapses below target
- Enable adaptive fallback (slightly larger window) under instability

### Reconnect Edge Cases

- Keep monotonic timeline normalization across reconnects
- Preserve session-level overlap memory on short reconnects
- Distinguish transport reconnect from logical new meeting segment

### Backpressure and Scale

- Queue depth guardrails and drop policies with telemetry
- Throttle expensive re-alignment under overload
- Prioritize stable finalization path over optional enrichments during pressure

## Prioritized Improvement Roadmap

## Immediate (0-2 weeks)

1. Add explicit stable vs volatile transcript separation
2. Introduce segment stability scoring v1 (simple weighted rule model)
3. Add production metrics for latency, drift, correction, duplicates
4. Ensure mid-meeting AI reads stable-only context

Technical rationale:

- Fastest path to reduce bad context leakage without full algorithm rewrite

Expected impact:

- Better Q&A reliability and lower visible transcript churn

## Short-Term (2-6 weeks)

1. Implement alignment-based overlap merge (edit-distance/token alignment)
2. Add adaptive window policy based on speech density and instability signals
3. Add semantic boundary scoring and stronger finalization gates
4. Build benchmarking harness for window/slide/overlap tuning

Technical rationale:

- Directly addresses drift and boundary quality while preserving target latency

Expected impact:

- Lower WER at boundaries, improved semantic continuity, predictable 5-8s stable latency

## Long-Term (6-12+ weeks)

1. Online learning of stability thresholds from quality telemetry
2. Topic-aware segmentation for stronger live reasoning context
3. Multi-pass optional refinement mode for high-stakes meetings
4. Unified transcript quality service shared by notes, Q&A, diarization consumers

Technical rationale:

- Converts pipeline from static heuristics to adaptive cognition infrastructure

Expected impact:

- Stronger trust, improved enterprise reliability, better downstream AI performance

## Decision Rules for Product/Engineering Alignment

- If a change lowers latency but increases semantic drift, reject or gate behind experiment.
- If a change improves stability with latency still under 8s p50 and under 10s p95, prioritize.
- Stable-context correctness for AI features is a release-blocking quality requirement.

## Success Criteria

- Stable segment latency:
  - p50: 5-8s
  - p95: <10s sustained
- Correction rate reduced release-over-release
- Drift frequency below agreed threshold
- Mid-meeting Q&A answer quality improves on internal eval sets
- User trust indicators improve (fewer "transcript is wrong" complaints)

