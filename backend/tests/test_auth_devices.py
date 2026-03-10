"""Tests for auth device management: linking, unlinking, and limits."""

from helpers import _register_payload


async def _register_and_get_token(client, email="test@example.com", device_id="device-001"):
    """Register a user and return (access_token, user_id)."""
    res = await client.post("/api/auth/register", json=_register_payload(
        email=email, device_id=device_id,
    ))
    data = res.json()
    return data["access_token"], data["user"]["user_id"]


def _auth(token):
    return {"Authorization": f"Bearer {token}"}


# ── Link device ──────────────────────────────────────────────────────


async def test_link_device_success(client):
    token, _ = await _register_and_get_token(client)
    res = await client.post(
        "/api/auth/link-device",
        json={"device_id": "device-002", "platform": "windows"},
        headers=_auth(token),
    )
    assert res.status_code == 200
    assert res.json()["device_id"] == "device-002"


async def test_link_device_already_linked(client):
    token, _ = await _register_and_get_token(client)
    res = await client.post(
        "/api/auth/link-device",
        json={"device_id": "device-001"},
        headers=_auth(token),
    )
    assert res.status_code == 200
    assert "already linked" in res.json()["message"].lower()


async def test_link_device_requires_auth(client):
    res = await client.post(
        "/api/auth/link-device",
        json={"device_id": "device-002"},
    )
    assert res.status_code == 401


# ── List devices ─────────────────────────────────────────────────────


async def test_list_auth_devices(client):
    token, _ = await _register_and_get_token(client)
    # Should have initial device from registration
    res = await client.get("/api/auth/devices", headers=_auth(token))
    assert res.status_code == 200
    devices = res.json()
    assert len(devices) == 1
    assert devices[0]["device_id"] == "device-001"


async def test_list_auth_devices_after_linking(client):
    token, _ = await _register_and_get_token(client)
    await client.post(
        "/api/auth/link-device",
        json={"device_id": "device-002"},
        headers=_auth(token),
    )
    res = await client.get("/api/auth/devices", headers=_auth(token))
    assert len(res.json()) == 2


# ── Unlink device ────────────────────────────────────────────────────


async def test_unlink_device_success(client):
    token, _ = await _register_and_get_token(client)
    # Link a second device first
    await client.post(
        "/api/auth/link-device",
        json={"device_id": "device-002"},
        headers=_auth(token),
    )
    # Now unlink it
    res = await client.delete("/api/auth/devices/device-002", headers=_auth(token))
    assert res.status_code == 200
    assert res.json()["device_id"] == "device-002"

    # Verify it's gone
    devices_res = await client.get("/api/auth/devices", headers=_auth(token))
    device_ids = [d["device_id"] for d in devices_res.json()]
    assert "device-002" not in device_ids


async def test_unlink_device_not_found(client):
    token, _ = await _register_and_get_token(client)
    res = await client.delete("/api/auth/devices/nonexistent", headers=_auth(token))
    assert res.status_code == 404


async def test_unlink_last_device_blocked(client):
    """Cannot unlink the only remaining device."""
    token, _ = await _register_and_get_token(client)
    res = await client.delete("/api/auth/devices/device-001", headers=_auth(token))
    assert res.status_code == 400
    assert "last device" in res.json()["detail"].lower()


async def test_unlink_device_requires_auth(client):
    res = await client.delete("/api/auth/devices/device-001")
    assert res.status_code == 401


# ── Device limits ────────────────────────────────────────────────────


async def test_device_limit_free_account(client):
    """Free accounts are limited to 3 devices."""
    token, _ = await _register_and_get_token(client)
    # device-001 already linked from registration
    for i in range(2, 4):
        res = await client.post(
            "/api/auth/link-device",
            json={"device_id": f"device-{i:03d}"},
            headers=_auth(token),
        )
        assert res.status_code == 200, f"Linking device-{i:03d} failed"

    # 4th device should be rejected
    res = await client.post(
        "/api/auth/link-device",
        json={"device_id": "device-004"},
        headers=_auth(token),
    )
    assert res.status_code == 403
    assert "maximum" in res.json()["detail"].lower()
