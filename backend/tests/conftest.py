"""
Shared test fixtures for auth and usage tests.

Requires a local MongoDB instance on localhost:27017.
Uses a dedicated test database that is cleaned between tests.
"""

import os
import sys
import pytest
import pytest_asyncio

# Add the app directory to the path
sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "app"))

# Set test environment variables BEFORE any app imports
os.environ["DEPLOYMENT_MODE"] = "cloud"
os.environ["JWT_SECRET"] = "a" * 32
os.environ["MONGODB_URI"] = "mongodb://localhost:27017"
os.environ["MONGODB_DATABASE"] = "iqcapture_test"
os.environ["SENDGRID_API_KEY"] = ""  # Skip actual email sending in tests


@pytest_asyncio.fixture
async def client():
    """Create an async test client for the FastAPI app."""
    from httpx import AsyncClient, ASGITransport
    from main import app

    transport = ASGITransport(app=app)
    async with AsyncClient(transport=transport, base_url="http://test") as ac:
        yield ac


@pytest_asyncio.fixture(autouse=True)
async def cleanup_db():
    """Clean the test database before and after each test."""
    from mongodb import get_mongo_client

    db = get_mongo_client()["iqcapture_test"]
    collection_names = await db.list_collection_names()
    for name in collection_names:
        await db[name].delete_many({})
    yield
    collection_names = await db.list_collection_names()
    for name in collection_names:
        await db[name].delete_many({})


def _register_payload(email="test@example.com", password="TestPass1", device_id="device-001"):
    return {"email": email, "password": password, "device_id": device_id}


def _login_payload(email="test@example.com", password="TestPass1", device_id="device-001"):
    return {"email": email, "password": password, "device_id": device_id}
