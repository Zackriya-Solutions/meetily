# AI Meeting Participant (Real-Time Meeting Intelligence)

## 1. Feature Overview

Pnyx will introduce an in-meeting AI capability that acts as a silent participant and monitors live conversation for situations that require intervention.

This feature is implemented as a lightweight, real-time guardrail system that:
- analyzes a rolling transcript window
- checks meeting context (title, description, optional agenda)
- intervenes only when specific risk conditions are met

Output is shown in a side panel as a guardrail alert, not as continuous commentary.


## 2. Product Motivation

Most meeting tools optimize for post-meeting summaries. Pnyx aims to improve meeting quality while the meeting is in progress.

Desired outcomes:
- reduce agenda drift
- prevent long discussions without closure
- surface unresolved critical questions
- prevent repeated or context-losing discussion loops

For MVP (2-3 days), implementation must be simple, reliable, and low-noise.


## 3. Guardrail Philosophy

Core principle:

`Silence by default. Intervene only when necessary.`

The AI is a silent observer, not a periodic commentator. It should not generate routine updates every few minutes. It should only alert when one or more guardrail conditions are detected.

If no guardrail condition is met, system output must be:

`NO_INTERVENTION`

This keeps meeting flow uninterrupted and preserves trust in alerts.


## 4. System Architecture

```text
Audio
-> Streaming Transcription
-> Transcript Buffer (rolling window)
-> Context Analyzer
-> LLM Reasoning
-> Guardrail Evaluator
-> Insight Publisher
-> AI Guardrail Side Panel
```

### Component roles

1. `Transcript Buffer`
- Maintains recent utterances (time-bounded and token-bounded).

2. `Context Analyzer`
- Packages meeting title, description, agenda, recent transcript window, and minimal prior state.

3. `LLM Reasoning`
- Produces structured assessment: topic status, decision signals, unresolved questions, context gaps, and intervention candidate.

4. `Guardrail Evaluator`
- Applies deterministic trigger rules, thresholds, cooldown, and dedup suppression.
- Decides publish vs `NO_INTERVENTION`.

5. `Insight Publisher`
- Pushes only accepted alerts to UI and stores short alert history.


## 5. Data Flow

1. Meeting starts:
- ingest `meeting_id`, `title`, `description`, `agenda_text` (optional)

2. Live transcript arrives continuously:
- `timestamp`, `speaker_id` (if available), `text`

3. Buffer management:
- rolling window: last 3 minutes
- token cap: 1000-1500 tokens

4. Analysis cadence:
- engine evaluates periodically (for example every 60-90 seconds)
- evaluation does not imply publication

5. LLM structured reasoning output:
- intervention required or not
- candidate reason and insight
- confidence score

6. Guardrail evaluation:
- check trigger conditions and thresholds
- enforce cooldown and duplicate suppression
- return publishable alert or `NO_INTERVENTION`

7. UI update:
- side panel updates only when a new alert is published


## 6. LLM Prompt Design

### Prompt intent

The prompt must explicitly enforce silent behavior:

`You are a silent meeting observer. You should remain silent unless one of the guardrail conditions is detected.`

### Inputs to model

- meeting title
- meeting description
- meeting agenda (optional)
- recent transcript window
- lightweight prior context (last topic/alerts summary)

### Required output schema

If no intervention is needed:

```json
{
  "intervention_required": false
}
```

If intervention is needed:

```json
{
  "intervention_required": true,
  "reason": "agenda_deviation | no_decision | unresolved_question | missing_context_or_repeat",
  "insight": "Short actionable guardrail alert.",
  "confidence": 0.84
}
```

### Prompt constraints

- do not produce commentary unless intervention is required
- ground output only in provided transcript/context
- keep insight concise (1 sentence, <= 30 words)
- return strict JSON only


## 7. Insight Generation Logic

The engine performs periodic analysis but only publishes on guardrail triggers.

### Guardrail trigger conditions

1. `agenda_deviation`
- current topic diverges from agenda for sustained period

2. `long_discussion_without_decision`
- topic duration exceeds threshold and no decision signal detected

3. `important_unresolved_question`
- meaningful question remains unanswered beyond threshold

4. `missing_context_or_repeated_topic`
- discussion revisits resolved topic or ignores previously established context

### Evaluator pseudocode

```python
if agenda_deviation is True:
    publish_alert()
elif topic_duration > TOPIC_DURATION_THRESHOLD and decision_detected is False:
    publish_alert()
elif unresolved_question is True:
    publish_alert()
elif missing_context_or_repeat is True:
    publish_alert()
else:
    return "NO_INTERVENTION"
```

### Suggested MVP thresholds

- agenda deviation sustained: >= 2 analysis cycles
- no-decision topic duration: >= 6 minutes
- unresolved question duration: >= 4 minutes
- minimum confidence to publish: >= 0.70


## 8. UI Behavior

The side panel stays quiet by default.

### Default state

- no active alert card
- optional label: `Monitoring meeting guardrails`

### Triggered state

Show a single alert card:

`AI Guardrail Alert`

Examples:
- `The discussion appears to have moved away from agenda topic "Latency Benchmark".`
- `The team has discussed transcription providers for several minutes but no decision has been recorded yet.`

### Display rules

- show reason type badge (Agenda, Decision, Question, Context)
- show timestamp (`Updated at HH:MM`)
- keep last 3 alerts in collapsible history
- do not display repeated identical alerts during cooldown


## 9. Implementation Plan

### Day 1

1. Add meeting context ingestion (title/description/agenda).
2. Build rolling transcript buffer (time + token bounded).
3. Implement LLM prompt and strict JSON parser.
4. Define guardrail output schema and evaluator interface.

### Day 2

1. Integrate reasoning call with live transcription pipeline.
2. Build Guardrail Evaluator rule checks and thresholds.
3. Add side panel alert rendering (quiet default + alert mode).
4. Add event publishing path for accepted alerts only.

### Day 3

1. Add cooldown system and duplicate suppression.
2. Tune thresholds on sample transcripts.
3. Improve prompt clarity for false positive reduction.
4. Add minimal telemetry and failure handling.


## 10. Risks and Limitations

1. False positives in decision/non-decision detection.
2. Missed alerts when transcript quality is poor.
3. Agenda deviation quality depends on agenda clarity.
4. Important questions may be hard to classify without speaker intent.
5. Alert fatigue risk if thresholds are too aggressive.

MVP mitigations:
- confidence thresholds
- sustained-condition checks (not single-window triggers)
- cooldown + dedupe
- concise alert wording


## 11. Future Improvements

1. Hybrid rule + embedding similarity for better topic continuity.
2. Speaker-aware responsibility detection (who asked / who should answer).
3. User feedback loop (`useful` / `not useful`) for online tuning.
4. Team-specific policy presets (strict vs relaxed guardrails).
5. Action-item extraction when repeated no-decision alerts occur.
6. Adaptive thresholds by meeting type (standup, planning, design review).


## Cooldown and Success Metrics (MVP Acceptance)

### Cooldown policy

- minimum 5 minutes between similar alerts (`reason` + semantic similarity)
- suppress duplicate or near-duplicate insights
- reopen alerting only when topic context materially changes

### Success metrics

Replace periodic-output metric with guardrail quality metrics:

- low noise: target `< 1 alert every 5-10 minutes` per active meeting
- high relevance: target `> 70% user-perceived usefulness`
- intervention discipline: majority of cycles return `NO_INTERVENTION`

