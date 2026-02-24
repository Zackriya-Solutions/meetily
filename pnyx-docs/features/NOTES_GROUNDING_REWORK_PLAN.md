# Notes Grounding Rework Plan

**Date:** February 24, 2026  
**Status:** Proposed (Review First, Then Implement)  
**Owner:** Backend + Notes UX

## 1) Why we are changing this

Current notes generation is powerful, but prompt/input complexity is high:

- We pass a large amount of instructions and strict structure requirements.
- Model can spend effort on format compliance instead of understanding meeting meaning.
- Notes quality can degrade when transcript is non-diarized and speaker ownership is unclear.

Goal: improve grounding quality by simplifying inputs and source priority.

## 2) Proposed grounding strategy

Use only the highest-value context for first-pass notes:

1. **Audio** (primary for meaning, intent, decisions, emphasis)
2. **Transcript** (secondary for wording and factual anchors)
3. **Calendar agenda/description** (context prior for purpose and expected outcomes)

Transcript source selection:

- If diarized transcript exists at generation time: use diarized transcript.
- Else use live transcript for initial generation.

## 3) Diarization-aware regeneration flow

### Problem
Notes may be generated before diarization completes.

### Proposed behavior

If notes were generated from non-diarized transcript and diarization later completes:

- Mark meeting with `notes_regen_recommended=true`.
- Show UX hint in notes panel:
  - “Speaker-aware transcript is now available. Regenerate notes for better quality.”
- Provide one-click CTA:
  - `Regenerate with diarized transcript`

### Scope

- No forced auto-regeneration (avoid surprising users and extra cost).
- User-controlled regeneration for quality/cost balance.

## 4) Prompt simplification proposal

### Current issue
Template prompts are strict and verbose JSON-shape instructions.

### Proposed change
Move to a simpler instruction style:

- “Generate meeting notes using selected template style.”
- Keep only essential constraints:
  - Do not hallucinate.
  - Prefer audio evidence for decisions/actions.
  - Use agenda to frame objectives.
  - Keep owners and due dates explicit when present.

Avoid large repeated “full structure” examples in prompt body.

## 5) Output format approach (compatibility-safe)

Frontend and storage currently depend on structured output.  
To improve grounding without breaking UI:

### Recommended two-step strategy

1. Model generates **clean template-guided markdown** from grounded sources.
2. Backend converts markdown into existing structured schema (or keeps markdown as canonical and derives JSON view).

Benefits:

- Model focuses on meaning and content quality first.
- We keep backward compatibility with existing UI/DB expectations.
- Reduced JSON parse fragility from multimodal prompts.

## 6) Backend design changes (target)

Primary file: `backend/app/api/routers/transcripts.py`

1. Add transcript source resolver:
- Prefer transcript version `source='diarized'`.
- Fallback to `source='live'`.

2. Persist notes-generation metadata:
- `notes_transcript_source: diarized|live`
- `notes_audio_used: true|false`
- `notes_agenda_used: true|false`
- `notes_prompt_version: v2_simplified`

3. After diarization completion:
- If latest notes used `live`, set recommendation flag for that meeting.

4. Add regenerate endpoint mode:
- Explicit flag `prefer_diarized=true`.

## 7) Frontend UX changes (target)

Likely files:
- `frontend/src/components/MeetingDetails/SummaryPanel.tsx`
- `frontend/src/hooks/meeting-details/useSummaryGeneration.ts`

Behavior:

1. Read notes metadata + meeting diarization status.
2. If diarization completed and notes source was live:
- Show non-blocking recommendation banner.
3. CTA triggers generate-notes call with diarized preference.
4. Show “generated from diarized transcript” badge after success.

## 8) Template strategy

Keep template selection (`standard_meeting`, `daily_standup`, etc.), but reduce template prompt complexity:

- Templates define **style and sections**, not huge rigid JSON examples.
- Move deep formatting rules from model prompt into deterministic backend post-processing where possible.

## 9) Rollout plan

### Phase A (safe metadata + recommendation)
1. Track transcript source used for notes.
2. Add diarization-complete recommendation banner and regenerate CTA.
3. No prompt change yet.

### Phase B (simplified prompt)
1. Introduce new prompt version `v2_simplified` behind feature flag.
2. A/B compare quality against current prompts.

### Phase C (output simplification)
1. Add markdown-first generation path.
2. Keep compatibility transformer for existing UI schema.

## 10) Quality evaluation

Evaluate on mixed meeting set (at least 30 recordings):

- Action-item precision (owner + due date)
- Decision extraction correctness
- Speaker attribution quality (especially for multi-speaker meetings)
- Human rating: clarity, usefulness, faithfulness

Compare:

1. Current pipeline
2. Simplified grounding + live transcript
3. Simplified grounding + diarized transcript

## 11) Risks and controls

- **Risk:** Simpler prompts reduce structural consistency.  
  **Control:** deterministic backend post-processing + template validator.

- **Risk:** Extra regeneration increases API cost.  
  **Control:** user-triggered CTA only, no automatic regeneration.

- **Risk:** Diarization delay causes stale recommendations.  
  **Control:** recompute recommendation state from latest notes metadata at render time.

## 12) Definition of done

1. Notes generation explicitly prioritizes audio + transcript + agenda only.
2. System stores which transcript source was used (live/diarized).
3. UI shows regenerate recommendation when diarized transcript becomes available later.
4. Simplified prompt version is available behind feature flag and benchmarked.
5. No regression in existing notes display/edit/save flows.
