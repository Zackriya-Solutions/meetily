from pydantic import BaseModel
from typing import List, Optional, Dict


class Transcript(BaseModel):
    id: str
    text: str
    timestamp: str
    # Recording-relative timestamps for audio-transcript synchronization
    audio_start_time: Optional[float] = None
    audio_end_time: Optional[float] = None
    duration: Optional[float] = None


class MeetingResponse(BaseModel):
    id: str
    title: str


class MeetingDetailsResponse(BaseModel):
    id: str
    title: str
    created_at: str
    updated_at: str
    transcripts: List[Transcript]


class MeetingTitleUpdate(BaseModel):
    meeting_id: str
    title: str


class DeleteMeetingRequest(BaseModel):
    meeting_id: str


class SaveSummaryRequest(BaseModel):
    meeting_id: str
    summary: dict


class GenerateNotesRequest(BaseModel):
    """Request model for generating detailed meeting notes."""

    meeting_id: str
    template_id: str = "standard_meeting"
    model: str = "gemini"
    model_name: str = "gemini-2.5-flash"
    custom_context: str = ""  # User-provided context for better note generation
    transcript: str = ""  # Optional explicit transcript text (to override DB)
    use_audio_context: bool = True
    audio_mode: str = "auto"  # auto | compressed | wav | transcript_only
    audio_url: str = ""  # Optional signed/public URL override for model ingestion
    max_audio_minutes: int = 120  # Safety cap for long recordings
    prefer_diarized_transcript: bool = True


class RefineNotesRequest(BaseModel):
    """Request model for refining meeting notes."""

    meeting_id: str
    current_notes: str
    user_instruction: str
    model: str = "gemini"
    model_name: str = "gemini-2.5-flash"
