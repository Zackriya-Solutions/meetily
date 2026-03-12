import pytest

from app.services import ai_participant as aip
from app.schemas.ai_participant import HostEventType, HostSuggestion


class DummyDb:
    async def get_api_key(self, provider: str, user_email: str = None):
        return "dummy-gemini-key"


def _make_engine(monkeypatch):
    monkeypatch.setenv("AI_PARTICIPANT_MIN_WINDOW_CHARS", "1")
    monkeypatch.setenv("AI_PARTICIPANT_ANALYSIS_INTERVAL_SECONDS", "0")
    return aip.AIParticipantEngine(
        db=DummyDb(),
        user_email="tester@example.com",
        meeting_context=aip.MeetingContext(meeting_id="meeting-1"),
    )


def test_host_skill_override_parses_role_and_threshold(monkeypatch):
    engine = _make_engine(monkeypatch)
    engine.apply_host_skill_override(
        """
        role_mode: advisor
        min_confidence: 0.82
        threshold_conflict_risk: 0.91
        """
    )

    stats = engine.get_stats_snapshot()
    policy = stats["host_policy"]
    assert policy["role_mode"] == "advisor"
    assert abs(policy["min_confidence"] - 0.82) < 1e-6
    assert abs(policy["event_threshold_overrides"]["conflict_risk"] - 0.91) < 1e-6


def test_host_skill_override_parses_fenced_markdown(monkeypatch):
    engine = _make_engine(monkeypatch)
    engine.apply_host_skill_override(
        """
        # My host profile
        ```yaml
        role_mode: chairperson
        min_confidence: 0.67
        threshold_open_question: 0.73
        ```
        """
    )
    stats = engine.get_stats_snapshot()
    policy = stats["host_policy"]
    assert policy["role_mode"] == "chairperson"
    assert abs(policy["min_confidence"] - 0.67) < 1e-6
    assert abs(policy["event_threshold_overrides"]["open_question"] - 0.73) < 1e-6


def test_host_skill_override_infers_role_from_natural_markdown(monkeypatch):
    engine = _make_engine(monkeypatch)
    engine.apply_host_skill_override(
        """
        # SKILL: Tech Lead

        ## Role & Identity
        You are a Senior Tech Lead with 10+ years of experience.

        ## Behavior Rules
        - Always ask clarifying questions before proposing solutions
        - Default to simplicity over clever solutions
        """
    )
    policy = engine.get_stats_snapshot()["host_policy"]
    assert policy["role_mode"] == "chairperson"
    assert policy["min_confidence"] >= 0.70


def test_host_pin_and_dismiss_update_state(monkeypatch):
    engine = _make_engine(monkeypatch)
    suggestion = HostSuggestion(
        id="s1",
        event_type=HostEventType.OPEN_QUESTION,
        title="Question pending",
        content="An unresolved question needs closure.",
        confidence=0.9,
        timestamp="2026-01-01T00:00:00",
    )
    engine._host_state.suggested_items = [suggestion]

    pinned = engine.pin_suggestion("s1", actor="host@example.com")
    assert pinned is not None
    assert pinned.status == "pinned"
    assert engine._host_state.pinned_items[0].id == "s1"

    engine._host_state.suggested_items = [
        HostSuggestion(
            id="s2",
            event_type=HostEventType.AGENDA_DRIFT,
            title="Agenda drift",
            content="Discussion moved away from agenda.",
            confidence=0.88,
            timestamp="2026-01-01T00:00:01",
        )
    ]
    dismissed = engine.dismiss_suggestion("s2", actor="host@example.com")
    assert dismissed is True
    assert "s2" in engine._host_state.dismissed_item_ids
    quality = engine.get_stats_snapshot()["host_quality"]
    assert quality["suggested"] >= 0
    assert "pin_rate" in quality
    assert "dismiss_rate" in quality


@pytest.mark.asyncio
async def test_ingest_transcript_host_emits_suggestion_and_intervention(monkeypatch):
    engine = _make_engine(monkeypatch)

    async def fake_generate(*args, **kwargs):
        return '{"events":[{"event_type":"urgency_risk","title":"Decision needed","content":"Time is running out without a clear decision.","confidence":0.92,"priority":"high"}]}'

    monkeypatch.setattr(aip, "generate_content_text_async", fake_generate)

    payload = await engine.ingest_transcript_host(
        text="We are still debating and no decision is made.",
        transcript_time_seconds=3,
    )

    assert len(payload["suggestions"]) == 1
    assert payload["suggestions"][0]["event_type"] == "urgency_risk"
    assert len(payload["interventions"]) == 1
    assert payload["interventions"][0]["event_type"] == "urgency_risk"


@pytest.mark.asyncio
async def test_ingest_transcript_host_handles_invalid_json(monkeypatch):
    engine = _make_engine(monkeypatch)

    async def fake_generate(*args, **kwargs):
        return "not-json"

    monkeypatch.setattr(aip, "generate_content_text_async", fake_generate)

    payload = await engine.ingest_transcript_host(
        text="random discussion",
        transcript_time_seconds=4,
    )

    assert payload["suggestions"] == []
    assert payload["interventions"] == []
    stats = engine.get_stats_snapshot()
    assert stats["parse_failures"] >= 1
