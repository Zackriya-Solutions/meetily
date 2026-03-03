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


def get_templates_collection():
    """Get the templates collection from the iqcapture database.

    Database name from MONGODB_DATABASE env var (default: iqcapture).
    """
    client = get_mongo_client()
    db_name = os.getenv("MONGODB_DATABASE", "iqcapture")
    return client[db_name]["templates"]


def get_users_collection():
    """Get the users collection for authentication."""
    client = get_mongo_client()
    db_name = os.getenv("MONGODB_DATABASE", "iqcapture")
    return client[db_name]["users"]


def get_usage_events_collection():
    """Get the usage_events collection for raw event storage."""
    client = get_mongo_client()
    db_name = os.getenv("MONGODB_DATABASE", "iqcapture")
    return client[db_name]["usage_events"]


def get_usage_summaries_collection():
    """Get the usage_summaries collection for pre-aggregated rollups."""
    client = get_mongo_client()
    db_name = os.getenv("MONGODB_DATABASE", "iqcapture")
    return client[db_name]["usage_summaries"]


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
