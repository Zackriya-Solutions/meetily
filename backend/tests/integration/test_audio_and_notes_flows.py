import json
from types import SimpleNamespace

import pytest
from fastapi.websockets import WebSocketDisconnect

from app.api.routers import audio as audio_router
from app.api.routers import transcripts as transcripts_router


@pytest.mark.anyio
async def test_websocket_streaming_connects(monkeypatch):
    class FakeWebSocket:
        def __init__(self):
            self.sent = []
            self.closed = False
            self._messages = [
                {"text": json.dumps({"type": "ping"})},
                {"text": json.dumps({"type": "stop"})},
            ]

        async def accept(self):
            return None

        async def close(self, code=None, reason=None):
            self.closed = True

        async def send_json(self, payload):
            self.sent.append(payload)

        async def receive(self):
            if not self._messages:
                raise WebSocketDisconnect()
            return self._messages.pop(0)

    async def fake_auth(_token):
        from app.schemas.user import User

        return User(email="test@appointy.com", name="Test User")

    async def fake_ensure_session(**kwargs):
        return None

    async def fake_transition(*args, **kwargs):
        return None

    async def fake_touch_heartbeat(_session_id):
        return None

    async def fake_merge_metadata(*args, **kwargs):
        return None

    async def fake_get_chunk_stats(_session_id):
        return {"total": 0, "uploaded": 0}

    async def fake_update_counters(**kwargs):
        return None

    async def fake_stop_recorder(_recorder_key):
        return {}

    class _FakePostService:
        async def finalize_recording(self, *args, **kwargs):
            return {"status": "completed"}

    fake_state_service = SimpleNamespace(
        ensure_session=fake_ensure_session,
        mark_stop_requested=fake_ensure_session,
        transition=fake_transition,
        db=SimpleNamespace(
            touch_recording_session_heartbeat=fake_touch_heartbeat,
            merge_recording_session_metadata=fake_merge_metadata,
        ),
    )

    monkeypatch.setenv("ENABLE_AUDIO_RECORDING", "false")
    monkeypatch.setattr(audio_router, "_authenticate_websocket", fake_auth)
    monkeypatch.setattr(audio_router, "state_service", fake_state_service)
    monkeypatch.setattr(audio_router.db, "get_recording_chunk_stats", fake_get_chunk_stats)
    monkeypatch.setattr(
        audio_router.db, "update_recording_session_counters", fake_update_counters
    )
    monkeypatch.setattr(audio_router, "stop_recorder", fake_stop_recorder)
    monkeypatch.setattr(
        audio_router, "get_post_recording_service", lambda: _FakePostService()
    )
    monkeypatch.setattr(audio_router, "AUDIO_CELERY_ENABLED", False)

    ws = FakeWebSocket()
    await audio_router.websocket_streaming_audio(ws, auth_token="test-token")

    message_types = [msg.get("type") for msg in ws.sent]
    assert "connected" in message_types
    assert "pong" in message_types
    assert "stop_ack" in message_types


@pytest.mark.anyio
async def test_get_recording_url_prefers_wav(async_client, monkeypatch):
    async def fake_can(*args, **kwargs):
        return True

    async def fake_exists(path):
        return path.endswith("/recording.wav")

    async def fake_signed_url(path, expiration_seconds=3600, download_filename=None):
        return f"https://example.test/{path}"

    monkeypatch.setattr(audio_router.rbac, "can", fake_can)
    monkeypatch.setattr(audio_router.StorageService, "check_file_exists", fake_exists)
    monkeypatch.setattr(audio_router.StorageService, "generate_signed_url", fake_signed_url)

    meeting_id = "00000000-0000-0000-0000-000000000999"
    response = await async_client.get(f"/meetings/{meeting_id}/recording-url")

    assert response.status_code == 200
    payload = response.json()
    assert payload["url"].endswith(f"{meeting_id}/recording.wav")


@pytest.mark.anyio
async def test_generate_notes_kicks_off_background_task(async_client, monkeypatch):
    async def fake_can(*args, **kwargs):
        return True

    async def fake_get_meeting(_meeting_id):
        return {"title": "Weekly Sync", "transcripts": [{"text": "hello"}]}

    async def fake_generate_notes(*args, **kwargs):
        return None

    monkeypatch.setattr(transcripts_router.rbac, "can", fake_can)
    monkeypatch.setattr(transcripts_router.db, "get_meeting", fake_get_meeting)
    monkeypatch.setattr(
        transcripts_router,
        "generate_notes_with_gemini_background",
        fake_generate_notes,
    )

    meeting_id = "00000000-0000-0000-0000-000000000123"
    payload = {
        "meeting_id": meeting_id,
        "template_id": "standard_meeting",
        "transcript": "Decision made to ship by Friday.",
        "use_audio_context": False,
    }
    response = await async_client.post(
        f"/meetings/{meeting_id}/generate-notes", json=payload
    )

    assert response.status_code == 200
    result = response.json()
    assert result["status"] == "processing"
    assert result["meeting_id"] == meeting_id
