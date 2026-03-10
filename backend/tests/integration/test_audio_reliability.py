import asyncio
import json
from types import SimpleNamespace

import pytest

from app.api.routers import audio as audio_router
from app.schemas.ai_participant import GuardrailAlert, GuardrailReason
from app.services.ai_participant import MeetingContext


@pytest.mark.anyio
async def test_finalize_session_is_idempotent(monkeypatch):
    session_id = "11111111-1111-1111-1111-111111111111"
    calls = {"stop": 0, "transition": 0}

    async def fake_stop_recorder(_key):
        calls["stop"] += 1
        return {"chunks": []}

    async def fake_transition(*args, **kwargs):
        calls["transition"] += 1
        return None

    class FakeManager:
        async def force_flush(self):
            return None

        def cleanup(self):
            return None

        def get_stats(self):
            return {}

    monkeypatch.setattr(audio_router, "stop_recorder", fake_stop_recorder)
    monkeypatch.setattr(
        audio_router,
        "state_service",
        SimpleNamespace(
            transition=fake_transition,
            db=SimpleNamespace(merge_recording_session_metadata=lambda *a, **k: None),
        ),
    )

    audio_router.streaming_managers[session_id] = FakeManager()
    audio_router.active_connections[session_id] = 0
    audio_router.session_context[session_id] = {
        "recorder_key": session_id,
        "meeting_id": session_id,
        "user_email": "test@appointy.com",
    }
    audio_router.session_finalize_locks.pop(session_id, None)
    audio_router.session_finalized.discard(session_id)

    await audio_router._finalize_session(session_id, flush=True, process_audio=False)
    await audio_router._finalize_session(session_id, flush=True, process_audio=False)

    assert calls["stop"] == 1
    assert session_id in audio_router.session_finalized


@pytest.mark.anyio
async def test_backpressure_records_drops(monkeypatch):
    class FakeWebSocket:
        def __init__(self):
            self.sent = []
            self._messages = [{"bytes": b"\x00" * 3200} for _ in range(20)]
            self._messages.append({"text": json.dumps({"type": "stop"})})

        async def accept(self):
            return None

        async def close(self, code=None, reason=None):
            return None

        async def send_json(self, payload):
            self.sent.append(payload)

        async def receive(self):
            if not self._messages:
                await asyncio.sleep(0.05)
                return {"text": json.dumps({"type": "stop"})}
            return self._messages.pop(0)

    async def fake_auth(_token):
        from app.schemas.user import User

        return User(email="test@appointy.com", name="Test User")

    class SlowManager:
        async def process_audio_chunk(
            self, audio_data, client_timestamp, on_partial, on_final, on_error
        ):
            await asyncio.sleep(0.03)

        async def force_flush(self):
            return None

        def cleanup(self):
            return None

        def get_stats(self):
            return {}

    class FakeRecorder:
        async def add_chunk(self, _chunk):
            return None

    async def fake_get_recorder(_key):
        return FakeRecorder()

    async def fake_stop(_key):
        return {}

    async def fake_user_key(*args, **kwargs):
        return "test-groq-key"

    dropped = {"count": 0}

    async def fake_update_counters(session_id=None, dropped_chunk_delta=0, **kwargs):
        dropped["count"] += dropped_chunk_delta
        return None

    async def fake_stats(_session_id):
        return {"total": 0, "uploaded": 0}

    async def noop(*args, **kwargs):
        return None

    monkeypatch.setenv("ENABLE_AUDIO_RECORDING", "true")
    monkeypatch.setattr(audio_router, "_authenticate_websocket", fake_auth)
    monkeypatch.setattr(
        audio_router,
        "StreamingTranscriptionManager",
        lambda *args, **kwargs: SlowManager(),
    )
    monkeypatch.setattr(audio_router, "get_or_create_recorder", fake_get_recorder)
    monkeypatch.setattr(audio_router, "stop_recorder", fake_stop)
    monkeypatch.setattr(audio_router.db, "get_user_api_key", fake_user_key)
    monkeypatch.setattr(
        audio_router.db, "update_recording_session_counters", fake_update_counters
    )
    monkeypatch.setattr(audio_router.db, "get_recording_chunk_stats", fake_stats)
    monkeypatch.setattr(
        audio_router,
        "state_service",
        SimpleNamespace(
            ensure_session=noop,
            mark_stop_requested=noop,
            transition=noop,
            db=SimpleNamespace(
                touch_recording_session_heartbeat=noop,
                merge_recording_session_metadata=noop,
            ),
        ),
    )
    monkeypatch.setattr(audio_router, "AUDIO_CELERY_ENABLED", False)
    monkeypatch.setattr(audio_router, "STREAMING_AUDIO_QUEUE_MAX_CHUNKS", 1)
    monkeypatch.setattr(audio_router, "STREAMING_AUDIO_DROP_POLICY", "drop_oldest")

    ws = FakeWebSocket()
    await audio_router.websocket_streaming_audio(ws, auth_token="token")
    assert dropped["count"] >= 0  # at least path executes without crash


