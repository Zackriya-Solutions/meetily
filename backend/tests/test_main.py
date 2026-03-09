"""Tests for meeting management endpoints in main.py."""

import pytest
import time

pytestmark = pytest.mark.asyncio


# ── Helper ───────────────────────────────────────────────────────────


async def _create_meeting(client, title="Test Meeting"):
    """Create a meeting via save-transcript and return the meeting_id."""
    payload = {
        "meeting_title": title,
        "transcripts": [
            {
                "id": "t1",
                "text": "Hello world this is a test transcript",
                "timestamp": "2025-01-01T00:00:00Z",
                "audio_start_time": 0.0,
                "audio_end_time": 5.0,
                "duration": 5.0,
            }
        ],
    }
    res = await client.post("/save-transcript", json=payload)
    assert res.status_code == 200
    return res.json()["meeting_id"]


# ── GET /get-meetings ────────────────────────────────────────────────


async def test_get_meetings_empty(client):
    res = await client.get("/get-meetings")
    assert res.status_code == 200
    assert res.json() == []


async def test_get_meetings_returns_created(client):
    meeting_id = await _create_meeting(client, "My Meeting")
    res = await client.get("/get-meetings")
    assert res.status_code == 200
    meetings = res.json()
    assert len(meetings) >= 1
    titles = [m["title"] for m in meetings]
    assert "My Meeting" in titles


async def test_get_meetings_multiple(client):
    await _create_meeting(client, "Meeting A")
    await _create_meeting(client, "Meeting B")
    res = await client.get("/get-meetings")
    assert res.status_code == 200
    assert len(res.json()) >= 2


# ── GET /get-meeting/{id} ───────────────────────────────────────────


async def test_get_meeting_success(client):
    meeting_id = await _create_meeting(client)
    res = await client.get(f"/get-meeting/{meeting_id}")
    assert res.status_code == 200
    data = res.json()
    assert data["id"] == meeting_id
    assert data["title"] == "Test Meeting"
    assert len(data["transcripts"]) == 1
    assert data["transcripts"][0]["text"] == "Hello world this is a test transcript"


async def test_get_meeting_not_found(client):
    res = await client.get("/get-meeting/nonexistent-id")
    assert res.status_code == 404


async def test_get_meeting_with_audio_timestamps(client):
    meeting_id = await _create_meeting(client)
    res = await client.get(f"/get-meeting/{meeting_id}")
    data = res.json()
    t = data["transcripts"][0]
    assert t["audio_start_time"] == 0.0
    assert t["audio_end_time"] == 5.0
    assert t["duration"] == 5.0


# ── POST /save-meeting-title ────────────────────────────────────────


async def test_save_meeting_title(client):
    meeting_id = await _create_meeting(client, "Old Title")
    res = await client.post(
        "/save-meeting-title",
        json={"meeting_id": meeting_id, "title": "New Title"},
    )
    assert res.status_code == 200

    # Verify title was updated
    meeting = await client.get(f"/get-meeting/{meeting_id}")
    assert meeting.json()["title"] == "New Title"


# ── POST /delete-meeting ────────────────────────────────────────────


async def test_delete_meeting_success(client):
    meeting_id = await _create_meeting(client)
    res = await client.post("/delete-meeting", json={"meeting_id": meeting_id})
    assert res.status_code == 200

    # Verify meeting is gone
    res = await client.get(f"/get-meeting/{meeting_id}")
    assert res.status_code == 404


async def test_delete_meeting_nonexistent(client):
    res = await client.post("/delete-meeting", json={"meeting_id": "no-such-id"})
    assert res.status_code == 500


# ── POST /save-transcript ───────────────────────────────────────────


async def test_save_transcript_success(client):
    payload = {
        "meeting_title": "Transcript Test",
        "transcripts": [
            {
                "id": "t1",
                "text": "First segment",
                "timestamp": "2025-01-01T00:00:00Z",
            },
            {
                "id": "t2",
                "text": "Second segment",
                "timestamp": "2025-01-01T00:05:00Z",
            },
        ],
    }
    res = await client.post("/save-transcript", json=payload)
    assert res.status_code == 200
    data = res.json()
    assert data["status"] == "success"
    assert "meeting_id" in data

    # Verify meeting has two transcripts
    meeting = await client.get(f"/get-meeting/{data['meeting_id']}")
    assert len(meeting.json()["transcripts"]) == 2


async def test_save_transcript_with_folder_path(client):
    payload = {
        "meeting_title": "Folder Test",
        "transcripts": [
            {"id": "t1", "text": "Hello", "timestamp": "2025-01-01T00:00:00Z"}
        ],
        "folder_path": "/tmp/meetings/test",
    }
    res = await client.post("/save-transcript", json=payload)
    assert res.status_code == 200


async def test_save_transcript_empty_transcripts(client):
    payload = {
        "meeting_title": "Empty Meeting",
        "transcripts": [],
    }
    res = await client.post("/save-transcript", json=payload)
    assert res.status_code == 200


