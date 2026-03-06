from enum import Enum
from pydantic import BaseModel, Field
from typing import Optional


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
