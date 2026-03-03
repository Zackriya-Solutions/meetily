"""
Cloud meetings CRUD — MongoDB-backed meeting storage for mobile devices.
Meetings are stored locally on mobile (SQLite) and synced here.
"""

import logging
import uuid
from datetime import datetime, timezone
from typing import Optional

from fastapi import APIRouter, HTTPException, Depends, Query
from auth_middleware import get_current_user
from cloud_meetings_models import (
    CreateMeetingRequest,
    UpdateMeetingRequest,
    CloudMeetingResponse,
    MeetingListResponse,
    SyncRequest,
    SyncResponse,
)
from mongodb import get_meetings_collection

logger = logging.getLogger(__name__)

router = APIRouter(prefix="/api/meetings", tags=["meetings-cloud"])


def _meeting_doc_to_response(doc: dict) -> dict:
    """Convert MongoDB document to API response dict."""
    return {
        "meeting_id": doc["meeting_id"],
        "user_id": doc["user_id"],
        "title": doc.get("title", ""),
        "created_at": doc.get("created_at", ""),
        "updated_at": doc.get("updated_at", ""),
        "status": doc.get("status", "completed"),
        "duration_seconds": doc.get("duration_seconds"),
        "transcript_text": doc.get("transcript_text"),
        "transcript_segments": doc.get("transcript_segments"),
        "summary": doc.get("summary"),
        "audio_file_key": doc.get("audio_file_key"),
        "transcription_provider": doc.get("transcription_provider"),
        "version": doc.get("version", 1),
    }


@router.post("", response_model=CloudMeetingResponse)
async def create_meeting(
    req: CreateMeetingRequest,
    current_user: dict = Depends(get_current_user),
):
    """Create a new meeting in the cloud."""
    now = datetime.now(timezone.utc).isoformat()
    meeting_id = str(uuid.uuid4())

    doc = {
        "meeting_id": meeting_id,
        "user_id": current_user["user_id"],
        "title": req.title,
        "created_at": now,
        "updated_at": now,
        "status": "recording",
        "duration_seconds": req.duration_seconds,
        "transcript_text": None,
        "transcript_segments": None,
        "summary": None,
        "audio_file_key": None,
        "transcription_provider": None,
        "deleted_at": None,
        "version": 1,
    }

    col = get_meetings_collection()
    await col.insert_one(doc)
    logger.info(f"Created cloud meeting {meeting_id} for user {current_user['user_id']}")

    return _meeting_doc_to_response(doc)


@router.get("", response_model=MeetingListResponse)
async def list_meetings(
    skip: int = Query(0, ge=0),
    limit: int = Query(50, ge=1, le=200),
    current_user: dict = Depends(get_current_user),
):
    """List all meetings for the authenticated user."""
    col = get_meetings_collection()
    query = {"user_id": current_user["user_id"], "deleted_at": None}

    total = await col.count_documents(query)
    cursor = col.find(query).sort("created_at", -1).skip(skip).limit(limit)
    docs = await cursor.to_list(length=limit)

    return {
        "meetings": [_meeting_doc_to_response(d) for d in docs],
        "total": total,
    }


@router.get("/{meeting_id}", response_model=CloudMeetingResponse)
async def get_meeting(
    meeting_id: str,
    current_user: dict = Depends(get_current_user),
):
    """Get a specific meeting by ID."""
    col = get_meetings_collection()
    doc = await col.find_one({
        "meeting_id": meeting_id,
        "user_id": current_user["user_id"],
        "deleted_at": None,
    })
    if not doc:
        raise HTTPException(status_code=404, detail="Meeting not found")
    return _meeting_doc_to_response(doc)