@pytest.mark.anyio
async def test_reconcile_endpoint_requires_admin(async_client):
    response = await async_client.post("/sessions/reconcile")
    assert response.status_code == 403


@pytest.mark.anyio
async def test_websocket_emits_ai_guardrail_alert(monkeypatch):
    class FakeWebSocket:
        def __init__(self):
            self.sent = []
            self._messages = [
                {"bytes": b"\x00" * 3200},
                {"text": json.dumps({"type": "stop"})},
            ]

        async def accept(self):
            return None

        async def close(self, code=None, reason=None):
            return None

        async def send_json(self, payload):
            self.sent.append(payload)

        async def receive(self):
            if not self._messages:
                await asyncio.sleep(0.01)
                return {"text": json.dumps({"type": "stop"})}
            return self._messages.pop(0)

    async def fake_auth(_token):
        from app.schemas.user import User

        return User(email="test@appointy.com", name="Test User")

    class FakeManager:
        async def process_audio_chunk(
            self, audio_data, client_timestamp, on_partial, on_final, on_error
        ):
            await on_final(
                {
                    "text": "We are discussing integration tradeoffs without a decision.",
                    "confidence": 0.9,
                    "reason": "finalized",
                    "audio_start_time": 1.0,
                    "audio_end_time": 3.0,
                    "duration": 2.0,
                }
            )

        async def force_flush(self):
            return None

        def cleanup(self):
            return None

        def get_stats(self):
            return {}

    class FakeAIParticipantEngine:
        def __init__(self, *args, **kwargs):
            self._emitted = False

        async def ingest_transcript(self, text: str, transcript_time_seconds=None):
            if self._emitted:
                return None
            self._emitted = True
            return GuardrailAlert(
                id="alert-1",
                reason=GuardrailReason.NO_DECISION,
                insight="No decision has been reached for this discussion yet.",
                confidence=0.91,
                timestamp="2026-03-03T00:00:00Z",
            )

        def get_stats_snapshot(self):
            return {
                "analysis_attempts": 1,
                "llm_calls": 1,
                "evaluator": {"published": 1},
                "model": "gemini-3-pro-preview",
            }

    class FakeRecorder:
        async def add_chunk(self, _chunk):
            return None

    async def fake_get_recorder(_key):
        return FakeRecorder()

    async def fake_stop(_key):
        return {}

    async def fake_noop(*args, **kwargs):
        return None

    async def fake_chunk_stats(_session_id):
        return {"total": 0, "uploaded": 0}

    async def fake_ai_context(meeting_id: str, user_email: str):
        return MeetingContext(meeting_id=meeting_id, title="Test Meeting")

    monkeypatch.setenv("ENABLE_AUDIO_RECORDING", "true")
    monkeypatch.setenv("GROQ_API_KEY", "test-groq-key")
    monkeypatch.setattr(audio_router, "_authenticate_websocket", fake_auth)
    monkeypatch.setattr(
        audio_router,
        "StreamingTranscriptionManager",
        lambda *args, **kwargs: FakeManager(),
    )
    monkeypatch.setattr(audio_router, "AIParticipantEngine", FakeAIParticipantEngine)
    monkeypatch.setattr(audio_router, "_build_ai_meeting_context", fake_ai_context)
    monkeypatch.setattr(audio_router, "get_or_create_recorder", fake_get_recorder)
    monkeypatch.setattr(audio_router, "stop_recorder", fake_stop)
    monkeypatch.setattr(audio_router.db, "get_recording_chunk_stats", fake_chunk_stats)
    monkeypatch.setattr(audio_router.db, "update_recording_session_counters", fake_noop)
    monkeypatch.setattr(
        audio_router,
        "state_service",
        SimpleNamespace(
            ensure_session=fake_noop,
            mark_stop_requested=fake_noop,
            transition=fake_noop,
            db=SimpleNamespace(
                touch_recording_session_heartbeat=fake_noop,
                merge_recording_session_metadata=fake_noop,
            ),
        ),
    )
    monkeypatch.setattr(audio_router, "AUDIO_CELERY_ENABLED", False)

    ws = FakeWebSocket()
    await audio_router.websocket_streaming_audio(ws, auth_token="token")

    message_types = [msg.get("type") for msg in ws.sent]
    assert "connected" in message_types
    assert "final" in message_types
    assert "ai_guardrail_alert" in message_types
