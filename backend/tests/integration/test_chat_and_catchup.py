import pytest

from app.api.routers import chat as chat_router


async def _allow_all(*args, **kwargs):
    return True


@pytest.mark.anyio
async def test_chat_meeting_streams_text(async_client, monkeypatch):
    async def fake_chat_about_meeting(**kwargs):
        async def _gen():
            yield "hello"
            yield " world"

        return _gen()

    monkeypatch.setattr(chat_router.rbac, "can", _allow_all)
    monkeypatch.setattr(
        chat_router.chat_service, "chat_about_meeting", fake_chat_about_meeting
    )

    payload = {
        "meeting_id": "meeting-1",
        "question": "What happened?",
        "model": "gemini",
        "model_name": "gemini-2.5-flash",
        "context_text": "Team discussed launch scope.",
    }

    response = await async_client.post("/chat-meeting", json=payload)

    assert response.status_code == 200
    assert response.text == "hello world"


@pytest.mark.anyio
async def test_catch_up_streams_summary(async_client, monkeypatch):
    async def fake_get_api_key(provider: str, user_email: str | None = None):
        return "fake-key"

    async def fake_stream_content_text_async(**kwargs):
        yield "- Topic A\n"
        yield "- Action item B"

    monkeypatch.setattr(chat_router.db, "get_api_key", fake_get_api_key)
    monkeypatch.setattr(
        chat_router, "stream_content_text_async", fake_stream_content_text_async
    )

    payload = {
        "transcripts": [
            {"timestamp": "00:10", "text": "We decided to ship next week."},
            {"timestamp": "00:42", "text": "Owner is Alex for release notes."},
        ],
        "model": "gemini",
        "model_name": "gemini-2.5-flash",
    }

    response = await async_client.post("/catch-up", json=payload)

    assert response.status_code == 200
    assert "Topic A" in response.text
    assert "Action item B" in response.text


@pytest.mark.anyio
async def test_catch_up_rejects_empty_transcript(async_client):
    payload = {
        "transcripts": [],
        "model": "gemini",
        "model_name": "gemini-2.5-flash",
    }

    response = await async_client.post("/catch-up", json=payload)

    assert response.status_code == 400
    assert response.json()["error"] == "Not enough transcript content to summarize yet."
