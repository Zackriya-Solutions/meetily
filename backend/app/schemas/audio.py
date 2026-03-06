from typing import Any, Dict, Optional

from pydantic import BaseModel, ConfigDict, Field


class AIGuardrailRuntimeStats(BaseModel):
    model_config = ConfigDict(extra="allow")
    analysis_attempts: int = 0
    analysis_skipped_small_window: int = 0
    analysis_skipped_interval: int = 0
    llm_calls: int = 0
    llm_failures: int = 0
    llm_timeouts: int = 0
    parse_failures: int = 0
    normalize_silent_fallbacks: int = 0
    assessment_none: int = 0
    last_analysis_at: Optional[str] = None
    window_char_count: int = 0
    window_duration_seconds: float = 0.0
    model: Optional[str] = None
    evaluator: Dict[str, Any] = Field(default_factory=dict)


class StreamingSessionRuntime(BaseModel):
    model_config = ConfigDict(extra="allow")
    reconnect_storm_detected: Optional[bool] = None
    recent_resume_events_count: Optional[int] = None
    dropped_audio_chunks: Optional[int] = None
    queue_depth: Optional[int] = None
    max_audio_queue_depth: Optional[int] = None
    backpressure_close_triggered: Optional[bool] = None
    last_warning: Optional[str] = None
    alert_counts: Optional[Dict[str, int]] = None
    ai_guardrail: Optional[AIGuardrailRuntimeStats] = None


class StreamingSessionHealthResponse(BaseModel):
    model_config = ConfigDict(extra="allow")
    session_id: str
    meeting_id: Optional[str] = None
    session_status: Optional[str] = None
    active_connections: int
    runtime: StreamingSessionRuntime = Field(default_factory=StreamingSessionRuntime)
    manager_stats: Dict[str, Any] = Field(default_factory=dict)
