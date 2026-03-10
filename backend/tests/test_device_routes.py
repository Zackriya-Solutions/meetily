"""Tests for device management endpoints (protected by auth)."""

from datetime import datetime, timezone
from helpers import _register_payload


async def _get_auth_token(client):
    """Register a user and return a valid access token."""
    res = await client.post("/api/auth/register", json=_register_payload())
    return res.json()["access_token"]


def _auth_headers(token):
    return {"Authorization": f"Bearer {token}"}


async def _insert_device(user_id="dev-user-1", advanced_logs=False):
    """Insert a device directly into the devices collection."""
    from mongodb import get_mongo_client
    import os
    db_name = os.getenv("MONGODB_DATABASE", "iqcapture_test")
    col = get_mongo_client()[db_name]["devices"]
    now = datetime.now(timezone.utc)
    await col.insert_one({
        "user_id": user_id,
        "first_seen": now,
        "last_seen": now,
        "advanced_logs": advanced_logs,
        "platform": "windows",
        "app_version": "0.4.0",
    })


# ── GET /api/devices ────────────────────────────────────────────────


async def test_list_devices_requires_auth(client):
    """Unauthenticated request should return 401."""
    res = await client.get("/api/devices")
    assert res.status_code == 401


async def test_list_devices_empty(client):
    token = await _get_auth_token(client)
    res = await client.get("/api/devices", headers=_auth_headers(token))
    assert res.status_code == 200
    assert res.json() == []


async def test_list_devices_returns_inserted(client):
    token = await _get_auth_token(client)
    await _insert_device("dev-abc")
    res = await client.get("/api/devices", headers=_auth_headers(token))
    assert res.status_code == 200
    devices = res.json()
    assert len(devices) == 1
    assert devices[0]["user_id"] == "dev-abc"


async def test_list_devices_multiple(client):
    token = await _get_auth_token(client)
    await _insert_device("dev-1")
    await _insert_device("dev-2")
    res = await client.get("/api/devices", headers=_auth_headers(token))
    assert res.status_code == 200
    assert len(res.json()) == 2


async def test_list_devices_with_limit(client):
    token = await _get_auth_token(client)
    for i in range(5):
        await _insert_device(f"dev-{i}")
    res = await client.get("/api/devices?limit=3", headers=_auth_headers(token))
    assert res.status_code == 200
    assert len(res.json()) == 3


async def test_list_devices_datetime_serialization(client):
    token = await _get_auth_token(client)
    await _insert_device("dev-serial")
    res = await client.get("/api/devices", headers=_auth_headers(token))
    device = res.json()[0]
    assert isinstance(device.get("first_seen"), str)
    assert isinstance(device.get("last_seen"), str)


async def test_list_devices_id_is_string(client):
    token = await _get_auth_token(client)
    await _insert_device("dev-id-check")
    res = await client.get("/api/devices", headers=_auth_headers(token))
    device = res.json()[0]
    assert isinstance(device["_id"], str)


# ── PATCH /api/devices/advanced-logs ─────────────────────────────────


async def test_toggle_advanced_logs_requires_auth(client):
    """Unauthenticated request should return 401."""
    res = await client.patch(
        "/api/devices/advanced-logs",
        json={"user_id": "dev-toggle", "enabled": True},
    )
    assert res.status_code == 401


async def test_toggle_advanced_logs_enable(client):
    token = await _get_auth_token(client)
    await _insert_device("dev-toggle")
    res = await client.patch(
        "/api/devices/advanced-logs",
        json={"user_id": "dev-toggle", "enabled": True},
        headers=_auth_headers(token),
    )
    assert res.status_code == 200
    assert res.json()["advanced_logs"] is True


async def test_toggle_advanced_logs_disable(client):
    token = await _get_auth_token(client)
    await _insert_device("dev-toggle-off", advanced_logs=True)
    res = await client.patch(
        "/api/devices/advanced-logs",
        json={"user_id": "dev-toggle-off", "enabled": False},
        headers=_auth_headers(token),
    )
    assert res.status_code == 200
    assert res.json()["advanced_logs"] is False


async def test_toggle_advanced_logs_device_not_found(client):
    token = await _get_auth_token(client)
    res = await client.patch(
        "/api/devices/advanced-logs",
        json={"user_id": "nonexistent-device", "enabled": True},
        headers=_auth_headers(token),
    )
    assert res.status_code == 404


async def test_toggle_advanced_logs_updates_timestamp(client):
    from mongodb import get_mongo_client
    import os

    token = await _get_auth_token(client)
    await _insert_device("dev-ts")
    await client.patch(
        "/api/devices/advanced-logs",
        json={"user_id": "dev-ts", "enabled": True},
        headers=_auth_headers(token),
    )

    db_name = os.getenv("MONGODB_DATABASE", "iqcapture_test")
    col = get_mongo_client()[db_name]["devices"]
    device = await col.find_one({"user_id": "dev-ts"})
    assert "advanced_logs_toggled_at" in device


async def test_toggle_advanced_logs_missing_user_id(client):
    """Missing user_id field should return 422."""
    token = await _get_auth_token(client)
    res = await client.patch(
        "/api/devices/advanced-logs",
        json={"enabled": True},
        headers=_auth_headers(token),
    )
    assert res.status_code == 422


async def test_toggle_advanced_logs_missing_enabled(client):
    """Missing enabled field should return 422."""
    token = await _get_auth_token(client)
    res = await client.patch(
        "/api/devices/advanced-logs",
        json={"user_id": "some-device"},
        headers=_auth_headers(token),
    )
    assert res.status_code == 422


async def test_list_devices_includes_platform(client):
    token = await _get_auth_token(client)
    await _insert_device("dev-plat")
    res = await client.get("/api/devices", headers=_auth_headers(token))
    device = res.json()[0]
    assert "platform" in device
    assert device["platform"] == "windows"


async def test_list_devices_includes_app_version(client):
    token = await _get_auth_token(client)
    await _insert_device("dev-ver")
    res = await client.get("/api/devices", headers=_auth_headers(token))
    device = res.json()[0]
    assert device.get("app_version") == "0.4.0"


async def test_toggle_then_verify_in_list(client):
    """After toggling, the device should show updated advanced_logs value."""
    from mongodb import get_mongo_client
    import os

    token = await _get_auth_token(client)
    await _insert_device("dev-verify")
    await client.patch(
        "/api/devices/advanced-logs",
        json={"user_id": "dev-verify", "enabled": True},
        headers=_auth_headers(token),
    )

    db_name = os.getenv("MONGODB_DATABASE", "iqcapture_test")
    col = get_mongo_client()[db_name]["devices"]
    device = await col.find_one({"user_id": "dev-verify"})
    assert device["advanced_logs"] is True
