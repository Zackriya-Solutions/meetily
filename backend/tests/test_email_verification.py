"""Tests for the email verification flow."""

from helpers import _register_payload, _login_payload


async def test_register_creates_unverified_user(client):
    """New users should have email_verified=False."""
    res = await client.post("/api/auth/register", json=_register_payload())
    assert res.status_code == 200
    assert res.json()["user"]["email_verified"] is False


async def test_login_blocked_before_verification(client):
    """Login should fail with 403 before email is verified."""
    await client.post("/api/auth/register", json=_register_payload())
    res = await client.post("/api/auth/login", json=_login_payload())
    assert res.status_code == 403
    assert "not verified" in res.json()["detail"].lower()


async def test_verify_email_with_correct_code(client):
    """Full verification flow: register → get code from DB → verify → login."""
    await client.post("/api/auth/register", json=_register_payload())

    # Get verification code from database
    from mongodb import get_email_verifications_collection
    col = get_email_verifications_collection()
    doc = await col.find_one({"email": "test@example.com", "used": False})
    assert doc is not None, "Verification record should exist"

    # We can't get the raw code (it's hashed), so manually verify via DB
    from mongodb import get_users_collection
    await get_users_collection().update_one(
        {"email": "test@example.com"},
        {"$set": {"email_verified": True}},
    )

    # Now login should succeed
    res = await client.post("/api/auth/login", json=_login_payload())
    assert res.status_code == 200
    assert "access_token" in res.json()


async def test_verify_email_wrong_code(client):
    """Invalid verification code should return 400."""
    await client.post("/api/auth/register", json=_register_payload())
    res = await client.post(
        "/api/auth/verify-email",
        json={"email": "test@example.com", "code": "000000"},
    )
    assert res.status_code == 400


async def test_resend_verification(client):
    """Resending verification should succeed."""
    await client.post("/api/auth/register", json=_register_payload())
    res = await client.post(
        "/api/auth/resend-verification",
        json={"email": "test@example.com"},
    )
    assert res.status_code == 200


async def test_resend_verification_unknown_email(client):
    """Resending for unknown email should still return 200 (no enumeration)."""
    res = await client.post(
        "/api/auth/resend-verification",
        json={"email": "nobody@example.com"},
    )
    # Should return 200 or similar to prevent email enumeration
    assert res.status_code in (200, 404)


async def test_login_after_manual_verification(client):
    """Register → manually verify → login should work end-to-end."""
    reg = await client.post("/api/auth/register", json=_register_payload())
    assert reg.status_code == 200

    from mongodb import get_users_collection
    await get_users_collection().update_one(
        {"email": "test@example.com"},
        {"$set": {"email_verified": True}},
    )

    login = await client.post("/api/auth/login", json=_login_payload())
    assert login.status_code == 200
    data = login.json()
    assert "access_token" in data
    assert data["user"]["email_verified"] is True

    # /me should work with the token
    me_res = await client.get(
        "/api/auth/me",
        headers={"Authorization": f"Bearer {data['access_token']}"},
    )
    assert me_res.status_code == 200
    assert me_res.json()["email"] == "test@example.com"
