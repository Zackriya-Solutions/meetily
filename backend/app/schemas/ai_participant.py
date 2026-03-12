from enum import Enum
from pydantic import BaseModel, Field
from typing import Optional, List, Dict, Any


class GuardrailReason(str, Enum):
    AGENDA_DEVIATION = "agenda_deviation"
    NO_DECISION = "no_decision"
    UNRESOLVED_QUESTION = "unresolved_question"
    MISSING_CONTEXT_OR_REPEAT = "missing_context_or_repeat"


class GuardrailLLMOutput(BaseModel):
    intervention_required: bool = False
    reason: Optional[GuardrailReason] = None
    insight: Optional[str] = None
    confidence: Optional[float] = Field(default=None, ge=0.0, le=1.0)


class GuardrailAlert(BaseModel):
    id: str
    reason: GuardrailReason
    insight: str
    confidence: float = Field(ge=0.0, le=1.0)
    timestamp: str


class HostEventType(str, Enum):
    DECISION_CANDIDATE = "decision_candidate"
    CONFLICT_RISK = "conflict_risk"
    AGENDA_DRIFT = "agenda_drift"
    URGENCY_RISK = "urgency_risk"
    MISTAKE_CANDIDATE = "mistake_candidate"
    UNHEARD_PARTICIPANT = "unheard_participant"
    OPEN_QUESTION = "open_question"


class HostRoleMode(str, Enum):
    FACILITATOR = "facilitator"
    ADVISOR = "advisor"
    CHAIRPERSON = "chairperson"


class HostSuggestion(BaseModel):
    id: str
    event_type: HostEventType
    title: str
    content: str
    confidence: float = Field(default=0.0, ge=0.0, le=1.0)
    timestamp: str
    status: str = "suggested"
    source_excerpt: Optional[str] = None
    metadata: Dict[str, Any] = Field(default_factory=dict)


class HostInterventionCard(BaseModel):
    id: str
    event_type: HostEventType
    headline: str
    body: str
    priority: str = "medium"
    confidence: float = Field(default=0.0, ge=0.0, le=1.0)
    timestamp: str
    linked_suggestion_id: Optional[str] = None


class MeetingHostState(BaseModel):
    meeting_id: str
    meeting_summary: Optional[str] = None
    agenda_progress: float = Field(default=0.0, ge=0.0, le=1.0)
    current_topic: Optional[str] = None
    unresolved_items: List[str] = Field(default_factory=list)
    suggested_items: List[HostSuggestion] = Field(default_factory=list)
    pinned_items: List[HostSuggestion] = Field(default_factory=list)
    dismissed_item_ids: List[str] = Field(default_factory=list)
    intervention_history: List[HostInterventionCard] = Field(default_factory=list)
    last_response_outcomes: List[str] = Field(default_factory=list)
    counters: Dict[str, int] = Field(default_factory=dict)
    updated_at: Optional[str] = None


class HostPolicyConfig(BaseModel):
    role_mode: HostRoleMode = HostRoleMode.FACILITATOR
    intervention_channel: str = "in_app_cards"
    min_confidence: float = Field(default=0.7, ge=0.0, le=1.0)
    suggestion_cooldown_seconds: int = 60
    intervention_cooldown_seconds: int = 120
    max_suggestions_buffer: int = 30
    max_intervention_history: int = 30
    max_pinned_items: int = 100
    allow_interruptions: bool = False
    event_threshold_overrides: Dict[HostEventType, float] = Field(default_factory=dict)
    forbidden_actions: List[str] = Field(default_factory=list)
    escalation_rules: Dict[str, str] = Field(default_factory=dict)
    source: str = "system"
