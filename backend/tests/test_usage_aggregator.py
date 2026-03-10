"""Tests for usage aggregation functions."""

import pytest
from datetime import datetime, timezone
from uuid import uuid4

pytestmark = pytest.mark.usefixtures("cleanup_db")


async def _insert_event(events_col, user_id, event_type, value, timestamp_str,
                        device_id="device-001", metadata=None):
    """Insert a raw usage event."""
    await events_col.insert_one({
        "user_id": user_id,
        "device_id": device_id,
        "event_type": event_type,
        "value": value,
        "metadata": metadata or {},
        "timestamp": timestamp_str,
        "received_at": datetime.now(timezone.utc),
        "aggregated": False,
        "client_event_id": str(uuid4()),
    })


# ── Basic aggregation ────────────────────────────────────────────────


async def test_aggregate_recording_minutes():
    from mongodb import get_usage_events_collection, get_usage_summaries_collection
    from usage_aggregator import aggregate_user_usage

    events_col = get_usage_events_collection()
    summaries_col = get_usage_summaries_collection()

    user_id = "agg-user-1"
    await _insert_event(events_col, user_id, "recording_minutes", 10.0, "2025-01-15T10:00:00Z")
    await _insert_event(events_col, user_id, "recording_minutes", 5.0, "2025-01-15T11:00:00Z")

    await aggregate_user_usage(user_id)

    summary = await summaries_col.find_one({
        "user_id": user_id,
        "period_type": "daily",
        "period_start": "2025-01-15",
    })
    assert summary is not None
    assert summary["recording_minutes"] == 15.0


async def test_aggregate_meeting_created():
    from mongodb import get_usage_events_collection, get_usage_summaries_collection
    from usage_aggregator import aggregate_user_usage

    events_col = get_usage_events_collection()
    summaries_col = get_usage_summaries_collection()

    user_id = "agg-user-2"
    await _insert_event(events_col, user_id, "meeting_created", 1, "2025-02-10T09:00:00Z")
    await _insert_event(events_col, user_id, "meeting_created", 1, "2025-02-10T10:00:00Z")

    await aggregate_user_usage(user_id)

    summary = await summaries_col.find_one({
        "user_id": user_id,
        "period_start": "2025-02-10",
    })
    assert summary is not None
    assert summary["meetings_count"] == 2


async def test_aggregate_multiple_event_types():
    from mongodb import get_usage_events_collection, get_usage_summaries_collection
    from usage_aggregator import aggregate_user_usage

    events_col = get_usage_events_collection()
    summaries_col = get_usage_summaries_collection()

    user_id = "agg-user-3"
    await _insert_event(events_col, user_id, "recording_minutes", 5.0, "2025-03-01T10:00:00Z")
    await _insert_event(events_col, user_id, "transcription_minutes", 3.0, "2025-03-01T10:05:00Z")
    await _insert_event(events_col, user_id, "summary_generated", 1, "2025-03-01T10:10:00Z")

    await aggregate_user_usage(user_id)

    summary = await summaries_col.find_one({
        "user_id": user_id,
        "period_start": "2025-03-01",
    })
    assert summary is not None
    assert summary["recording_minutes"] == 5.0
    assert summary["transcription_minutes"] == 3.0
    assert summary["summaries_count"] == 1


# ── Idempotency ──────────────────────────────────────────────────────


async def test_aggregate_is_idempotent():
    from mongodb import get_usage_events_collection, get_usage_summaries_collection
    from usage_aggregator import aggregate_user_usage

    events_col = get_usage_events_collection()
    summaries_col = get_usage_summaries_collection()

    user_id = "agg-user-idem"
    await _insert_event(events_col, user_id, "recording_minutes", 10.0, "2025-01-20T10:00:00Z")

    await aggregate_user_usage(user_id)
    await aggregate_user_usage(user_id)  # Should not double-count

    summary = await summaries_col.find_one({
        "user_id": user_id,
        "period_start": "2025-01-20",
    })
    assert summary["recording_minutes"] == 10.0


async def test_events_marked_as_aggregated():
    from mongodb import get_usage_events_collection
    from usage_aggregator import aggregate_user_usage

    events_col = get_usage_events_collection()

    user_id = "agg-user-mark"
    await _insert_event(events_col, user_id, "session_started", 1, "2025-04-01T10:00:00Z")

    await aggregate_user_usage(user_id)

    event = await events_col.find_one({"user_id": user_id})
    assert event["aggregated"] is True


