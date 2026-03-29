"""Tests that auth is mandatory regardless of DEPLOYMENT_MODE."""
import os
import pytest
from unittest.mock import patch, AsyncMock
from httpx import AsyncClient, ASGITransport


@pytest.fixture
def required_env(monkeypatch):
    """Set all required env vars for auth startup."""
    monkeypatch.setenv("JWT_SECRET", "test-secret-that-is-at-least-32-characters-long")
    monkeypatch.setenv("MONGODB_URI", "mongodb://localhost:27017/test")
    monkeypatch.setenv("SENDGRID_API_KEY", "SG.fake-key-for-testing")


@pytest.mark.asyncio
async def test_health_endpoint_returns_ok(required_env):
    """GET /health must be available and unauthenticated."""
    with patch("mongodb.check_mongo_connection", new_callable=AsyncMock, return_value=True), \
         patch("mongodb.ensure_indexes", new_callable=AsyncMock):
        import importlib
        import main
        importlib.reload(main)
        app = main.app
        transport = ASGITransport(app=app)
        async with AsyncClient(transport=transport, base_url="http://test") as client:
            resp = await client.get("/health")
            assert resp.status_code == 200
            data = resp.json()
            assert data["status"] == "ok"


@pytest.mark.asyncio
async def test_auth_routes_registered_in_local_mode(required_env, monkeypatch):
    """Auth endpoints must exist even when DEPLOYMENT_MODE=local."""
    monkeypatch.setenv("DEPLOYMENT_MODE", "local")
    with patch("mongodb.check_mongo_connection", new_callable=AsyncMock, return_value=True), \
         patch("mongodb.ensure_indexes", new_callable=AsyncMock):
        import importlib
        import main
        importlib.reload(main)
        app = main.app

        transport = ASGITransport(app=app)
        async with AsyncClient(transport=transport, base_url="http://test") as client:
            resp = await client.post("/api/auth/login")
            assert resp.status_code != 404, "Auth routes not registered in local mode"
