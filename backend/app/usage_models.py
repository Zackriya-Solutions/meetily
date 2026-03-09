"""
Pydantic models for usage tracking endpoints.
"""

from pydantic import BaseModel, field_validator
from typing import Optional, List
from enum import Enum


class EventType(str, Enum):
    RECORDING_MINUTES = "recording_minutes"
    TRANSCRIPTION_MINUTES = "transcription_minutes"
    MEETING_CREATED = "meeting_created"
    SUMMARY_GENERATED = "summary_generated"
    ACTIVE_MINUTES = "active_minutes"
    SESSION_STARTED = "session_started"
    SESSION_ENDED = "session_ended"


class UsageEvent(BaseModel):
    event_type: EventType
    value: float
    metadata: Optional[dict] = None
    session_id: Optional[str] = None
    client_event_id: Optional[str] = None  # For deduplication on retry
    timestamp: str  # ISO format, client-generated


class BatchUsageEventsRequest(BaseModel):
    device_id: str
    events: List[UsageEvent]

    @field_validator("events")
    @classmethod
    def limit_batch_size(cls, v: list) -> list:
        if len(v) > 100:
            raise ValueError("Maximum 100 events per batch")
        return v


class ModelUsageEntry(BaseModel):
    provider: str
    model: str
    count: int


class UsageSummaryResponse(BaseModel):
    period: str  # "daily" | "monthly" | "all_time"
    period_start: Optional[str] = None
    recording_minutes: float = 0.0
    transcription_minutes: float = 0.0
    meetings_count: int = 0
    summaries_count: int = 0
    active_minutes: float = 0.0
    sessions_count: int = 0
    whisper_models_used: List[str] = []
    llm_models_used: List[ModelUsageEntry] = []
    device_count: int = 0
