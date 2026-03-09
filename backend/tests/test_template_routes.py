"""Tests for template CRUD endpoints."""

import pytest
from datetime import datetime

pytestmark = pytest.mark.asyncio


def _template_payload(template_id="tpl-1", client_id="default", name="Standard Template"):
    return {
        "template_id": template_id,
        "client_id": client_id,
        "name": name,
        "description": "A test template",
        "sections": [
            {
                "title": "Summary",
                "instruction": "Summarize the meeting",
                "format": "paragraph",
            }
        ],
        "version": 1,
        "is_active": True,
    }


# ── GET /api/templates/health ────────────────────────────────────────


async def test_templates_health(client):
    res = await client.get("/api/templates/health")
    assert res.status_code == 200
    assert res.json()["status"] == "ok"


# ── POST /api/templates (create) ────────────────────────────────────


async def test_create_template(client):
    res = await client.post("/api/templates", json=_template_payload())
    assert res.status_code == 201
    data = res.json()
    assert data["template_id"] == "tpl-1"
    assert data["name"] == "Standard Template"


async def test_create_template_duplicate_conflict(client):
    await client.post("/api/templates", json=_template_payload())
    res = await client.post("/api/templates", json=_template_payload())
    assert res.status_code == 409


async def test_create_template_different_client(client):
    await client.post("/api/templates", json=_template_payload())
    payload = _template_payload(client_id="client-2")
    res = await client.post("/api/templates", json=payload)
    assert res.status_code == 201


# ── GET /api/templates (list) ───────────────────────────────────────


async def test_list_templates_empty(client):
    res = await client.get("/api/templates")
    assert res.status_code == 200
    data = res.json()
    assert data["templates"] == []


async def test_list_templates_returns_created(client):
    await client.post("/api/templates", json=_template_payload())
    res = await client.get("/api/templates")
    assert res.status_code == 200
    data = res.json()
    assert len(data["templates"]) == 1
    assert data["templates"][0]["template_id"] == "tpl-1"


async def test_list_templates_sorted_by_name(client):
    await client.post("/api/templates", json=_template_payload(template_id="tpl-b", name="Beta"))
    await client.post("/api/templates", json=_template_payload(template_id="tpl-a", name="Alpha"))
    res = await client.get("/api/templates")
    names = [t["name"] for t in res.json()["templates"]]
    assert names == ["Alpha", "Beta"]


async def test_list_templates_client_override(client):
    # Create a default template
    await client.post("/api/templates", json=_template_payload(template_id="shared", name="Default Version"))
    # Create a client-specific override
    override = _template_payload(template_id="shared", client_id="client-x", name="Client Version")
    await client.post("/api/templates", json=override)

    # Fetching with client_id=client-x should show the override
    res = await client.get("/api/templates?client_id=client-x")
    assert res.status_code == 200
    templates = res.json()["templates"]
    assert len(templates) == 1
    assert templates[0]["name"] == "Client Version"


async def test_list_templates_excludes_inactive(client):
    payload = _template_payload(template_id="inactive-tpl")
    payload["is_active"] = False
    await client.post("/api/templates", json=payload)
    res = await client.get("/api/templates")
    assert len(res.json()["templates"]) == 0


async def test_list_templates_sync_timestamp(client):
    res = await client.get("/api/templates")
    data = res.json()
    assert "sync_timestamp" in data


# ── GET /api/templates/{id} ─────────────────────────────────────────


async def test_get_template_success(client):
    await client.post("/api/templates", json=_template_payload())
    res = await client.get("/api/templates/tpl-1")
    assert res.status_code == 200
    assert res.json()["template_id"] == "tpl-1"


async def test_get_template_not_found(client):
    res = await client.get("/api/templates/nonexistent")
    assert res.status_code == 404


async def test_get_template_client_override_precedence(client):
    await client.post("/api/templates", json=_template_payload(template_id="ovr", name="Default"))
    override = _template_payload(template_id="ovr", client_id="client-y", name="Override")
    await client.post("/api/templates", json=override)

    # Client-specific should take precedence
    res = await client.get("/api/templates/ovr?client_id=client-y")
    assert res.json()["name"] == "Override"

    # Default should still be accessible without client_id
    res = await client.get("/api/templates/ovr")
    assert res.json()["name"] == "Default"


# ── PUT /api/templates/{id} ─────────────────────────────────────────


async def test_update_template(client):
    await client.post("/api/templates", json=_template_payload())
    updated = _template_payload(name="Updated Template")
    res = await client.put("/api/templates/tpl-1", json=updated)
    assert res.status_code == 200
    assert res.json()["name"] == "Updated Template"


async def test_update_template_not_found(client):
    payload = _template_payload(template_id="missing")
    res = await client.put("/api/templates/missing", json=payload)
    assert res.status_code == 404


async def test_update_template_changes_sections(client):
    await client.post("/api/templates", json=_template_payload())
    updated = _template_payload()
    updated["sections"] = [
        {"title": "New Section", "instruction": "New instruction", "format": "bullets"}
    ]
    res = await client.put("/api/templates/tpl-1", json=updated)
    assert res.status_code == 200
    assert res.json()["sections"][0]["title"] == "New Section"


# ── Additional template tests ──────────────────────────────────────


async def test_create_template_with_multiple_sections(client):
    payload = _template_payload(template_id="multi-sec")
    payload["sections"] = [
        {"title": "Summary", "instruction": "Summarize", "format": "paragraph"},
        {"title": "Action Items", "instruction": "List actions", "format": "bullets"},
        {"title": "Decisions", "instruction": "List decisions", "format": "bullets"},
    ]
    res = await client.post("/api/templates", json=payload)
    assert res.status_code == 201
    assert len(res.json()["sections"]) == 3


async def test_create_template_with_optional_fields(client):
    payload = _template_payload(template_id="opt-fields")
    payload["global_instruction"] = "Be concise and professional"
    payload["clinical_safety_rules"] = ["Do not include PHI", "Anonymize names"]
    res = await client.post("/api/templates", json=payload)
    assert res.status_code == 201
    data = res.json()
    assert data["global_instruction"] == "Be concise and professional"
    assert len(data["clinical_safety_rules"]) == 2


async def test_get_template_falls_back_to_default(client):
    """When client-specific template doesn't exist, should fall back to default."""
    await client.post("/api/templates", json=_template_payload(template_id="fallback-tpl"))
    res = await client.get("/api/templates/fallback-tpl?client_id=nonexistent-client")
    assert res.status_code == 200
    assert res.json()["client_id"] == "default"


async def test_list_templates_default_client_only(client):
    """Without client_id param, should only return default templates."""
    await client.post("/api/templates", json=_template_payload(template_id="def-only"))
    res = await client.get("/api/templates")
    assert res.status_code == 200
    for t in res.json()["templates"]:
        assert t["client_id"] == "default"


async def test_update_template_preserves_template_id(client):
    await client.post("/api/templates", json=_template_payload(template_id="preserve-id"))
    updated = _template_payload(template_id="preserve-id", name="Updated")
    res = await client.put("/api/templates/preserve-id", json=updated)
    assert res.status_code == 200
    assert res.json()["template_id"] == "preserve-id"
