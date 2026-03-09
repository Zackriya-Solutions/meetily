"""Tests for login rate limiting."""

import pytest
from conftest import _register_payload, _login_payload

pytestmark = pytest.mark.asyncio


async def test_lockout_after_5_failures(client):
    """After 5 failed logins, the account should be locked for 15 minutes."""
    # Register and verify email
    await client.post("/api/auth/register", json=_register_payload())
    from mongodb import get_users_collection
    await get_users_collection().update_one(
        {"email": "test@example.com"}, {"$set": {"email_verified": True}}
    )

    # Fail 5 times
    for _ in range(5):
        res = await client.post(
            "/api/auth/login",
            json=_login_payload(password="WrongPass1"),
        )
        assert res.status_code == 401

    # 6th attempt should be rate-limited
    res = await client.post(
        "/api/auth/login",
        json=_login_payload(password="WrongPass1"),
    )
    assert res.status_code == 429
    assert "locked" in res.json()["detail"].lower()


async def test_successful_login_clears_attempts(client):
    """A successful login should clear the failed attempt counter."""
    await client.post("/api/auth/register", json=_register_payload())
    from mongodb import get_users_collection
    await get_users_collection().update_one(
        {"email": "test@example.com"}, {"$set": {"email_verified": True}}
    )

    # Fail 3 times
    for _ in range(3):
        await client.post("/api/auth/login", json=_login_payload(password="WrongPass1"))

    # Succeed
    res = await client.post("/api/auth/login", json=_login_payload())
    assert res.status_code == 200

    # Fail 3 more times — should NOT be locked (counter was reset)
    for _ in range(3):
        res = await client.post("/api/auth/login", json=_login_payload(password="WrongPass1"))
        assert res.status_code == 401
