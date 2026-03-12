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


class UserAIHostSkillRequest(BaseModel):
    skill_markdown: str
    is_active: bool = True


class UserAIHostSkillResponse(BaseModel):
    user_email: str
    skill_markdown: str
    is_active: bool = True
    source: str = "user"


class AIHostStyleItem(BaseModel):
    id: str
    name: str
    source: str  # system | user
    read_only: bool = False
    is_default: bool = False
    is_active: bool = True
    skill_markdown: str


class AIHostStylesListResponse(BaseModel):
    styles: List[AIHostStyleItem]
    default_style_id: str


class UserAIHostStyleCreateRequest(BaseModel):
    name: str
    skill_markdown: str
    is_active: bool = True
    set_default: bool = False


class UserAIHostStyleUpdateRequest(BaseModel):
    name: Optional[str] = None
    skill_markdown: Optional[str] = None
    is_active: Optional[bool] = None


class UserAIHostStyleDefaultRequest(BaseModel):
    style_id: str


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
