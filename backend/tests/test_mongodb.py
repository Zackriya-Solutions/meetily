"""Tests for MongoDB connection management."""

import pytest
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "app"))

pytestmark = pytest.mark.asyncio


# ── get_mongo_client ─────────────────────────────────────────────────


def test_get_mongo_client_returns_client():
    from mongodb import get_mongo_client
    client = get_mongo_client()
    assert client is not None


def test_get_mongo_client_is_singleton():
    from mongodb import get_mongo_client
    client1 = get_mongo_client()
    client2 = get_mongo_client()
    assert client1 is client2


# ── _get_db ──────────────────────────────────────────────────────────


def test_get_db_uses_env_var():
    from mongodb import _get_db
    db = _get_db()
    assert db.name == os.getenv("MONGODB_DATABASE", "iqcapture")


# ── Collection accessors ────────────────────────────────────────────


def test_get_templates_collection():
    from mongodb import get_templates_collection
    col = get_templates_collection()
    assert col.name == "templates"


def test_get_users_collection():
    from mongodb import get_users_collection
    col = get_users_collection()
    assert col.name == "users"


def test_get_usage_events_collection():
    from mongodb import get_usage_events_collection
    col = get_usage_events_collection()
    assert col.name == "usage_events"


def test_get_usage_summaries_collection():
    from mongodb import get_usage_summaries_collection
    col = get_usage_summaries_collection()
    assert col.name == "usage_summaries"


def test_get_password_resets_collection():
    from mongodb import get_password_resets_collection
    col = get_password_resets_collection()
    assert col.name == "password_resets"


def test_get_login_attempts_collection():
    from mongodb import get_login_attempts_collection
    col = get_login_attempts_collection()
    assert col.name == "login_attempts"


def test_get_token_families_collection():
    from mongodb import get_token_families_collection
    col = get_token_families_collection()
    assert col.name == "token_families"


def test_get_email_verifications_collection():
    from mongodb import get_email_verifications_collection
    col = get_email_verifications_collection()
    assert col.name == "email_verifications"


def test_get_audit_log_collection():
    from mongodb import get_audit_log_collection
    col = get_audit_log_collection()
    assert col.name == "audit_log"


# ── check_mongo_connection ───────────────────────────────────────────


async def test_check_mongo_connection_success():
    from mongodb import check_mongo_connection
    result = await check_mongo_connection()
    assert result is True


# ── ensure_indexes ───────────────────────────────────────────────────


async def test_ensure_indexes_runs_without_error():
    from mongodb import ensure_indexes
    # Should not raise
    await ensure_indexes()


async def test_ensure_indexes_idempotent():
    from mongodb import ensure_indexes
    # Running twice should be safe
    await ensure_indexes()
    await ensure_indexes()
