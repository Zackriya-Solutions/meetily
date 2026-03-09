"""Tests for release endpoints."""

import pytest

pytestmark = pytest.mark.asyncio


def _release_payload(version="1.0.0", is_latest=True, **kwargs):
    data = {
        "version": version,
        "is_latest": is_latest,
        "release_notes": "Bug fixes and improvements",
        "whats_new": ["Feature A", "Feature B"],
        "download_url": "https://example.com/download/1.0.0",
        "platform": "all",
    }
    data.update(kwargs)
    return data


# ── GET /api/releases/latest ────────────────────────────────────────


async def test_latest_release_no_releases(client):
    res = await client.get("/api/releases/latest")
    assert res.status_code == 200
    data = res.json()
    assert data["available"] is False


async def test_latest_release_with_version(client):
    await client.post("/api/releases", json=_release_payload("2.0.0"))
    res = await client.get("/api/releases/latest?current_version=1.0.0")
    assert res.status_code == 200
    data = res.json()
    assert data["available"] is True
    assert data["version"] == "2.0.0"


async def test_latest_release_no_update_needed(client):
    await client.post("/api/releases", json=_release_payload("1.0.0"))
    res = await client.get("/api/releases/latest?current_version=1.0.0")
    assert res.status_code == 200
    assert res.json()["available"] is False


async def test_latest_release_without_current_version(client):
    await client.post("/api/releases", json=_release_payload("1.5.0"))
    res = await client.get("/api/releases/latest")
    assert res.status_code == 200
    data = res.json()
    assert data["version"] == "1.5.0"
    assert data["available"] is False  # No current_version to compare


async def test_latest_release_includes_metadata(client):
    await client.post("/api/releases", json=_release_payload("3.0.0"))
    res = await client.get("/api/releases/latest?current_version=1.0.0")
    data = res.json()
    assert data["release_notes"] == "Bug fixes and improvements"
    assert "Feature A" in data["whats_new"]
    assert data["download_url"] == "https://example.com/download/1.0.0"


# ── GET /api/releases (list) ────────────────────────────────────────


async def test_list_releases_empty(client):
    res = await client.get("/api/releases")
    assert res.status_code == 200
    assert res.json() == []


async def test_list_releases_returns_created(client):
    await client.post("/api/releases", json=_release_payload("1.0.0"))
    await client.post("/api/releases", json=_release_payload("1.1.0", is_latest=False))
    res = await client.get("/api/releases")
    assert res.status_code == 200
    assert len(res.json()) == 2


async def test_list_releases_with_limit(client):
    for i in range(5):
        await client.post("/api/releases", json=_release_payload(f"1.{i}.0", is_latest=(i == 4)))
    res = await client.get("/api/releases?limit=3")
    assert res.status_code == 200
    assert len(res.json()) == 3


# ── POST /api/releases (create) ─────────────────────────────────────


async def test_create_release(client):
    res = await client.post("/api/releases", json=_release_payload("1.0.0"))
    assert res.status_code == 201
    assert res.json()["version"] == "1.0.0"


async def test_create_release_duplicate_conflict(client):
    await client.post("/api/releases", json=_release_payload("1.0.0"))
    res = await client.post("/api/releases", json=_release_payload("1.0.0"))
    assert res.status_code == 409


async def test_create_release_unmarks_previous_latest(client):
    await client.post("/api/releases", json=_release_payload("1.0.0", is_latest=True))
    await client.post("/api/releases", json=_release_payload("2.0.0", is_latest=True))

    # Check that latest is now 2.0.0
    res = await client.get("/api/releases/latest?current_version=0.1.0")
    assert res.json()["version"] == "2.0.0"


async def test_create_release_non_latest(client):
    res = await client.post("/api/releases", json=_release_payload("0.9.0", is_latest=False))
    assert res.status_code == 201
    # Should not show as latest
    latest = await client.get("/api/releases/latest")
    assert latest.json()["available"] is False


# ── _is_newer semver comparison ──────────────────────────────────────


def test_is_newer_true():
    import sys, os
    sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "app"))
    from release_routes import _is_newer
    assert _is_newer("2.0.0", "1.0.0") is True
    assert _is_newer("1.1.0", "1.0.0") is True
    assert _is_newer("1.0.1", "1.0.0") is True


def test_is_newer_false():
    from release_routes import _is_newer
    assert _is_newer("1.0.0", "1.0.0") is False
    assert _is_newer("1.0.0", "2.0.0") is False


def test_is_newer_malformed():
    from release_routes import _is_newer
    # Malformed versions fall back to string comparison
    result = _is_newer("abc", "def")
    assert isinstance(result, bool)


def test_is_newer_patch_version():
    from release_routes import _is_newer
    assert _is_newer("1.0.2", "1.0.1") is True
    assert _is_newer("1.0.1", "1.0.2") is False


def test_is_newer_minor_version():
    from release_routes import _is_newer
    assert _is_newer("1.2.0", "1.1.0") is True
    assert _is_newer("1.1.0", "1.2.0") is False


def test_is_newer_major_version():
    from release_routes import _is_newer
    assert _is_newer("3.0.0", "2.9.9") is True


# ── Additional release endpoint tests ─────────────────────────────


async def test_create_release_with_min_version(client):
    payload = _release_payload("1.0.0", min_version="0.5.0")
    res = await client.post("/api/releases", json=payload)
    assert res.status_code == 201


async def test_create_release_minimal_fields(client):
    """Creating a release with only required fields should succeed."""
    res = await client.post("/api/releases", json={"version": "0.1.0"})
    assert res.status_code == 201
    data = res.json()
    assert data["version"] == "0.1.0"
    assert data["is_latest"] is False


async def test_latest_release_returns_newest_when_multiple(client):
    """When multiple are marked latest, should return the most recent."""
    await client.post("/api/releases", json=_release_payload("1.0.0", is_latest=True))
    await client.post("/api/releases", json=_release_payload("2.0.0", is_latest=True))
    await client.post("/api/releases", json=_release_payload("3.0.0", is_latest=True))
    res = await client.get("/api/releases/latest?current_version=0.1.0")
    assert res.json()["version"] == "3.0.0"
    assert res.json()["available"] is True