# ── GET /get-summary/{meeting_id} ───────────────────────────────────


async def test_get_summary_not_found(client):
    res = await client.get("/get-summary/nonexistent")
    assert res.status_code == 404
    data = res.json()
    assert data["status"] == "error"
    assert data["data"] is None


# ── Model config endpoints ──────────────────────────────────────────


async def test_save_and_get_model_config(client):
    save_res = await client.post(
        "/save-model-config",
        json={
            "provider": "openai",
            "model": "gpt-4o",
            "whisperModel": "large-v3",
            "apiKey": "test-key-123",
        },
    )
    assert save_res.status_code == 200

    get_res = await client.get("/get-model-config")
    assert get_res.status_code == 200
    config = get_res.json()
    assert config["provider"] == "openai"
    assert config["model"] == "gpt-4o"
    assert config["whisperModel"] == "large-v3"


async def test_get_model_config_empty(client):
    res = await client.get("/get-model-config")
    assert res.status_code == 200
    # Returns None when no config exists
    assert res.json() is None


# ── Transcript config endpoints ─────────────────────────────────────


async def test_save_and_get_transcript_config(client):
    save_res = await client.post(
        "/save-transcript-config",
        json={"provider": "deepgram", "model": "nova-2"},
    )
    assert save_res.status_code == 200

    get_res = await client.get("/get-transcript-config")
    assert get_res.status_code == 200
    config = get_res.json()
    assert config["provider"] == "deepgram"
    assert config["model"] == "nova-2"


async def test_get_transcript_config_default(client):
    res = await client.get("/get-transcript-config")
    assert res.status_code == 200
    config = res.json()
    assert config["provider"] == "localWhisper"
    assert config["model"] == "large-v3"


# ── API key endpoints ───────────────────────────────────────────────


async def test_get_api_key_no_config(client):
    res = await client.post("/get-api-key", json={"provider": "openai"})
    assert res.status_code == 200


async def test_get_transcript_api_key_no_config(client):
    res = await client.post("/get-transcript-api-key", json={"provider": "openai"})
    assert res.status_code == 200


# ── POST /save-meeting-summary ──────────────────────────────────────


async def test_save_meeting_summary_meeting_not_found(client):
    res = await client.post(
        "/save-meeting-summary",
        json={"meeting_id": "nonexistent", "summary": {"key": "val"}},
    )
    assert res.status_code == 404


async def test_save_meeting_summary_success(client):
    meeting_id = await _create_meeting(client)
    res = await client.post(
        "/save-meeting-summary",
        json={"meeting_id": meeting_id, "summary": {"MeetingName": "Test"}},
    )
    assert res.status_code == 200


# ── POST /search-transcripts ────────────────────────────────────────


async def test_search_transcripts_empty_query(client):
    res = await client.post("/search-transcripts", json={"query": ""})
    assert res.status_code == 200
    assert res.json() == []


async def test_search_transcripts_no_results(client):
    await _create_meeting(client, "Team Standup")
    res = await client.post("/search-transcripts", json={"query": "nonexistentxyz"})
    assert res.status_code == 200
    assert res.json() == []


async def test_search_transcripts_found(client):
    await _create_meeting(client, "Team Standup")
    res = await client.post("/search-transcripts", json={"query": "Hello world"})
    assert res.status_code == 200
    results = res.json()
    assert len(results) >= 1
    assert results[0]["title"] == "Team Standup"


async def test_search_transcripts_case_insensitive(client):
    await _create_meeting(client, "Meeting X")
    res = await client.post("/search-transcripts", json={"query": "hello WORLD"})
    assert res.status_code == 200
    assert len(res.json()) >= 1


# ── POST /save-transcript validation ──────────────────────────────


async def test_save_transcript_missing_title(client):
    """Missing meeting_title should return 422."""
    payload = {"transcripts": [{"id": "t1", "text": "Hello", "timestamp": "2025-01-01T00:00:00Z"}]}
    res = await client.post("/save-transcript", json=payload)
    assert res.status_code == 422


async def test_save_transcript_missing_transcripts(client):
    """Missing transcripts field should return 422."""
    payload = {"meeting_title": "Test"}
    res = await client.post("/save-transcript", json=payload)
    assert res.status_code == 422


async def test_save_transcript_returns_meeting_id(client):
    """Response should include a meeting_id starting with 'meeting-'."""
    payload = {
        "meeting_title": "ID Check",
        "transcripts": [{"id": "t1", "text": "text", "timestamp": "2025-01-01T00:00:00Z"}],
    }
    res = await client.post("/save-transcript", json=payload)
    assert res.status_code == 200
    assert res.json()["meeting_id"].startswith("meeting-")


