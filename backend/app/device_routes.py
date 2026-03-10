"""
Device registry admin API.

Provides endpoints to list registered devices and toggle the
``advanced_logs`` flag used by the desktop app to enable debug-level
PostHog events for a specific installation.
"""

import logging
from datetime import datetime, timezone

from fastapi import APIRouter, Depends, HTTPException
from pydantic import BaseModel
from mongodb import get_mongo_client
from auth_middleware import get_current_user, get_optional_user
import os

DEPLOYMENT_MODE = os.getenv("DEPLOYMENT_MODE", "local")

# In cloud mode, require authentication; in local mode, allow unauthenticated access
_auth_dependency = get_current_user if DEPLOYMENT_MODE == "cloud" else get_optional_user

logger = logging.getLogger(__name__)

router = APIRouter(prefix="/api/devices", tags=["devices"])

_DB_NAME = os.getenv("MONGODB_DATABASE", "iqcapture")


def _devices_collection():
    client = get_mongo_client()
    return client[_DB_NAME]["devices"]


# ── Models ──────────────────────────────────────────────────────────

class ToggleAdvancedLogsRequest(BaseModel):
    user_id: str
    enabled: bool


# ── Routes ──────────────────────────────────────────────────────────

@router.get("")
async def list_devices(limit: int = 50, current_user=Depends(_auth_dependency)):
    """List registered devices sorted by most recently seen. Requires authentication in cloud mode."""
    try:
        col = _devices_collection()
        cursor = col.find({}).sort("last_seen", -1).limit(limit)
        devices = []
        async for doc in cursor:
            doc["_id"] = str(doc["_id"])
            # Convert BSON datetimes to ISO strings for JSON serialisation
            for key in ("first_seen", "last_seen"):
                if key in doc and hasattr(doc[key], "isoformat"):
                    doc[key] = doc[key].isoformat()
            devices.append(doc)
        return devices
    except Exception as e:
        logger.error(f"Failed to list devices: {e}", exc_info=True)
        raise HTTPException(status_code=500, detail=str(e))


@router.patch("/advanced-logs")
async def toggle_advanced_logs(request: ToggleAdvancedLogsRequest, current_user=Depends(_auth_dependency)):
    """Enable or disable advanced logging for a specific device. Requires authentication in cloud mode."""
    try:
        col = _devices_collection()
        result = await col.update_one(
            {"user_id": request.user_id},
            {"$set": {
                "advanced_logs": request.enabled,
                "advanced_logs_toggled_at": datetime.now(timezone.utc),
            }},
        )
        if result.matched_count == 0:
            raise HTTPException(status_code=404, detail="Device not found")
        return {
            "user_id": request.user_id,
            "advanced_logs": request.enabled,
        }
    except HTTPException:
        raise
    except Exception as e:
        logger.error(f"Failed to toggle advanced_logs: {e}", exc_info=True)
        raise HTTPException(status_code=500, detail=str(e))
