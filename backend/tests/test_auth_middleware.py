"""Tests for JWT auth middleware: token creation, decoding, and validation."""

import pytest
import datetime
import os
import sys
import jwt as pyjwt

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "app"))

from auth_middleware import (
    create_access_token,
    create_refresh_token,
    _decode_token,
    get_current_user,
    get_optional_user,
    JWT_SECRET,
    JWT_ALGORITHM,
    ACCESS_TOKEN_EXPIRE_MINUTES,
    REFRESH_TOKEN_EXPIRE_DAYS,
)
from fastapi import HTTPException

pytestmark = pytest.mark.asyncio


# ── create_access_token ─────────────────────────────────────────────


def test_create_access_token_returns_string():
    token = create_access_token("user-123", "device-abc")
    assert isinstance(token, str)
    assert len(token) > 0


def test_access_token_contains_correct_claims():
    token = create_access_token("user-123", "device-abc")
    payload = pyjwt.decode(token, JWT_SECRET, algorithms=[JWT_ALGORITHM])
    assert payload["sub"] == "user-123"
    assert payload["device_id"] == "device-abc"
    assert payload["type"] == "access"
    assert "iat" in payload
    assert "exp" in payload


def test_access_token_expiry():
    token = create_access_token("user-123", "device-abc")
    payload = pyjwt.decode(token, JWT_SECRET, algorithms=[JWT_ALGORITHM])
    iat = datetime.datetime.fromtimestamp(payload["iat"], tz=datetime.timezone.utc)
    exp = datetime.datetime.fromtimestamp(payload["exp"], tz=datetime.timezone.utc)
    delta = exp - iat
    assert abs(delta.total_seconds() - ACCESS_TOKEN_EXPIRE_MINUTES * 60) < 5


# ── create_refresh_token ────────────────────────────────────────────


def test_create_refresh_token_returns_string():
    token = create_refresh_token("user-123", "device-abc")
    assert isinstance(token, str)


def test_refresh_token_contains_correct_claims():
    token = create_refresh_token("user-123", "device-abc", family_id="fam-1")
    payload = pyjwt.decode(token, JWT_SECRET, algorithms=[JWT_ALGORITHM])
    assert payload["sub"] == "user-123"
    assert payload["device_id"] == "device-abc"
    assert payload["type"] == "refresh"
    assert payload["family_id"] == "fam-1"


def test_refresh_token_auto_generates_family_id():
    token = create_refresh_token("user-123", "device-abc")
    payload = pyjwt.decode(token, JWT_SECRET, algorithms=[JWT_ALGORITHM])
    assert "family_id" in payload
    assert len(payload["family_id"]) > 0


def test_refresh_token_expiry():
    token = create_refresh_token("user-123", "device-abc")
    payload = pyjwt.decode(token, JWT_SECRET, algorithms=[JWT_ALGORITHM])
    iat = datetime.datetime.fromtimestamp(payload["iat"], tz=datetime.timezone.utc)
    exp = datetime.datetime.fromtimestamp(payload["exp"], tz=datetime.timezone.utc)
    delta = exp - iat
    assert abs(delta.total_seconds() - REFRESH_TOKEN_EXPIRE_DAYS * 86400) < 5


# ── _decode_token ────────────────────────────────────────────────────


def test_decode_valid_access_token():
    token = create_access_token("user-1", "dev-1")
    payload = _decode_token(token, expected_type="access")
    assert payload["sub"] == "user-1"
    assert payload["type"] == "access"


def test_decode_valid_refresh_token():
    token = create_refresh_token("user-1", "dev-1")
    payload = _decode_token(token, expected_type="refresh")
    assert payload["sub"] == "user-1"
    assert payload["type"] == "refresh"


def test_decode_wrong_type_raises():
    token = create_access_token("user-1", "dev-1")
    with pytest.raises(HTTPException) as exc_info:
        _decode_token(token, expected_type="refresh")
    assert exc_info.value.status_code == 401
    assert "Expected refresh token" in exc_info.value.detail


def test_decode_expired_token_raises():
    now = datetime.datetime.now(datetime.timezone.utc)
    payload = {
        "sub": "user-1",
        "device_id": "dev-1",
        "type": "access",
        "iat": now - datetime.timedelta(hours=2),
        "exp": now - datetime.timedelta(hours=1),
    }
    token = pyjwt.encode(payload, JWT_SECRET, algorithm=JWT_ALGORITHM)
    with pytest.raises(HTTPException) as exc_info:
        _decode_token(token, expected_type="access")
    assert exc_info.value.status_code == 401
    assert "expired" in exc_info.value.detail.lower()


def test_decode_invalid_token_raises():
    with pytest.raises(HTTPException) as exc_info:
        _decode_token("not.a.valid.token", expected_type="access")
    assert exc_info.value.status_code == 401


def test_decode_malformed_jwt():
    with pytest.raises(HTTPException) as exc_info:
        _decode_token("completelygarbage", expected_type="access")
    assert exc_info.value.status_code == 401


def test_decode_token_wrong_secret():
    now = datetime.datetime.now(datetime.timezone.utc)
    payload = {
        "sub": "user-1",
        "device_id": "dev-1",
        "type": "access",
        "iat": now,
        "exp": now + datetime.timedelta(hours=1),
    }
    token = pyjwt.encode(payload, "wrong-secret-key-that-is-long-enough", algorithm=JWT_ALGORITHM)
    with pytest.raises(HTTPException) as exc_info:
        _decode_token(token, expected_type="access")
    assert exc_info.value.status_code == 401


