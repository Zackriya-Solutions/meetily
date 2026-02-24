import sys
import os
from pathlib import Path

import pytest
from fastapi import FastAPI
from fastapi.testclient import TestClient
from httpx import ASGITransport, AsyncClient

BACKEND_ROOT = Path(__file__).resolve().parents[1]
if str(BACKEND_ROOT) not in sys.path:
    sys.path.insert(0, str(BACKEND_ROOT))

os.environ.setdefault(
    "DATABASE_URL", "postgresql://postgres:postgres@localhost:5432/meeting_copilot"
)
os.environ.setdefault("CALENDAR_REMINDER_AUTOMATION_ENABLED", "false")
os.environ.setdefault("AUDIO_SESSION_RECONCILER_ENABLED", "false")

from app.api.deps import get_current_user
from app.api.routers import audio as audio_router
from app.api.routers import chat as chat_router
from app.api.routers import transcripts as transcripts_router
from app.schemas.user import User


@pytest.fixture(autouse=True)
def clear_overrides():
    yield


@pytest.fixture
def anyio_backend():
    return "asyncio"


@pytest.fixture
def test_user() -> User:
    return User(email="test@appointy.com", name="Test User")


@pytest.fixture
def test_app(test_user: User):
    app = FastAPI()
    app.include_router(audio_router.router)
    app.include_router(chat_router.router)
    app.include_router(transcripts_router.router)

    @app.get("/health")
    async def _health():
        return {"status": "ok"}

    async def _fake_current_user():
        return test_user

    app.dependency_overrides[get_current_user] = _fake_current_user
    return app


@pytest.fixture
async def async_client(test_app):
    transport = ASGITransport(app=test_app)
    async with AsyncClient(transport=transport, base_url="http://test") as client:
        yield client


@pytest.fixture
def client(test_app):
    with TestClient(test_app) as test_client:
        yield test_client
