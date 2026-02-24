import os

import httpx
import pytest
import websockets


def _require_live_http_e2e() -> tuple[str, str]:
    if os.getenv("RUN_HTTP_E2E", "false").lower() != "true":
        pytest.skip("Live HTTP E2E tests disabled (set RUN_HTTP_E2E=true).")

    base_url = os.getenv("BACKEND_BASE_URL", "http://localhost:5167")
    auth_token = os.getenv("TEST_AUTH_BEARER")
    if not auth_token:
        pytest.skip("Missing TEST_AUTH_BEARER for live authenticated E2E tests.")

    return base_url.rstrip("/"), auth_token


@pytest.mark.anyio
async def test_websocket_http_e2e_connect_ping_stop_live():
    base_url, auth_token = _require_live_http_e2e()
    ws_url = base_url.replace("http://", "ws://").replace("https://", "wss://")
    ws_url = f"{ws_url}/ws/streaming-audio?auth_token={auth_token}"

    async with websockets.connect(ws_url) as ws:
        connected = await ws.recv()
        assert "connected" in connected

        await ws.send('{"type":"ping"}')
        pong = await ws.recv()
        assert "pong" in pong

        await ws.send('{"type":"stop"}')
        ack = await ws.recv()
        assert "stop_ack" in ack


@pytest.mark.anyio
async def test_upload_recording_http_e2e_live():
    base_url, auth_token = _require_live_http_e2e()

    files = {"file": ("e2e.wav", b"fake wav bytes", "audio/wav")}
    data = {"title": "Live E2E Upload"}
    headers = {"Authorization": f"Bearer {auth_token}"}

    async with httpx.AsyncClient(timeout=30) as client:
        response = await client.post(
            f"{base_url}/upload-meeting-recording",
            files=files,
            data=data,
            headers=headers,
        )

    assert response.status_code in (200, 202)
    payload = response.json()
    assert payload.get("status") == "processing"
    assert payload.get("meeting_id")