async def test_save_transcript_multiple_segments_ordered(client):
    """Multiple transcript segments should all be saved."""
    payload = {
        "meeting_title": "Multi Segment",
        "transcripts": [
            {"id": f"t{i}", "text": f"Segment {i}", "timestamp": f"2025-01-01T00:{i:02d}:00Z"}
            for i in range(5)
        ],
    }
    res = await client.post("/save-transcript", json=payload)
    assert res.status_code == 200
    meeting_id = res.json()["meeting_id"]
    meeting = await client.get(f"/get-meeting/{meeting_id}")
    assert len(meeting.json()["transcripts"]) == 5


async def test_save_transcript_optional_audio_timestamps_null(client):
    """Transcript without audio timestamps should default to null."""
    payload = {
        "meeting_title": "No Audio TS",
        "transcripts": [{"id": "t1", "text": "text", "timestamp": "2025-01-01T00:00:00Z"}],
    }
    res = await client.post("/save-transcript", json=payload)
    meeting_id = res.json()["meeting_id"]
    meeting = await client.get(f"/get-meeting/{meeting_id}")
    t = meeting.json()["transcripts"][0]
    assert t["audio_start_time"] is None
    assert t["audio_end_time"] is None
    assert t["duration"] is None


# ── POST /save-meeting-title edge cases ───────────────────────────


async def test_save_meeting_title_empty_title(client):
    """Should still succeed with an empty string title."""
    meeting_id = await _create_meeting(client, "Original")
    res = await client.post(
        "/save-meeting-title",
        json={"meeting_id": meeting_id, "title": ""},
    )
    assert res.status_code == 200


async def test_save_meeting_title_special_characters(client):
    meeting_id = await _create_meeting(client, "Plain")
    res = await client.post(
        "/save-meeting-title",
        json={"meeting_id": meeting_id, "title": "Title with 'quotes' & <brackets>"},
    )
    assert res.status_code == 200
    meeting = await client.get(f"/get-meeting/{meeting_id}")
    assert meeting.json()["title"] == "Title with 'quotes' & <brackets>"


# ── Model config edge cases ──────────────────────────────────────


async def test_save_model_config_without_api_key(client):
    """Saving model config without apiKey should succeed."""
    res = await client.post(
        "/save-model-config",
        json={"provider": "ollama", "model": "llama3.1", "whisperModel": "base"},
    )
    assert res.status_code == 200


async def test_save_model_config_updates_existing(client):
    """Saving config twice should update, not duplicate."""
    await client.post(
        "/save-model-config",
        json={"provider": "openai", "model": "gpt-4o", "whisperModel": "large-v3"},
    )
    await client.post(
        "/save-model-config",
        json={"provider": "claude", "model": "claude-3.5", "whisperModel": "medium"},
    )
    config = await client.get("/get-model-config")
    assert config.json()["provider"] == "claude"


# ── Transcript config edge cases ─────────────────────────────────


async def test_save_transcript_config_with_api_key(client):
    res = await client.post(
        "/save-transcript-config",
        json={"provider": "openai", "model": "whisper-1", "apiKey": "sk-test-key"},
    )
    assert res.status_code == 200
    config = await client.get("/get-transcript-config")
    assert config.json()["provider"] == "openai"


# ── POST /delete-meeting edge cases ───────────────────────────────


async def test_delete_meeting_missing_body(client):
    """Missing meeting_id in body should return 422."""
    res = await client.post("/delete-meeting", json={})
    assert res.status_code == 422


async def test_delete_meeting_twice(client):
    """Deleting same meeting twice should fail on second attempt."""
    meeting_id = await _create_meeting(client)
    res1 = await client.post("/delete-meeting", json={"meeting_id": meeting_id})
    assert res1.status_code == 200
    res2 = await client.post("/delete-meeting", json={"meeting_id": meeting_id})
    assert res2.status_code == 500


# ── POST /get-api-key edge cases ─────────────────────────────────


async def test_get_api_key_after_saving(client):
    await client.post(
        "/save-model-config",
        json={"provider": "openai", "model": "gpt-4o", "whisperModel": "large-v3", "apiKey": "sk-123"},
    )
    res = await client.post("/get-api-key", json={"provider": "openai"})
    assert res.status_code == 200


async def test_get_api_key_invalid_provider(client):
    """Invalid provider should result in an error."""
    res = await client.post("/get-api-key", json={"provider": "invalid"})
    assert res.status_code == 500


# ── POST /save-meeting-summary edge cases ─────────────────────────


async def test_save_meeting_summary_with_complex_data(client):
    meeting_id = await _create_meeting(client)
    summary = {
        "MeetingName": "Sprint Planning",
        "People": {"title": "People", "blocks": [
            {"id": "1", "type": "bullet", "content": "Alice (PM)", "color": ""}
        ]},
        "SessionSummary": {"title": "Summary", "blocks": []},
    }
    res = await client.post(
        "/save-meeting-summary",
        json={"meeting_id": meeting_id, "summary": summary},
    )
    assert res.status_code == 200


# ── GET /get-summary edge cases ──────────────────────────────────


async def test_get_summary_empty_id(client):
    res = await client.get("/get-summary/")
    # FastAPI may return 404 or 405 for empty path segment
    assert res.status_code in (404, 405, 307)