@router.put("/{meeting_id}", response_model=CloudMeetingResponse)
async def update_meeting(
    meeting_id: str,
    req: UpdateMeetingRequest,
    current_user: dict = Depends(get_current_user),
):
    """Update a meeting (title, transcript, summary, status)."""
    col = get_meetings_collection()
    now = datetime.now(timezone.utc).isoformat()

    update_fields: dict = {"updated_at": now}
    if req.title is not None:
        update_fields["title"] = req.title
    if req.status is not None:
        update_fields["status"] = req.status
    if req.duration_seconds is not None:
        update_fields["duration_seconds"] = req.duration_seconds
    if req.transcript_text is not None:
        update_fields["transcript_text"] = req.transcript_text
    if req.transcript_segments is not None:
        update_fields["transcript_segments"] = req.transcript_segments
    if req.summary is not None:
        update_fields["summary"] = req.summary

    result = await col.find_one_and_update(
        {
            "meeting_id": meeting_id,
            "user_id": current_user["user_id"],
            "deleted_at": None,
        },
        {"$set": update_fields, "$inc": {"version": 1}},
        return_document=True,
    )

    if not result:
        raise HTTPException(status_code=404, detail="Meeting not found")
    return _meeting_doc_to_response(result)


@router.delete("/{meeting_id}")
async def delete_meeting(
    meeting_id: str,
    current_user: dict = Depends(get_current_user),
):
    """Soft-delete a meeting."""
    col = get_meetings_collection()
    now = datetime.now(timezone.utc).isoformat()

    result = await col.update_one(
        {
            "meeting_id": meeting_id,
            "user_id": current_user["user_id"],
            "deleted_at": None,
        },
        {"$set": {"deleted_at": now, "updated_at": now}, "$inc": {"version": 1}},
    )

    if result.modified_count == 0:
        raise HTTPException(status_code=404, detail="Meeting not found")
    return {"message": "Meeting deleted"}


@router.post("/sync", response_model=SyncResponse)
async def sync_meetings(
    req: SyncRequest,
    current_user: dict = Depends(get_current_user),
):
    """
    Sync endpoint: push local changes, pull remote changes.
    Uses last-write-wins for conflicts. Server is authoritative
    for transcripts and summaries.
    """
    col = get_meetings_collection()
    user_id = current_user["user_id"]
    now = datetime.now(timezone.utc).isoformat()
    conflicts = []

    # 1. Apply local changes from mobile
    for change in req.local_changes:
        if change.action == "create" and change.data:
            # Create meeting if it doesn't exist
            existing = await col.find_one({"meeting_id": change.meeting_id})
            if not existing:
                doc = {
                    "meeting_id": change.meeting_id,
                    "user_id": user_id,
                    "title": change.data.get("title", ""),
                    "created_at": change.data.get("created_at", now),
                    "updated_at": now,
                    "status": change.data.get("status", "pending_upload"),
                    "duration_seconds": change.data.get("duration_seconds"),
                    "transcript_text": None,
                    "transcript_segments": None,
                    "summary": None,
                    "audio_file_key": None,
                    "transcription_provider": None,
                    "deleted_at": None,
                    "version": 1,
                }
                await col.insert_one(doc)

        elif change.action == "update" and change.data:
            # Only update non-server-authoritative fields (title, status)
            safe_fields = {}
            for key in ["title", "duration_seconds"]:
                if key in change.data:
                    safe_fields[key] = change.data[key]
            if safe_fields:
                safe_fields["updated_at"] = now
                existing = await col.find_one({
                    "meeting_id": change.meeting_id,
                    "user_id": user_id,
                })
                if existing and existing.get("version", 1) > change.version:
                    conflicts.append({
                        "meeting_id": change.meeting_id,
                        "reason": "server_version_newer",
                        "server_version": existing["version"],
                        "client_version": change.version,
                    })
                else:
                    await col.update_one(
                        {"meeting_id": change.meeting_id, "user_id": user_id},
                        {"$set": safe_fields, "$inc": {"version": 1}},
                    )

        elif change.action == "delete":
            await col.update_one(
                {"meeting_id": change.meeting_id, "user_id": user_id},
                {"$set": {"deleted_at": now, "updated_at": now}},
            )

    # 2. Return remote changes since last sync
    remote_query: dict = {"user_id": user_id}
    if req.last_sync_at:
        remote_query["updated_at"] = {"$gt": req.last_sync_at}

    cursor = col.find(remote_query).sort("updated_at", -1).limit(500)
    remote_docs = await cursor.to_list(length=500)

    remote_changes = [_meeting_doc_to_response(d) for d in remote_docs]

    return {
        "remote_changes": remote_changes,
        "server_time": now,
        "conflicts": conflicts,
    }
