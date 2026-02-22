from pydantic import BaseModel
from typing import List, Optional, Dict, Union


class ChatRequest(BaseModel):
    meeting_id: str
    question: str
    model: str
    model_name: str
    context_text: Optional[str] = None
    allowed_meeting_ids: Optional[List[str]] = None  # Scoped search
    history: Optional[List[Dict[str, str]]] = None  # Conversation history


class CatchUpRequest(BaseModel):
    """Request model for catch-up summary"""

    transcripts: List[Union[str, Dict[str, object]]]  # Supports plain text or rich transcript entries
    model: str = "gemini"
    model_name: str = "gemini-2.5-flash"
    window_minutes: Optional[int] = None
    window_start_iso: Optional[str] = None
    window_end_iso: Optional[str] = None
    meeting_elapsed_seconds: Optional[int] = None


class SearchContextRequest(BaseModel):
    """Request model for cross-meeting context search"""

    query: str
    n_results: int = 5
    allowed_meeting_ids: Optional[List[str]] = None  # None = search all meetings
