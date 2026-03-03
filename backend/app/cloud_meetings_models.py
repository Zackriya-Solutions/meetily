"""
Pydantic models for cloud meetings API (mobile + cross-device sync).
"""

from pydantic import BaseModel
from typing import Optional, List, Any
from datetime import datetime


class CreateMeetingRequest(BaseModel):
    title: str
    duration_seconds: Optional[float] = None


class UpdateMeetingRequest(BaseModel):
    title: Optional[str] = None
    status: Optional[str] = None
    duration_seconds: Optional[float] = None
    transcript_text: Optional[str] = None
    transcript_segments: Optional[List[dict]] = None
    summary: Optional[dict] = None


class CloudMeetingResponse(BaseModel):
    meeting_id: str
    user_id: str
    title: str
    created_at: str
    updated_at: str
    status: str
    duration_seconds: Optional[float] = None
    transcript_text: Optional[str] = None
    transcript_segments: Optional[List[dict]] = None
    summary: Optional[dict] = None
    audio_file_key: Optional[str] = None
    transcription_provider: Optional[str] = None
    version: int = 1


class MeetingListResponse(BaseModel):
    meetings: List[CloudMeetingResponse]
    total: int


class MeetingChange(BaseModel):
    """A single meeting change for sync."""
    meeting_id: str
    action: str  # "create" | "update" | "delete"
    data: Optional[dict] = None
    version: int = 1


class SyncRequest(BaseModel):
    last_sync_at: Optional[str] = None
    local_changes: List[MeetingChange] = []


class SyncResponse(BaseModel):
    remote_changes: List[dict]
    server_time: str
    conflicts: List[dict] = []
