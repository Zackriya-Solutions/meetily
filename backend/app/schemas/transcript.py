from pydantic import BaseModel
from typing import List, Optional
from .meeting import Transcript


class SaveTranscriptRequest(BaseModel):
    meeting_title: str
    transcripts: List[Transcript]
    folder_path: Optional[str] = None
    template_id: Optional[str] = "standard_meeting"
    session_id: Optional[str] = None  # NEW: For linking audio recording
    meeting_id: Optional[str] = None  # Preferred canonical meeting id


class SaveModelConfigRequest(BaseModel):
    provider: str
    model: str
    whisperModel: str
    apiKey: Optional[str] = None


class SaveTranscriptConfigRequest(BaseModel):
    provider: str
    model: str
    apiKey: Optional[str] = None


class TranscriptRequest(BaseModel):
    """Request model for transcript text, updated with meeting_id"""

    text: str
    model: str = "gemini"
    model_name: str = "gemini-2.5-flash"
    meeting_id: str
    chunk_size: Optional[int] = 5000
    overlap: Optional[int] = 1000
    custom_prompt: Optional[str] = "Generate a summary of the meeting transcript."
    templateId: Optional[str] = "standard_meeting"  # Template for note generation


class DiarizeRequest(BaseModel):
    """Request model for triggering speaker diarization."""

    provider: str = "deepgram"  # 'deepgram' or 'assemblyai'


class RenameSpeakerRequest(BaseModel):
    """Request model for renaming a speaker label."""

    display_name: str


class DiarizationStatusResponse(BaseModel):
    """Response model for diarization status."""

    meeting_id: str
    status: str  # 'pending', 'processing', 'completed', 'failed', 'not_recorded'
    speaker_count: Optional[int] = None
    provider: Optional[str] = None
    error: Optional[str] = None
    completed_at: Optional[str] = None


class SpeakerMappingItem(BaseModel):
    """Represents a single speaker mapping entry."""

    label: str
    display_name: str
    color: Optional[str] = None


class SpeakerMappingResponse(BaseModel):
    """Response model for speaker label mappings."""

    meeting_id: str
    speakers: List[SpeakerMappingItem]
