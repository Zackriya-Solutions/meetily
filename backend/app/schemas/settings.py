from pydantic import BaseModel
from typing import List, Optional


class SaveModelConfigRequest(BaseModel):
    provider: str
    model: str
    whisperModel: str
    apiKey: Optional[str] = None


class SaveTranscriptConfigRequest(BaseModel):
    provider: str
    model: str
    apiKey: Optional[str] = None


class GetApiKeyRequest(BaseModel):
    provider: str


class UserApiKeySaveRequest(BaseModel):
    provider: str
    api_key: str


class CalendarConnectRequest(BaseModel):
    request_write_scope: bool = False


class CalendarDisconnectRequest(BaseModel):
    provider: str = "google"


class CalendarAutomationSettingsRequest(BaseModel):
    reminders_enabled: bool
    attendee_reminders_enabled: bool
    reminder_offset_minutes: int
    recap_enabled: bool
    writeback_enabled: bool
    share_summary: bool = True
    share_transcript: bool = False


class CalendarReminderEmailRequest(BaseModel):
    meeting_title: str
    meeting_start_iso: Optional[str] = None
    meeting_link: Optional[str] = None
    start_meeting_url: Optional[str] = None
    attendees: Optional[List[str]] = None
    include_attendees: Optional[bool] = None

from datetime import datetime
from typing import Optional, List

class SyncOAuthRequest(BaseModel):
    access_token: str
    refresh_token: Optional[str] = None
    token_expires_at: Optional[str] = None
    scopes: List[str]
    external_account_email: str
