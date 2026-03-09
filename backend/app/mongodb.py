"""
MongoDB connection module for IQ:capture backend.

Provides lazy singleton connection to MongoDB for template storage
and future synced resources. Designed to fail gracefully — the backend
starts even if MongoDB is unreachable.
"""

import os
import logging
from motor.motor_asyncio import AsyncIOMotorClient

logger = logging.getLogger(__name__)

_client: AsyncIOMotorClient | None = None


def get_mongo_client() -> AsyncIOMotorClient:
    """Get or create the singleton MongoDB client.

    Uses MONGODB_URI env var (default: mongodb://localhost:27017).
    Connection is lazy — no actual network call until first operation.
    """
    global _client
    if _client is None:
        uri = os.getenv("MONGODB_URI", "mongodb://localhost:27017")
        logger.info(f"Initialising MongoDB client with URI: {uri}")
        _client = AsyncIOMotorClient(uri, serverSelectionTimeoutMS=5000)
    return _client


def _get_db():
    client = get_mongo_client()
    db_name = os.getenv("MONGODB_DATABASE", "iqcapture")
    return client[db_name]


def get_templates_collection():
    """Get the templates collection from the iqcapture database."""
    return _get_db()["templates"]


def get_users_collection():
    """Get the users collection for authentication."""
    return _get_db()["users"]


def get_usage_events_collection():
    """Get the usage_events collection for raw event storage."""
    return _get_db()["usage_events"]


def get_usage_summaries_collection():
    """Get the usage_summaries collection for pre-aggregated rollups."""
    return _get_db()["usage_summaries"]


def get_password_resets_collection():
    """Get the password_resets collection for reset code storage."""
    return _get_db()["password_resets"]


def get_login_attempts_collection():
    """Get the login_attempts collection for rate limiting."""
    return _get_db()["login_attempts"]


def get_token_families_collection():
    """Get the token_families collection for refresh token revocation."""
    return _get_db()["token_families"]


def get_email_verifications_collection():
    """Get the email_verifications collection."""
    return _get_db()["email_verifications"]


def get_audit_log_collection():
    """Get the audit_log collection for structured auth event logging."""
    return _get_db()["audit_log"]


async def ensure_indexes():
    """Create all required indexes. Call once at startup in cloud mode."""
    db = _get_db()

    # users — unique email
    await db["users"].create_index("email", unique=True)

    # password_resets — TTL auto-cleanup + rate limit query
    await db["password_resets"].create_index("expires_at", expireAfterSeconds=0)
    await db["password_resets"].create_index([("email", 1), ("created_at", -1)])

    # email_verifications — TTL auto-cleanup
    await db["email_verifications"].create_index("expires_at", expireAfterSeconds=0)
    await db["email_verifications"].create_index([("email", 1), ("created_at", -1)])

    # login_attempts — TTL auto-cleanup (1 hour) + query
    await db["login_attempts"].create_index("attempted_at", expireAfterSeconds=3600)
    await db["login_attempts"].create_index([("email", 1), ("attempted_at", -1)])

    # token_families — lookup + user query
    await db["token_families"].create_index("family_id", unique=True)
    await db["token_families"].create_index("user_id")

    # usage_events — query + aggregation + deduplication
    await db["usage_events"].create_index([("user_id", 1), ("received_at", -1)])
    await db["usage_events"].create_index([("user_id", 1), ("aggregated", 1)])
    await db["usage_events"].create_index(
        [("user_id", 1), ("device_id", 1), ("client_event_id", 1)],
        unique=True,
        sparse=True,
    )

    # usage_summaries — compound unique
    await db["usage_summaries"].create_index(
        [("user_id", 1), ("device_id", 1), ("period_type", 1), ("period_start", 1)],
        unique=True,
    )

    # audit_log — query by user and event type
    await db["audit_log"].create_index([("user_id", 1), ("timestamp", -1)])
    await db["audit_log"].create_index("event_type")

    logger.info("MongoDB indexes ensured")


def get_meetings_collection():
    """Get the meetings collection for cloud-stored meetings (mobile)."""
    client = get_mongo_client()
    db_name = os.getenv("MONGODB_DATABASE", "iqcapture")
    return client[db_name]["meetings"]


def get_transcription_jobs_collection():
    """Get the transcription_jobs collection for cloud transcription tracking."""
    client = get_mongo_client()
    db_name = os.getenv("MONGODB_DATABASE", "iqcapture")
    return client[db_name]["transcription_jobs"]


async def check_mongo_connection() -> bool:
    """Check if MongoDB is reachable by sending a ping.

    Returns True if connected, False otherwise. Never raises.
    """
    try:
        client = get_mongo_client()
        await client.admin.command("ping")
        return True
    except Exception as e:
        logger.warning(f"MongoDB connection check failed: {e}")
        return False
