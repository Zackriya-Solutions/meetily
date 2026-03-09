"""Tests for authentication endpoints."""

import pytest
from helpers import _register_payload, _login_payload

pytestmark = pytest.mark.asyncio


async def test_register_success(client):
    res = await client.post("/api/auth/register", json=_register_payload())
    assert res.status_code == 200
    data = res.json()
    assert "access_token" in data
    assert "refresh_token" in data
    assert data["user"]["email"] == "test@example.com"
    assert data["user"]["email_verified"] is False
    assert data["user"]["account_level"] == "free"


async def test_register_duplicate_email(client):
    await client.post("/api/auth/register", json=_register_payload())
    res = await client.post("/api/auth/register", json=_register_payload())
    assert res.status_code == 409


async def test_register_weak_password(client):
    res = await client.post(
        "/api/auth/register",
        json=_register_payload(password="weak"),
    )
    assert res.status_code == 422


async def test_login_unverified_blocked(client):
    """Login should fail with 403 if email is not verified."""
    await client.post("/api/auth/register", json=_register_payload())
    res = await client.post("/api/auth/login", json=_login_payload())
    assert res.status_code == 403
    assert "not verified" in res.json()["detail"].lower()


async def test_login_after_verification(client):
    """Login should succeed after email verification."""
    # Register
    await client.post("/api/auth/register", json=_register_payload())

    # Manually verify email in DB
    from mongodb import get_users_collection
    col = get_users_collection()
    await col.update_one({"email": "test@example.com"}, {"$set": {"email_verified": True}})

    res = await client.post("/api/auth/login", json=_login_payload())
    assert res.status_code == 200
    assert "access_token" in res.json()


async def test_login_wrong_password(client):
    await client.post("/api/auth/register", json=_register_payload())
    from mongodb import get_users_collection
    await get_users_collection().update_one(
        {"email": "test@example.com"}, {"$set": {"email_verified": True}}
    )
    res = await client.post(
        "/api/auth/login",
        json=_login_payload(password="WrongPass1"),
    )
    assert res.status_code == 401


async def test_login_nonexistent_email(client):
    res = await client.post(
        "/api/auth/login",
        json=_login_payload(email="nobody@example.com"),
    )
    assert res.status_code == 401


async def test_forgot_password(client):
    await client.post("/api/auth/register", json=_register_payload())
    res = await client.post(
        "/api/auth/forgot-password",
        json={"email": "test@example.com"},
    )
    assert res.status_code == 200
    # Should always return the same message (no email enumeration)
    assert "reset code" in res.json()["message"].lower()


async def test_forgot_password_unknown_email(client):
    res = await client.post(
        "/api/auth/forgot-password",
        json={"email": "unknown@example.com"},
    )
    assert res.status_code == 200


async def test_reset_password_invalid_code(client):
    await client.post("/api/auth/register", json=_register_payload())
    await client.post("/api/auth/forgot-password", json={"email": "test@example.com"})
    res = await client.post(
        "/api/auth/reset-password",
        json={"email": "test@example.com", "code": "000000", "new_password": "NewPass1"},
    )
    assert res.status_code == 400


async def test_change_password(client):
    # Register and verify
    reg = await client.post("/api/auth/register", json=_register_payload())
    token = reg.json()["access_token"]

    res = await client.post(
        "/api/auth/change-password",
        json={"current_password": "TestPass1", "new_password": "NewPass1x"},
        headers={"Authorization": f"Bearer {token}"},
    )
    assert res.status_code == 200


async def test_change_password_wrong_current(client):
    reg = await client.post("/api/auth/register", json=_register_payload())
    token = reg.json()["access_token"]

    res = await client.post(
        "/api/auth/change-password",
        json={"current_password": "WrongPass1", "new_password": "NewPass1x"},
        headers={"Authorization": f"Bearer {token}"},
    )
    assert res.status_code == 401


async def test_profile_update(client):
    reg = await client.post("/api/auth/register", json=_register_payload())
    token = reg.json()["access_token"]

    res = await client.put(
        "/api/auth/profile",
        json={"display_name": "New Name"},
        headers={"Authorization": f"Bearer {token}"},
    )
    assert res.status_code == 200


async def test_delete_account(client):
    reg = await client.post("/api/auth/register", json=_register_payload())
    token = reg.json()["access_token"]

    res = await client.delete(
        "/api/auth/account",
        headers={"Authorization": f"Bearer {token}"},
    )
    assert res.status_code == 200

    # Verify user is gone
    from mongodb import get_users_collection
    user = await get_users_collection().find_one({"email": "test@example.com"})
    assert user is None