def test_decode_token_missing_type_field():
    now = datetime.datetime.now(datetime.timezone.utc)
    payload = {
        "sub": "user-1",
        "iat": now,
        "exp": now + datetime.timedelta(hours=1),
    }
    token = pyjwt.encode(payload, JWT_SECRET, algorithm=JWT_ALGORITHM)
    with pytest.raises(HTTPException) as exc_info:
        _decode_token(token, expected_type="access")
    assert exc_info.value.status_code == 401


# ── get_current_user ─────────────────────────────────────────────────


async def test_get_current_user_no_credentials():
    with pytest.raises(HTTPException) as exc_info:
        await get_current_user(credentials=None)
    assert exc_info.value.status_code == 401
    assert "Authentication required" in exc_info.value.detail


async def test_get_current_user_valid_token():
    from unittest.mock import MagicMock

    token = create_access_token("user-1", "dev-1")
    creds = MagicMock()
    creds.credentials = token
    result = await get_current_user(credentials=creds)
    assert result["sub"] == "user-1"


async def test_get_current_user_invalid_token():
    from unittest.mock import MagicMock

    creds = MagicMock()
    creds.credentials = "invalid-token"
    with pytest.raises(HTTPException) as exc_info:
        await get_current_user(credentials=creds)
    assert exc_info.value.status_code == 401


# ── get_optional_user ────────────────────────────────────────────────


async def test_get_optional_user_no_credentials():
    result = await get_optional_user(credentials=None)
    assert result is None


async def test_get_optional_user_valid_token():
    from unittest.mock import MagicMock

    token = create_access_token("user-1", "dev-1")
    creds = MagicMock()
    creds.credentials = token
    result = await get_optional_user(credentials=creds)
    assert result["sub"] == "user-1"


async def test_get_optional_user_invalid_token():
    from unittest.mock import MagicMock

    creds = MagicMock()
    creds.credentials = "bad-token"
    result = await get_optional_user(credentials=creds)
    assert result is None


# ── Additional edge cases ──────────────────────────────────────────


def test_access_token_different_users_differ():
    token1 = create_access_token("user-1", "dev-1")
    token2 = create_access_token("user-2", "dev-1")
    assert token1 != token2


def test_refresh_token_different_families():
    token1 = create_refresh_token("user-1", "dev-1", family_id="fam-a")
    token2 = create_refresh_token("user-1", "dev-1", family_id="fam-b")
    payload1 = pyjwt.decode(token1, JWT_SECRET, algorithms=[JWT_ALGORITHM])
    payload2 = pyjwt.decode(token2, JWT_SECRET, algorithms=[JWT_ALGORITHM])
    assert payload1["family_id"] != payload2["family_id"]


def test_decode_token_with_extra_claims():
    """Extra claims in token should not cause decode to fail."""
    now = datetime.datetime.now(datetime.timezone.utc)
    payload = {
        "sub": "user-1",
        "device_id": "dev-1",
        "type": "access",
        "iat": now,
        "exp": now + datetime.timedelta(hours=1),
        "extra_claim": "extra_value",
    }
    token = pyjwt.encode(payload, JWT_SECRET, algorithm=JWT_ALGORITHM)
    result = _decode_token(token, expected_type="access")
    assert result["sub"] == "user-1"
    assert result["extra_claim"] == "extra_value"


def test_decode_access_token_as_refresh_fails():
    """An access token should not be accepted as a refresh token."""
    token = create_access_token("user-1", "dev-1")
    with pytest.raises(HTTPException) as exc_info:
        _decode_token(token, expected_type="refresh")
    assert exc_info.value.status_code == 401


def test_decode_refresh_token_as_access_fails():
    """A refresh token should not be accepted as an access token."""
    token = create_refresh_token("user-1", "dev-1")
    with pytest.raises(HTTPException) as exc_info:
        _decode_token(token, expected_type="access")
    assert exc_info.value.status_code == 401


async def test_get_current_user_with_refresh_token():
    """Using a refresh token for get_current_user should fail."""
    from unittest.mock import MagicMock

    token = create_refresh_token("user-1", "dev-1")
    creds = MagicMock()
    creds.credentials = token
    with pytest.raises(HTTPException) as exc_info:
        await get_current_user(credentials=creds)
    assert exc_info.value.status_code == 401


def test_create_access_token_is_decodable():
    """Token should be decodable with the same secret."""
    token = create_access_token("user-abc", "device-xyz")
    payload = pyjwt.decode(token, JWT_SECRET, algorithms=[JWT_ALGORITHM])
    assert payload["sub"] == "user-abc"
    assert payload["device_id"] == "device-xyz"


def test_create_refresh_token_expiry_is_30_days():
    token = create_refresh_token("user-1", "dev-1")
    payload = pyjwt.decode(token, JWT_SECRET, algorithms=[JWT_ALGORITHM])
    iat = datetime.datetime.fromtimestamp(payload["iat"], tz=datetime.timezone.utc)
    exp = datetime.datetime.fromtimestamp(payload["exp"], tz=datetime.timezone.utc)
    delta = exp - iat
    assert abs(delta.days - REFRESH_TOKEN_EXPIRE_DAYS) <= 1
