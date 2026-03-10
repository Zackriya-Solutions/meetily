"""
Shared test fixtures for backend tests.

Requires a local MongoDB instance on localhost:27017.
Uses a dedicated test database that is cleaned between tests.
"""

import asyncio
import os
import sys
import pytest
import pytest_asyncio

# Add the app and tests directories to the path
sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "app"))
sys.path.insert(0, os.path.dirname(__file__))

# Set test environment variables BEFORE any app imports
os.environ["DEPLOYMENT_MODE"] = "cloud"
os.environ["JWT_SECRET"] = "a" * 32
os.environ["MONGODB_URI"] = "mongodb://localhost:27017"
os.environ["MONGODB_DATABASE"] = "iqcapture_test"
os.environ["SENDGRID_API_KEY"] = ""  # Skip actual email sending in tests


# ── Session-scoped event loop ────────────────────────────────────────
# Motor's AsyncIOMotorClient binds to the event loop on first use.
# With per-function loops (the default), the singleton client becomes
# stale after the first test closes its loop.  A single session-wide
# loop avoids the problem entirely.
@pytest.fixture(scope="session")
def event_loop():
    loop = asyncio.new_event_loop()
    yield loop
    loop.close()


# ── Fixtures ─────────────────────────────────────────────────────────
@pytest_asyncio.fixture
async def client(cleanup_db):
    """Create an async test client for the FastAPI app.

    Depends on cleanup_db to ensure a clean MongoDB state for each test.
    Also cleans the SQLite database so tests don't leak state.
    """
    from httpx import AsyncClient, ASGITransport
    from main import app, db as sqlite_db
    import aiosqlite

    # Clean SQLite tables between tests
    async with aiosqlite.connect(sqlite_db.db_path) as conn:
        for table in ("meetings", "transcripts", "summary_processes",
                       "transcript_chunks", "settings", "transcript_settings"):
            await conn.execute(f"DELETE FROM {table}")
        await conn.commit()

    transport = ASGITransport(app=app)
    async with AsyncClient(transport=transport, base_url="http://test") as ac:
        yield ac


@pytest_asyncio.fixture
async def cleanup_db():
    """Clean the MongoDB test database before and after each test.

    Drops the entire database (not just documents) so that unique indexes
    created by ensure_indexes() don't cause DuplicateKeyError in tests
    that insert documents without all indexed fields.
    """
    from mongodb import get_mongo_client

    mongo_client = get_mongo_client()
    await mongo_client.drop_database("iqcapture_test")
    yield
    await mongo_client.drop_database("iqcapture_test")


def _register_payload(email="test@example.com", password="TestPass1", device_id="device-001"):
    return {"email": email, "password": password, "device_id": device_id}


def _login_payload(email="test@example.com", password="TestPass1", device_id="device-001"):
    return {"email": email, "password": password, "device_id": device_id}
