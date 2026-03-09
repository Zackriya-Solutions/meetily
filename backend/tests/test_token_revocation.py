"""Tests for refresh token family revocation."""

import pytest
from conftest import _register_payload, _login_payload

pytestmark = pytest.mark.asyncio


async def test_refresh_after_logout_fails(client):
    """After logout, using the old refresh token should fail."""
    # Register and verify
    await client.post("/api/auth/register", json=_register_payload())
    from mongodb import get_users_collection
    await get_users_collection().update_one(
        {"email": "test@example.com"}, {"$set": {"email_verified": True}}
    )

    # Login
    login_res = await client.post("/api/auth/login", json=_login_payload())
    tokens = login_res.json()
    access = tokens["access_token"]
    refresh = tokens["refresh_token"]

    # Logout
    await client.post(
        "/api/auth/logout",
        headers={"Authorization": f"Bearer {access}"},
    )

    # Try to refresh — should fail
    res = await client.post(
        "/api/auth/refresh",
        json={"refresh_token": refresh},
    )
    assert res.status_code == 401
    assert "revoked" in res.json()["detail"].lower()


async def test_refresh_rotation(client):
    """Refreshing should issue new tokens and revoke the old family."""
    await client.post("/api/auth/register", json=_register_payload())
    from mongodb import get_users_collection
    await get_users_collection().update_one(
        {"email": "test@example.com"}, {"$set": {"email_verified": True}}
    )

    login_res = await client.post("/api/auth/login", json=_login_payload())
    old_refresh = login_res.json()["refresh_token"]

    # Refresh once
    refresh_res = await client.post(
        "/api/auth/refresh",
        json={"refresh_token": old_refresh},
    )
    assert refresh_res.status_code == 200
    new_refresh = refresh_res.json()["refresh_token"]
    assert new_refresh != old_refresh

    # Old refresh token should now be revoked
    res = await client.post(
        "/api/auth/refresh",
        json={"refresh_token": old_refresh},
    )
    assert res.status_code == 401

    # New refresh token should work
    res = await client.post(
        "/api/auth/refresh",
        json={"refresh_token": new_refresh},
    )
    assert res.status_code == 200