# ── Empty data ───────────────────────────────────────────────────────


async def test_aggregate_no_events():
    from usage_aggregator import aggregate_user_usage
    # Should not raise on user with no events
    await aggregate_user_usage("nonexistent-user")


# ── Different days ───────────────────────────────────────────────────


async def test_aggregate_across_days():
    from mongodb import get_usage_events_collection, get_usage_summaries_collection
    from usage_aggregator import aggregate_user_usage

    events_col = get_usage_events_collection()
    summaries_col = get_usage_summaries_collection()

    user_id = "agg-user-days"
    await _insert_event(events_col, user_id, "recording_minutes", 5.0, "2025-05-01T10:00:00Z")
    await _insert_event(events_col, user_id, "recording_minutes", 8.0, "2025-05-02T10:00:00Z")

    await aggregate_user_usage(user_id)

    day1 = await summaries_col.find_one({"user_id": user_id, "period_start": "2025-05-01"})
    day2 = await summaries_col.find_one({"user_id": user_id, "period_start": "2025-05-02"})
    assert day1["recording_minutes"] == 5.0
    assert day2["recording_minutes"] == 8.0


# ── Different devices ────────────────────────────────────────────────


async def test_aggregate_per_device():
    from mongodb import get_usage_events_collection, get_usage_summaries_collection
    from usage_aggregator import aggregate_user_usage

    events_col = get_usage_events_collection()
    summaries_col = get_usage_summaries_collection()

    user_id = "agg-user-dev"
    await _insert_event(events_col, user_id, "recording_minutes", 5.0,
                        "2025-06-01T10:00:00Z", device_id="dev-A")
    await _insert_event(events_col, user_id, "recording_minutes", 3.0,
                        "2025-06-01T10:00:00Z", device_id="dev-B")

    await aggregate_user_usage(user_id)

    dev_a = await summaries_col.find_one({"user_id": user_id, "device_id": "dev-A"})
    dev_b = await summaries_col.find_one({"user_id": user_id, "device_id": "dev-B"})
    assert dev_a["recording_minutes"] == 5.0
    assert dev_b["recording_minutes"] == 3.0


# ── Metadata tracking ───────────────────────────────────────────────


async def test_aggregate_whisper_model_tracking():
    from mongodb import get_usage_events_collection, get_usage_summaries_collection
    from usage_aggregator import aggregate_user_usage

    events_col = get_usage_events_collection()
    summaries_col = get_usage_summaries_collection()

    user_id = "agg-user-whisper"
    await _insert_event(
        events_col, user_id, "transcription_minutes", 5.0,
        "2025-07-01T10:00:00Z",
        metadata={"whisper_model": "large-v3"},
    )

    await aggregate_user_usage(user_id)

    summary = await summaries_col.find_one({"user_id": user_id})
    assert "large-v3" in summary.get("whisper_models_used", [])


async def test_aggregate_llm_model_tracking():
    from mongodb import get_usage_events_collection, get_usage_summaries_collection
    from usage_aggregator import aggregate_user_usage

    events_col = get_usage_events_collection()
    summaries_col = get_usage_summaries_collection()

    user_id = "agg-user-llm"
    await _insert_event(
        events_col, user_id, "summary_generated", 1,
        "2025-08-01T10:00:00Z",
        metadata={"llm_provider": "openai", "llm_model": "gpt-4o"},
    )

    await aggregate_user_usage(user_id)

    summary = await summaries_col.find_one({"user_id": user_id})
    assert any(
        m.get("provider") == "openai" and m.get("model") == "gpt-4o"
        for m in summary.get("llm_models_used", [])
    )


# ── Unknown event types ─────────────────────────────────────────────


async def test_aggregate_unknown_event_type():
    from mongodb import get_usage_events_collection
    from usage_aggregator import aggregate_user_usage

    events_col = get_usage_events_collection()

    user_id = "agg-user-unknown"
    await _insert_event(events_col, user_id, "unknown_type", 1, "2025-09-01T10:00:00Z")

    # Should process without error; event gets marked as aggregated
    await aggregate_user_usage(user_id)

    event = await events_col.find_one({"user_id": user_id})
    assert event["aggregated"] is True
