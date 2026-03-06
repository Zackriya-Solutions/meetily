import asyncio
import time
import pytest

from app.services import ai_participant as aip
from app.schemas.ai_participant import GuardrailLLMOutput, GuardrailReason
from app.services.ai_participant import GuardrailEvaluator, RollingTranscriptBuffer, AIParticipantEngine


class DummyDb:
    def __init__(self, key: str = "dummy-gemini-key"):
        self._key = key

    async def get_api_key(self, provider: str, user_email: str = None):
        return self._key


def test_rolling_buffer_prunes_by_time_and_size():
    buf = RollingTranscriptBuffer(window_seconds=10, max_chars=20)
    buf.add(1, "hello")
    buf.add(5, "world")
    buf.add(20, "trim")
    assert "hello" not in buf.get_text()
    assert "world" not in buf.get_text()
    assert "trim" in buf.get_text()


def test_agenda_deviation_requires_sustained_cycles(monkeypatch):
    monkeypatch.setenv("AI_PARTICIPANT_AGENDA_SUSTAINED_CYCLES", "2")
    monkeypatch.setenv("AI_PARTICIPANT_COOLDOWN_SECONDS", "0")
    evaluator = GuardrailEvaluator()

    assessment = GuardrailLLMOutput(
        intervention_required=True,
        reason=GuardrailReason.AGENDA_DEVIATION,
        insight="Discussion has moved off the agenda.",
        confidence=0.9,
    )

    assert evaluator.evaluate(assessment, window_duration_seconds=300, now_ts=time.time()) is None
    alert = evaluator.evaluate(assessment, window_duration_seconds=300, now_ts=time.time() + 1)
    assert alert is not None
    assert alert.reason == GuardrailReason.AGENDA_DEVIATION


def test_no_decision_respects_duration_threshold(monkeypatch):
    monkeypatch.setenv("AI_PARTICIPANT_NO_DECISION_SECONDS", "360")
    monkeypatch.setenv("AI_PARTICIPANT_COOLDOWN_SECONDS", "0")
    evaluator = GuardrailEvaluator()

    assessment = GuardrailLLMOutput(
        intervention_required=True,
        reason=GuardrailReason.NO_DECISION,
        insight="The team has not converged on a decision yet.",
        confidence=0.88,
    )

    assert evaluator.evaluate(assessment, window_duration_seconds=200, now_ts=time.time()) is None
    alert = evaluator.evaluate(assessment, window_duration_seconds=400, now_ts=time.time() + 1)
    assert alert is not None
    assert alert.reason == GuardrailReason.NO_DECISION


def test_evaluator_telemetry_cooldown_suppression(monkeypatch):
    monkeypatch.setenv("AI_PARTICIPANT_COOLDOWN_SECONDS", "180")
    evaluator = GuardrailEvaluator()

    assessment = GuardrailLLMOutput(
        intervention_required=True,
        reason=GuardrailReason.NO_DECISION,
        insight="Decision is pending.",
        confidence=0.9,
    )

    first = evaluator.evaluate(assessment, window_duration_seconds=600, now_ts=time.time())
    second = evaluator.evaluate(assessment, window_duration_seconds=600, now_ts=time.time() + 1)
    assert first is not None
    assert second is None

    metrics = evaluator.get_metrics_snapshot()
    assert metrics["evaluations"] == 2
    assert metrics["published"] == 1
    assert metrics["suppressed_cooldown"] == 1


def test_extract_json_handles_fenced_payload():
    payload = """Some preamble\n```json\n{\"intervention_required\": false}\n```"""
    parsed = AIParticipantEngine._extract_json(payload)
    assert parsed == {"intervention_required": False}


def test_normalize_reason_aliases():
    assert (
        AIParticipantEngine._normalize_reason("long_discussion_without_decision")
        == "no_decision"
    )
    assert (
        AIParticipantEngine._normalize_reason("important_unresolved_question")
        == "unresolved_question"
    )
    assert AIParticipantEngine._normalize_reason("missing_context") == "missing_context_or_repeat"


def test_normalize_model_payload_falls_back_to_silent_on_invalid_reason():
    payload = {
        "intervention_required": True,
        "reason": "unknown_reason",
        "insight": "Some text",
        "confidence": 0.99,
    }
    normalized = AIParticipantEngine._normalize_model_payload(payload)
    assert normalized == {"intervention_required": False}


def test_normalize_model_payload_clamps_confidence():
    payload = {
        "intervention_required": True,
        "reason": "no_decision",
        "insight": "No decision has been reached.",
        "confidence": 7,
    }
    normalized = AIParticipantEngine._normalize_model_payload(payload)
    assert normalized is not None
    assert normalized["confidence"] == 1.0


@pytest.mark.asyncio
async def test_reasoning_timeout_updates_stats(monkeypatch):
    monkeypatch.setenv("AI_PARTICIPANT_MIN_WINDOW_CHARS", "1")
    monkeypatch.setenv("AI_PARTICIPANT_ANALYSIS_INTERVAL_SECONDS", "0")
    monkeypatch.setenv("AI_PARTICIPANT_LLM_TIMEOUT_SECONDS", "0")

    async def slow_generate(*args, **kwargs):
        await asyncio.sleep(0.01)
        return '{"intervention_required": false}'

    monkeypatch.setattr(aip, "generate_content_text_async", slow_generate)

    engine = AIParticipantEngine(
        db=DummyDb(),
        user_email="tester@example.com",
        meeting_context=aip.MeetingContext(meeting_id="m1"),
    )

    alert = await engine.ingest_transcript("hello world", transcript_time_seconds=1)
    assert alert is None
    stats = engine.get_stats_snapshot()
    assert stats["llm_calls"] == 1
    assert stats["llm_failures"] >= 1
    assert stats["llm_timeouts"] >= 1


@pytest.mark.asyncio
async def test_parse_failure_updates_stats(monkeypatch):
    monkeypatch.setenv("AI_PARTICIPANT_MIN_WINDOW_CHARS", "1")
    monkeypatch.setenv("AI_PARTICIPANT_ANALYSIS_INTERVAL_SECONDS", "0")

    async def bad_json_generate(*args, **kwargs):
        return "definitely-not-json"

    monkeypatch.setattr(aip, "generate_content_text_async", bad_json_generate)

    engine = AIParticipantEngine(
        db=DummyDb(),
        user_email="tester@example.com",
        meeting_context=aip.MeetingContext(meeting_id="m1"),
    )

    alert = await engine.ingest_transcript("hello world", transcript_time_seconds=1)
    assert alert is None
    stats = engine.get_stats_snapshot()
    assert stats["llm_calls"] == 1
    assert stats["parse_failures"] == 1
