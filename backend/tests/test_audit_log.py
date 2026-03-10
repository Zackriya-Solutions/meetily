"""Tests for audit logging."""

import pytest
from datetime import datetime, timezone

pytestmark = pytest.mark.usefixtures("cleanup_db")


# ── log_event ────────────────────────────────────────────────────────


async def test_log_event_creates_entry():
    from audit_log import log_event
    from mongodb import get_audit_log_collection

    await log_event(
        event_type="login",
        user_id="user-123",
        email="test@example.com",
        ip="127.0.0.1",
    )

    col = get_audit_log_collection()
    entry = await col.find_one({"event_type": "login", "user_id": "user-123"})
    assert entry is not None
    assert entry["email"] == "test@example.com"
    assert entry["ip"] == "127.0.0.1"


async def test_log_event_with_metadata():
    from audit_log import log_event
    from mongodb import get_audit_log_collection

    await log_event(
        event_type="failed_login",
        user_id="user-456",
        metadata={"reason": "wrong_password"},
    )

    col = get_audit_log_collection()
    entry = await col.find_one({"user_id": "user-456"})
    assert entry is not None
    assert entry["metadata"]["reason"] == "wrong_password"


async def test_log_event_without_metadata():
    from audit_log import log_event
    from mongodb import get_audit_log_collection

    await log_event(event_type="logout", user_id="user-789")

    col = get_audit_log_collection()
    entry = await col.find_one({"user_id": "user-789"})
    assert entry is not None
    assert entry["metadata"] == {}


async def test_log_event_stores_timestamp():
    from audit_log import log_event
    from mongodb import get_audit_log_collection

    before = datetime.now(timezone.utc)
    await log_event(event_type="register", user_id="user-ts")
    after = datetime.now(timezone.utc)

    col = get_audit_log_collection()
    entry = await col.find_one({"user_id": "user-ts"})
    assert entry is not None
    # MongoDB returns naive datetimes (UTC without tzinfo), so strip tz for comparison
    ts = entry["timestamp"]
    assert before.replace(tzinfo=None) <= ts.replace(tzinfo=None) <= after.replace(tzinfo=None)


async def test_log_event_optional_fields():
    from audit_log import log_event
    from mongodb import get_audit_log_collection

    await log_event(event_type="test_event")

    col = get_audit_log_collection()
    entry = await col.find_one({"event_type": "test_event"})
    assert entry is not None
    assert entry["user_id"] is None
    assert entry["email"] is None
    assert entry["ip"] is None


async def test_log_event_multiple_entries():
    from audit_log import log_event
    from mongodb import get_audit_log_collection

    await log_event(event_type="event_a", user_id="multi-user")
    await log_event(event_type="event_b", user_id="multi-user")

    col = get_audit_log_collection()
    count = await col.count_documents({"user_id": "multi-user"})
    assert count == 2


async def test_log_event_different_event_types():
    from audit_log import log_event
    from mongodb import get_audit_log_collection

    event_types = [
        "login", "failed_login", "register", "logout",
        "password_reset_requested", "password_reset_completed",
        "password_changed", "email_verified", "account_deactivated", "account_deleted",
    ]
    for et in event_types:
        await log_event(event_type=et, user_id=f"user-{et}")

    col = get_audit_log_collection()
    for et in event_types:
        entry = await col.find_one({"event_type": et})
        assert entry is not None, f"Missing audit entry for {et}"


# ── Filtering / retrieval ────────────────────────────────────────────


async def test_query_audit_by_user():
    from audit_log import log_event
    from mongodb import get_audit_log_collection

    await log_event(event_type="login", user_id="filter-user-1")
    await log_event(event_type="login", user_id="filter-user-2")

    col = get_audit_log_collection()
    entries = []
    async for doc in col.find({"user_id": "filter-user-1"}):
        entries.append(doc)
    assert len(entries) == 1


async def test_query_audit_by_event_type():
    from audit_log import log_event
    from mongodb import get_audit_log_collection

    await log_event(event_type="special_event", user_id="type-user")
    await log_event(event_type="other_event", user_id="type-user")

    col = get_audit_log_collection()
    entries = []
    async for doc in col.find({"event_type": "special_event"}):
        entries.append(doc)
    assert len(entries) == 1


# ── Graceful failure ──────────────────────────────────────────────


async def test_log_event_never_raises():
    """log_event should never raise, even on insert failure."""
    from audit_log import log_event
    from unittest.mock import patch, AsyncMock

    # Patch the collection to raise on insert
    mock_col = AsyncMock()
    mock_col.insert_one.side_effect = Exception("DB down")

    with patch("audit_log.get_audit_log_collection", return_value=mock_col):
        # Should not raise
        await log_event(event_type="test", user_id="u1")


# ── All fields populated ─────────────────────────────────────────


async def test_log_event_all_fields_populated():
    from audit_log import log_event
    from mongodb import get_audit_log_collection

    await log_event(
        event_type="full_event",
        user_id="full-user",
        email="full@example.com",
        ip="192.168.1.1",
        metadata={"key1": "val1", "key2": 42},
    )

    col = get_audit_log_collection()
    entry = await col.find_one({"event_type": "full_event"})
    assert entry["user_id"] == "full-user"
    assert entry["email"] == "full@example.com"
    assert entry["ip"] == "192.168.1.1"
    assert entry["metadata"]["key1"] == "val1"
    assert entry["metadata"]["key2"] == 42
    assert "timestamp" in entry


async def test_log_event_empty_metadata_defaults_to_dict():
    from audit_log import log_event
    from mongodb import get_audit_log_collection

    await log_event(event_type="empty_meta", user_id="meta-user")

    col = get_audit_log_collection()
    entry = await col.find_one({"event_type": "empty_meta"})
    assert entry["metadata"] == {}
