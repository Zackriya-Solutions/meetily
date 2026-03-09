"""Tests for cloud configuration endpoint."""

import pytest

pytestmark = pytest.mark.asyncio


async def test_get_config_returns_expected_keys(client):
    res = await client.get("/api/config")
    assert res.status_code == 200
    data = res.json()
    assert "deployment_mode" in data
    assert "cloud_api_url" in data
    assert "version" in data


async def test_get_config_deployment_mode_value(client):
    """In test env, DEPLOYMENT_MODE is set to 'cloud'."""
    res = await client.get("/api/config")
    data = res.json()
    assert data["deployment_mode"] == "cloud"


async def test_get_config_version_is_string(client):
    res = await client.get("/api/config")
    data = res.json()
    assert isinstance(data["version"], str)
    assert len(data["version"]) > 0


async def test_get_config_cloud_api_url_is_string(client):
    res = await client.get("/api/config")
    data = res.json()
    assert isinstance(data["cloud_api_url"], str)
