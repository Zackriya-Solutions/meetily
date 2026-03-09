"""Tests for usage event ingestion and deduplication."""

import pytest
from helpers import _register_payload

pytestmark = pytest.mark.asyncio


async def _get_auth_header(client) -> dict:
    """Register a user and return auth header."""
    res = await client.post("/api/auth/register", json=_register_payload())
    return {"Authorization": f"Bearer {res.json()['access_token']}"}


async def test_ingest_events(client):
    headers = await _get_auth_header(client)
    payload = {
        "device_id": "device-001",
        "events": [
            {
                "event_type": "recording_minutes",
                "value": 5.5,
                "timestamp": "2025-01-01T00:00:00Z",
            },
            {
                "event_type": "meeting_created",
                "value": 1,
                "timestamp": "2025-01-01T00:01:00Z",
            },
        ],
    }
    res = await client.post("/api/usage/events", json=payload, headers=headers)
    assert res.status_code == 200
    assert res.json()["ingested"] == 2


async def test_deduplication(client):
    """Sending the same client_event_id twice should not double-count."""
    headers = await _get_auth_header(client)
    payload = {
        "device_id": "device-001",
        "events": [
            {
                "event_type": "session_started",
                "value": 1,
                "client_event_id": "unique-123",
                "timestamp": "2025-01-01T00:00:00Z",
            },
        ],
    }

    res1 = await client.post("/api/usage/events", json=payload, headers=headers)
    assert res1.json()["ingested"] == 1

    # Send again — should be skipped
    res2 = await client.post("/api/usage/events", json=payload, headers=headers)
    assert res2.json()["ingested"] == 0


async def test_summary_query(client):
    headers = await _get_auth_header(client)
    res = await client.get("/api/usage/summary", headers=headers)
    assert res.status_code == 200
    data = res.json()
    assert data["period"] == "all_time"
    assert data["recording_minutes"] == 0.0
