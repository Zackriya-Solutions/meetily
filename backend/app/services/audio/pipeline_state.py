import logging
from typing import Dict, Optional, Set

try:
    from ...db import DatabaseManager
except (ImportError, ValueError):
    from db import DatabaseManager

logger = logging.getLogger(__name__)


class AudioPipelineStateService:
    """
    Phase 2 state manager for recording session lifecycle.
    Keeps transition logic centralized so API/worker paths stay consistent.
    """

    ALLOWED_TRANSITIONS: Dict[str, Set[str]] = {
        "recording": {"stopping_requested", "uploading_chunks", "completed", "failed"},
        "stopping_requested": {"uploading_chunks", "finalizing", "completed", "failed"},
        "uploading_chunks": {"finalizing", "completed", "failed"},
        "finalizing": {"postprocessing", "completed", "failed"},
        "postprocessing": {"completed", "failed"},
        "completed": set(),
        "failed": set(),
    }

    def __init__(self, db: Optional[DatabaseManager] = None):
        self.db = db or DatabaseManager()

    async def ensure_session(
        self, session_id: str, user_email: str, meeting_id: str, metadata: Optional[Dict] = None
    ):
        return await self.db.upsert_recording_session(
            session_id=session_id,
            user_email=user_email,
            meeting_id=meeting_id,
            status="recording",
            metadata=metadata or {},
        )

    async def mark_stop_requested(self, session_id: str) -> bool:
        return await self.transition(
            session_id=session_id,
            to_status="stopping_requested",
        )

    async def transition(
        self,
        session_id: str,
        to_status: str,
        error_code: Optional[str] = None,
        error_message: Optional[str] = None,
    ) -> bool:
        current = await self.db.get_recording_session(session_id)
        if not current:
            logger.warning(
                "[AudioPipelineState] Session not found for transition: %s -> %s",
                session_id,
                to_status,
            )
            return False

        current_status = current["status"]
        if current_status == to_status:
            return True

        allowed = self.ALLOWED_TRANSITIONS.get(current_status, set())
        if to_status not in allowed:
            logger.warning(
                "[AudioPipelineState] Invalid transition for %s: %s -> %s",
                session_id,
                current_status,
                to_status,
            )
            return False

        return await self.db.transition_recording_session_status(
            session_id=session_id,
            from_statuses=[current_status],
            to_status=to_status,
            error_code=error_code,
            error_message=error_message,
        )


_state_service: Optional[AudioPipelineStateService] = None


def get_audio_pipeline_state_service() -> AudioPipelineStateService:
    global _state_service
    if _state_service is None:
        _state_service = AudioPipelineStateService()
    return _state_service
