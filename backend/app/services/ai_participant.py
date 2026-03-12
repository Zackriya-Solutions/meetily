import asyncio
import json
import logging
import os
import re
import time
import uuid
from collections import deque
from dataclasses import dataclass
from datetime import datetime
from typing import Any, Deque, Dict, List, Optional, Tuple

from anthropic import AsyncAnthropic
from openai import AsyncOpenAI

try:
    from ..schemas.ai_participant import (
        GuardrailAlert,
        GuardrailLLMOutput,
        GuardrailReason,
        HostInterventionCard,
        HostPolicyConfig,
        HostRoleMode,
        HostSuggestion,
        MeetingHostState,
    )
    from .gemini_client import generate_content_text_async
    from .ai_participant_skills import (
        load_system_skill_templates,
        parse_skill_markdown,
    )
except (ImportError, ValueError):
    from schemas.ai_participant import (
        GuardrailAlert,
        GuardrailLLMOutput,
        GuardrailReason,
        HostInterventionCard,
        HostPolicyConfig,
        HostRoleMode,
        HostSuggestion,
        MeetingHostState,
    )
    from services.gemini_client import generate_content_text_async
    from services.ai_participant_skills import (
        load_system_skill_templates,
        parse_skill_markdown,
    )

logger = logging.getLogger(__name__)

SYSTEM_HOST_SKILLS: Dict[str, str] = load_system_skill_templates()
CORE_EVENT_TYPES = {"decision_candidate", "open_discussion"}
DEFAULT_PROVIDER_MODELS = {
    "gemini": "gemini-2.5-flash",
    "openai": "gpt-4o-mini",
    "anthropic": "claude-3-5-sonnet-20241022",
}


@dataclass
class MeetingContext:
    meeting_id: str
    title: str = ""
    goal: str = ""
    description: str = ""
    agenda_text: str = ""
    participant_names: Optional[List[str]] = None


class RollingTranscriptBuffer:
    def __init__(self, window_seconds: int = 180, max_chars: int = 6000):
        self.window_seconds = window_seconds
        self.max_chars = max_chars
        self._items: Deque[Tuple[float, str]] = deque()
        self._char_count = 0

    def add(self, timestamp_seconds: float, text: str) -> None:
        clean_text = (text or "").strip()
        if not clean_text:
            return

        ts = float(timestamp_seconds)
        self._items.append((ts, clean_text))
        self._char_count += len(clean_text)
        self._prune(ts)

    def _prune(self, current_ts: float) -> None:
        window_start = current_ts - float(self.window_seconds)
        while self._items and self._items[0][0] < window_start:
            _, old_text = self._items.popleft()
            self._char_count -= len(old_text)

        while self._items and self._char_count > self.max_chars:
            _, old_text = self._items.popleft()
            self._char_count -= len(old_text)

    def is_empty(self) -> bool:
        return not self._items

    def get_duration_seconds(self) -> float:
        if len(self._items) < 2:
            return 0.0
        return max(0.0, self._items[-1][0] - self._items[0][0])

    def get_text(self) -> str:
        return "\n".join(item[1] for item in self._items)

    def get_char_count(self) -> int:
        return max(0, self._char_count)


class GuardrailEvaluator:
    def __init__(self):
        self.min_confidence = float(os.getenv("AI_PARTICIPANT_MIN_CONFIDENCE", "0.70"))
        self.cooldown_seconds = int(os.getenv("AI_PARTICIPANT_COOLDOWN_SECONDS", "180"))
        self.decision_logs = (
            os.getenv("AI_PARTICIPANT_DECISION_LOGS", "true").strip().lower() == "true"
        )
        self.agenda_sustained_cycles = int(
            os.getenv("AI_PARTICIPANT_AGENDA_SUSTAINED_CYCLES", "1")
        )
        self.no_decision_threshold_seconds = int(
            os.getenv("AI_PARTICIPANT_NO_DECISION_SECONDS", "360")
        )
        self.unresolved_question_threshold_seconds = int(
            os.getenv("AI_PARTICIPANT_UNRESOLVED_QUESTION_SECONDS", "240")
        )

        self._agenda_deviation_streak = 0
        self._last_alert_signature = ""
        self._last_publish_at = 0.0
        self._metrics: Dict[str, Any] = {
            "evaluations": 0,
            "published": 0,
            "published_by_reason": {},
            "suppressed_no_intervention": 0,
            "suppressed_missing_fields": 0,
            "suppressed_low_confidence": 0,
            "suppressed_agenda_not_sustained": 0,
            "suppressed_no_decision_duration": 0,
            "suppressed_unresolved_question_duration": 0,
            "suppressed_cooldown": 0,
            "suppressed_duplicate": 0,
        }

    def evaluate(
        self,
        assessment: GuardrailLLMOutput,
        window_duration_seconds: float,
        now_ts: float,
    ) -> Optional[GuardrailAlert]:
        reason_value = assessment.reason.value if assessment.reason else None
        confidence = float(assessment.confidence or 0.0)

        def log_decision(decision: str, detail: str) -> None:
            if not self.decision_logs:
                return
            logger.info(
                "[AIParticipant][Decision] %s reason=%s confidence=%.2f window_duration=%.1fs detail=%s",
                decision,
                reason_value,
                confidence,
                window_duration_seconds,
                detail,
            )

        self._metrics["evaluations"] += 1
        if not assessment.intervention_required:
            self._agenda_deviation_streak = 0
            self._metrics["suppressed_no_intervention"] += 1
            log_decision("suppressed_no_intervention", "intervention_required=false")
            return None

        if not assessment.reason or not assessment.insight:
            self._metrics["suppressed_missing_fields"] += 1
            log_decision(
                "suppressed_missing_fields", "missing reason or insight in model output"
            )
            return None

        if confidence < self.min_confidence:
            self._metrics["suppressed_low_confidence"] += 1
            log_decision(
                "suppressed_low_confidence",
                f"confidence={confidence:.2f} < min_confidence={self.min_confidence:.2f}",
            )
            return None

        if assessment.reason == GuardrailReason.AGENDA_DEVIATION:
            self._agenda_deviation_streak += 1
            if self._agenda_deviation_streak < self.agenda_sustained_cycles:
                self._metrics["suppressed_agenda_not_sustained"] += 1
                log_decision(
                    "suppressed_agenda_not_sustained",
                    f"streak={self._agenda_deviation_streak} < required={self.agenda_sustained_cycles}",
                )
                return None
        else:
            self._agenda_deviation_streak = 0

        if (
            assessment.reason == GuardrailReason.NO_DECISION
            and window_duration_seconds < self.no_decision_threshold_seconds
        ):
            self._metrics["suppressed_no_decision_duration"] += 1
            log_decision(
                "suppressed_no_decision_duration",
                f"window={window_duration_seconds:.1f}s < threshold={self.no_decision_threshold_seconds}s",
            )
            return None

        if (
            assessment.reason == GuardrailReason.UNRESOLVED_QUESTION
            and window_duration_seconds < self.unresolved_question_threshold_seconds
        ):
            self._metrics["suppressed_unresolved_question_duration"] += 1
            log_decision(
                "suppressed_unresolved_question_duration",
                f"window={window_duration_seconds:.1f}s < threshold={self.unresolved_question_threshold_seconds}s",
            )
            return None

        insight = self._normalize_insight(assessment.insight)
        signature = self._signature(assessment.reason.value, insight)

        if now_ts - self._last_publish_at < self.cooldown_seconds:
            self._metrics["suppressed_cooldown"] += 1
            log_decision(
                "suppressed_cooldown",
                f"since_last={now_ts - self._last_publish_at:.1f}s < cooldown={self.cooldown_seconds}s",
            )
            return None

        if signature == self._last_alert_signature:
            self._metrics["suppressed_duplicate"] += 1
            log_decision(
                "suppressed_duplicate",
                "same reason+insight signature as previous alert",
            )
            return None

        self._last_alert_signature = signature
        self._last_publish_at = now_ts
        self._metrics["published"] += 1
        by_reason = self._metrics.setdefault("published_by_reason", {})
        by_reason[assessment.reason.value] = int(by_reason.get(assessment.reason.value) or 0) + 1

        return GuardrailAlert(
            id=str(uuid.uuid4()),
            reason=assessment.reason,
            insight=insight,
            confidence=round(confidence, 2),
            timestamp=datetime.utcnow().isoformat(),
        )

    @staticmethod
    def _signature(reason: str, insight: str) -> str:
        normalized = " ".join((insight or "").strip().lower().split())
        return f"{reason}:{normalized}"

    @staticmethod
    def _normalize_insight(insight: str) -> str:
        text = " ".join((insight or "").strip().split())
        words = text.split(" ")
        if len(words) <= 30:
            return text
        return " ".join(words[:30]).rstrip(" ,.;") + "."

    def get_metrics_snapshot(self) -> Dict[str, Any]:
        payload = dict(self._metrics)
        payload["published_by_reason"] = dict(self._metrics.get("published_by_reason") or {})
        return payload


class AIParticipantEngine:
    def __init__(
        self,
        db,
        user_email: str,
        meeting_context: MeetingContext,
    ):
        self.db = db
        self.user_email = user_email
        self.meeting_context = meeting_context

        self.enabled = os.getenv("AI_PARTICIPANT_ENABLED", "true").lower() == "true"
        self.provider = (
            os.getenv("AI_PARTICIPANT_PROVIDER", "gemini").strip().lower() or "gemini"
        )
        if self.provider == "claude":
            self.provider = "anthropic"
        self.model_name = os.getenv(
            "AI_PARTICIPANT_MODEL",
            DEFAULT_PROVIDER_MODELS.get(self.provider, DEFAULT_PROVIDER_MODELS["gemini"]),
        )
        fallback_models = os.getenv(
            "AI_PARTICIPANT_FALLBACK_MODELS", ""
        )
        self.fallback_models = [
            m.strip() for m in fallback_models.split(",") if (m or "").strip()
        ]
        self.llm_timeout_seconds = float(
            os.getenv("AI_PARTICIPANT_LLM_TIMEOUT_SECONDS", "12")
        )
        self.analysis_interval_seconds = int(
            os.getenv("AI_PARTICIPANT_ANALYSIS_INTERVAL_SECONDS", "90")
        )
        self.verbose_logs = (
            os.getenv("AI_PARTICIPANT_VERBOSE_LOGS", "false").strip().lower() == "true"
        )
        self.min_chars_before_analysis = int(
            os.getenv("AI_PARTICIPANT_MIN_WINDOW_CHARS", "0")
        )

        window_seconds = int(os.getenv("AI_PARTICIPANT_WINDOW_SECONDS", "180"))
        max_chars = int(os.getenv("AI_PARTICIPANT_MAX_WINDOW_CHARS", "6000"))

        self.buffer = RollingTranscriptBuffer(
            window_seconds=window_seconds,
            max_chars=max_chars,
        )
        self.evaluator = GuardrailEvaluator()

        self._last_analysis_at = 0.0
        self._lock = asyncio.Lock()
        self._provider_api_key: Optional[str] = None
        self._missing_key_logged = False
        self._last_alert_summary = "None"

        self._host_event_last_published_at: Dict[str, float] = {}
        self._host_event_last_signature: Dict[str, str] = {}
        self._host_state = MeetingHostState(meeting_id=self.meeting_context.meeting_id)
        default_skill_markdown = (
            os.getenv("AI_HOST_DEFAULT_SKILL_MARKDOWN", "").strip()
            or SYSTEM_HOST_SKILLS.get("facilitator", "")
        )
        self._active_skill_markdown = default_skill_markdown
        self._active_skill_definition = parse_skill_markdown(default_skill_markdown)
        self._host_policy = self._load_policy_from_skill(
            skill_text=default_skill_markdown,
            source="system",
        )
        self._host_policy_source = "system"

        self._stats: Dict[str, Any] = {
            "analysis_attempts": 0,
            "analysis_skipped_small_window": 0,
            "analysis_skipped_interval": 0,
            "llm_calls": 0,
            "llm_failures": 0,
            "llm_timeouts": 0,
            "parse_failures": 0,
            "normalize_silent_fallbacks": 0,
            "assessment_none": 0,
            "model_fallbacks": 0,
            "last_model_used": self.model_name,
            "provider": self.provider,
            "last_assessment_intervention_required": None,
            "last_assessment_reason": None,
            "last_assessment_confidence": None,
            "last_analysis_at": None,
            "host_suggestions_emitted": 0,
            "host_interventions_emitted": 0,
            "host_suggestions_pinned": 0,
            "host_suggestions_dismissed": 0,
            "host_suggestions_suppressed": 0,
            "host_policy_source": self._host_policy_source,
        }

    async def ingest_transcript(
        self,
        text: str,
        transcript_time_seconds: Optional[float] = None,
    ) -> Optional[GuardrailAlert]:
        """Backward-compatible guardrail path."""
        if not self.enabled:
            return None

        now_ts = time.time()
        ts = (
            float(transcript_time_seconds)
            if transcript_time_seconds is not None
            else now_ts
        )
        self.buffer.add(ts, text)

        if (
            self.min_chars_before_analysis > 0
            and self.buffer.get_char_count() < self.min_chars_before_analysis
        ):
            self._stats["analysis_skipped_small_window"] += 1
            return None

        if now_ts - self._last_analysis_at < self.analysis_interval_seconds:
            self._stats["analysis_skipped_interval"] += 1
            return None

        async with self._lock:
            now_ts = time.time()
            if now_ts - self._last_analysis_at < self.analysis_interval_seconds:
                self._stats["analysis_skipped_interval"] += 1
                return None

            self._last_analysis_at = now_ts
            self._stats["analysis_attempts"] += 1
            self._stats["last_analysis_at"] = datetime.utcnow().isoformat()
            assessment = await self._reason_with_llm()
            if not assessment:
                self._stats["assessment_none"] += 1
                return None

            self._stats["last_assessment_intervention_required"] = bool(
                assessment.intervention_required
            )
            self._stats["last_assessment_reason"] = (
                assessment.reason.value if assessment.reason else None
            )
            self._stats["last_assessment_confidence"] = float(
                assessment.confidence or 0.0
            )

            alert = self.evaluator.evaluate(
                assessment=assessment,
                window_duration_seconds=self.buffer.get_duration_seconds(),
                now_ts=now_ts,
            )
            if alert:
                self._last_alert_summary = f"{alert.reason.value}: {alert.insight}"
            return alert

    async def ingest_transcript_host(
        self,
        text: str,
        transcript_time_seconds: Optional[float] = None,
    ) -> Dict[str, Any]:
        """Active host path: suggestions + interventions + state delta."""
        payload: Dict[str, Any] = {
            "suggestions": [],
            "interventions": [],
            "state_delta": {},
            "policy_source": self._host_policy_source,
        }
        if not self.enabled:
            return payload

        now_ts = time.time()
        ts = (
            float(transcript_time_seconds)
            if transcript_time_seconds is not None
            else now_ts
        )
        self.buffer.add(ts, text)

        if (
            self.min_chars_before_analysis > 0
            and self.buffer.get_char_count() < self.min_chars_before_analysis
        ):
            self._stats["analysis_skipped_small_window"] += 1
            return payload

        if now_ts - self._last_analysis_at < self.analysis_interval_seconds:
            self._stats["analysis_skipped_interval"] += 1
            return payload

        async with self._lock:
            now_ts = time.time()
            if now_ts - self._last_analysis_at < self.analysis_interval_seconds:
                self._stats["analysis_skipped_interval"] += 1
                return payload

            self._last_analysis_at = now_ts
            self._stats["analysis_attempts"] += 1
            self._stats["last_analysis_at"] = datetime.utcnow().isoformat()

            events = await self._reason_host_events()
            if not events:
                self._stats["assessment_none"] += 1
                payload["state_delta"] = self.get_host_state_snapshot()
                return payload

            for event in events:
                suggestion = self._build_host_suggestion(event)
                if not suggestion:
                    self._stats["host_suggestions_suppressed"] += 1
                    continue

                self._host_state.suggested_items.insert(0, suggestion)
                self._host_state.suggested_items = self._host_state.suggested_items[
                    : self._host_policy.max_suggestions_buffer
                ]
                self._host_state.counters["suggested"] = (
                    int(self._host_state.counters.get("suggested") or 0) + 1
                )
                self._host_state.updated_at = datetime.utcnow().isoformat()
                self._stats["host_suggestions_emitted"] += 1
                payload["suggestions"].append(suggestion.model_dump())

                card = self._build_intervention_from_suggestion(suggestion, now_ts)
                if card is not None:
                    self._host_state.intervention_history.insert(0, card)
                    self._host_state.intervention_history = self._host_state.intervention_history[
                        : self._host_policy.max_intervention_history
                    ]
                    self._host_state.counters["intervened"] = (
                        int(self._host_state.counters.get("intervened") or 0) + 1
                    )
                    self._host_state.updated_at = datetime.utcnow().isoformat()
                    self._stats["host_interventions_emitted"] += 1
                    payload["interventions"].append(card.model_dump())

            payload["state_delta"] = self.get_host_state_snapshot()
            return payload

    async def _reason_with_llm(self) -> Optional[GuardrailLLMOutput]:
        api_key = await self._get_gemini_api_key()
        if not api_key:
            return None

        transcript_window = self.buffer.get_text()
        if not transcript_window:
            return None

        prompt = self._build_prompt(transcript_window)
        raw_text, used_model = await self._call_llm_json(prompt)
        if raw_text is None:
            return None

        try:
            self._stats["last_model_used"] = used_model
            parsed = self._extract_json(raw_text)
            if not parsed:
                self._stats["parse_failures"] += 1
                return None
            normalized = self._normalize_model_payload(parsed)
            if not normalized:
                self._stats["normalize_silent_fallbacks"] += 1
                return None
            return GuardrailLLMOutput.model_validate(normalized)
        except Exception:
            self._stats["llm_failures"] += 1
            return None

    async def _reason_host_events(self) -> List[Dict[str, Any]]:
        api_key = await self._get_gemini_api_key()
        if not api_key:
            return []

        transcript_window = self.buffer.get_text()
        if not transcript_window:
            return []

        prompt = self._build_host_prompt(transcript_window)
        raw_text, used_model = await self._call_llm_json(prompt)
        if raw_text is None:
            return []

        self._stats["last_model_used"] = used_model
        parsed = self._extract_json(raw_text)
        if not parsed:
            self._stats["parse_failures"] += 1
            return []

        # Update the rolling meeting summary from the model directly into host state
        summary_text = str(parsed.get("meeting_summary") or "").strip()
        if summary_text and summary_text.lower() not in ("null", "none"):
            self._host_state.meeting_summary = summary_text

        events = parsed.get("events") if isinstance(parsed, dict) else None
        if not isinstance(events, list):
            return []

        normalized_events: List[Dict[str, Any]] = []
        for event in events:
            if not isinstance(event, dict):
                continue
            event_type = self._normalize_host_event_type(event.get("event_type"))
            if not event_type:
                continue

            title = " ".join(str(event.get("title") or "").split()).strip()
            content = " ".join(str(event.get("content") or "").split()).strip()
            if not content:
                continue

            confidence = event.get("confidence", 0.0)
            try:
                confidence = float(confidence)
            except Exception:
                confidence = 0.0
            confidence = max(0.0, min(1.0, confidence))

            priority = str(event.get("priority") or "medium").strip().lower()
            if priority not in {"low", "medium", "high"}:
                priority = "medium"

            source_excerpt = " ".join(
                str(event.get("source_excerpt") or "").split()
            ).strip()

            normalized_events.append(
                {
                    "event_type": event_type,
                    "title": title or event_type.replace("_", " ").title(),
                    "content": content,
                    "confidence": confidence,
                    "priority": priority,
                    "source_excerpt": source_excerpt[:240] if source_excerpt else None,
                }
            )
        self._refresh_host_state_from_events(normalized_events)
        return normalized_events

    async def _call_llm_json(self, prompt: str) -> Tuple[Optional[str], str]:
        model_candidates: List[str] = []
        for model in [self.model_name, *self.fallback_models]:
            if model and model not in model_candidates:
                model_candidates.append(model)

        used_model = self.model_name
        for idx, model in enumerate(model_candidates):
            try:
                self._stats["llm_calls"] += 1
                used_model = model
                raw_text = await asyncio.wait_for(
                    self._generate_llm_text(model=model, prompt=prompt),
                    timeout=self.llm_timeout_seconds,
                )
                if idx > 0:
                    self._stats["model_fallbacks"] += 1
                return raw_text, used_model
            except (asyncio.TimeoutError, TimeoutError):
                self._stats["llm_failures"] += 1
                self._stats["llm_timeouts"] += 1
                if idx == len(model_candidates) - 1:
                    return None, used_model
            except Exception:
                self._stats["llm_failures"] += 1
                if idx == len(model_candidates) - 1:
                    return None, used_model

        return None, used_model

    async def _generate_llm_text(self, model: str, prompt: str) -> str:
        api_key = await self._get_provider_api_key()
        if not api_key:
            raise ValueError(f"{self.provider} API key not found")

        if self.provider == "gemini":
            return await generate_content_text_async(
                api_key=api_key,
                model=model,
                contents=prompt,
                config={"temperature": 0.1},
            )

        if self.provider == "openai":
            client = AsyncOpenAI(api_key=api_key)
            response = await client.chat.completions.create(
                model=model,
                temperature=0.1,
                response_format={"type": "json_object"},
                messages=[
                    {"role": "system", "content": "Return strict JSON only."},
                    {"role": "user", "content": prompt},
                ],
            )
            return response.choices[0].message.content or ""

        if self.provider == "anthropic":
            client = AsyncAnthropic(api_key=api_key)
            response = await client.messages.create(
                model=model,
                max_tokens=1200,
                temperature=0.1,
                system="Return strict JSON only.",
                messages=[{"role": "user", "content": prompt}],
            )
            parts: List[str] = []
            for block in getattr(response, "content", []) or []:
                text = getattr(block, "text", None)
                if text:
                    parts.append(text)
            return "".join(parts)

        raise ValueError(f"Unsupported AI participant provider: {self.provider}")

    async def _get_provider_api_key(self) -> Optional[str]:
        if self._provider_api_key:
            return self._provider_api_key

        key = ""
        if self.provider == "gemini":
            key = os.getenv("GEMINI_API_KEY", "").strip() or os.getenv("GOOGLE_API_KEY", "").strip()
            if not key:
                key = (await self.db.get_api_key("gemini", user_email=self.user_email)) or ""
        elif self.provider == "openai":
            key = os.getenv("OPENAI_API_KEY", "").strip()
            if not key:
                key = (await self.db.get_api_key("openai", user_email=self.user_email)) or ""
        elif self.provider == "anthropic":
            key = os.getenv("ANTHROPIC_API_KEY", "").strip()
            if not key:
                key = (await self.db.get_api_key("claude", user_email=self.user_email)) or ""

        key = key.strip()
        if not key and not self._missing_key_logged:
            logger.info(
                "[AIParticipant] %s API key not found for %s",
                self.provider,
                self.user_email,
            )
            self._missing_key_logged = True

        self._provider_api_key = key or None
        return self._provider_api_key

    def _build_prompt(self, transcript_window: str) -> str:
        title = self.meeting_context.title or ""
        goal = self.meeting_context.goal or ""
        description = self.meeting_context.description or ""
        agenda_text = self.meeting_context.agenda_text or ""
        participant_names = self.meeting_context.participant_names or []
        participant_line = ", ".join(participant_names[:25]) if participant_names else "None"

        return f"""
You are a silent meeting observer. Stay silent unless a guardrail condition is detected.

Meeting Context:
- Title: {title}
- Goal: {goal}
- Description: {description}
- Agenda: {agenda_text}
- Participants: {participant_line}
- Previous alert summary: {self._last_alert_summary}

Guardrail reasons:
- agenda_deviation
- no_decision
- unresolved_question
- missing_context_or_repeat

Rules:
- If no intervention is required, return: {{"intervention_required": false}}
- If intervention is required, return strict JSON:
  {{"intervention_required": true, "reason": "...", "insight": "...", "confidence": 0.0}}
- Insight must be one actionable sentence and no more than 30 words.
- Reason must be one of: agenda_deviation, no_decision, unresolved_question, missing_context_or_repeat.
- Return JSON only. No markdown.

Recent transcript window:
{transcript_window}
""".strip()

    def _build_host_prompt(self, transcript_window: str) -> str:
        title = self.meeting_context.title or ""
        goal = self.meeting_context.goal or ""
        description = self.meeting_context.description or ""
        agenda_text = self.meeting_context.agenda_text or ""
        participant_names = self.meeting_context.participant_names or []
        participant_line = ", ".join(participant_names[:25]) if participant_names else "None"

        policy = self._host_policy
        role = policy.role_mode.value
        skill_definition = self._active_skill_definition or {}
        skill_name = str(skill_definition.get("name") or role.title())
        skill_description = str(skill_definition.get("description") or "").strip() or "None"
        skill_role = str(skill_definition.get("role") or "").strip() or "None"
        skill_goals = skill_definition.get("goals") or []
        skill_rules = skill_definition.get("rules") or []
        allowed_custom_types = skill_definition.get("allowed_custom_event_types") or []

        pinned_titles = [item.title for item in self._host_state.pinned_items]
        pinned_line = ", ".join(pinned_titles) if pinned_titles else "None"
        goals_block = "\n".join(f"- {goal_item}" for goal_item in skill_goals) if skill_goals else "- None"
        rules_block = "\n".join(f"- {rule_item}" for rule_item in skill_rules) if skill_rules else "- None"
        custom_types_block = (
            "\n".join(f"- {event_type}" for event_type in allowed_custom_types)
            if allowed_custom_types
            else "- None"
        )

        return f"""
You are an active AI Participant in this meeting. Generate event suggestions conservatively, based only on transcript and meeting context.

Meeting Context:
- Title: {title}
- Goal: {goal}
- Description: {description}
- Agenda: {agenda_text}
- Participants: {participant_line}
- Role Mode: {role}
- Current Summary: {self._host_state.meeting_summary or "None"}
- Already Pinned Decisions/Topics: {pinned_line}
- Skill Name: {skill_name}
- Skill Description: {skill_description}

Skill Role:
{skill_role}

Skill Goals:
{goals_block}

Skill Rules:
{rules_block}

Reserved core event_type values:
- decision_candidate
- open_discussion

Allowed custom event_type values from the active skill:
{custom_types_block}

Rules:
- Return strict JSON object only with this shape:
  {{"meeting_summary": "...", "events": [{{"event_type": "...", "title": "...", "content": "...", "confidence": 0.0, "priority": "low|medium|high", "source_excerpt": "..."}}]}}
- `meeting_summary` should be a rolling 2-3 sentence paragraph summarizing the entire meeting up to this point. Include key details from the past 20-30 minutes if applicable.
- If no event is needed, return {{"events": []}}.
- Do NOT suggest events for topics or decisions that are already in the "Already Pinned Decisions/Topics" list.
- ONLY output a `decision_candidate` if an explicit choice, commitment, or action has been agreed upon by participants.
- ONLY output an `open_discussion` when the transcript shows an unresolved question, active debate, disagreement, or conflict that still needs resolution.
- Any non-core event must use one of the allowed custom event types from the active skill.
- Do NOT classify informational statements, general discussion, tech news summaries, or observations as decisions.
- Keep title <= 10 words and content <= 35 words.
- Avoid personal criticism or blame.
- Do not hallucinate facts outside provided transcript/context.

Recent transcript window:
{transcript_window}
""".strip()

    def _build_host_suggestion(self, event: Dict[str, Any]) -> Optional[HostSuggestion]:
        event_type = event.get("event_type")
        if not isinstance(event_type, str) or not event_type.strip():
            return None

        confidence = float(event.get("confidence") or 0.0)
        threshold = float(
            self._host_policy.event_threshold_overrides.get(event_type)
            or self._host_policy.min_confidence
        )
        if confidence < threshold:
            return None

        title = " ".join(str(event.get("title") or "").split()).strip()
        content = " ".join(str(event.get("content") or "").split()).strip()
        if not content:
            return None

        return HostSuggestion(
            id=str(uuid.uuid4()),
            event_type=event_type,
            title=title or event_type.replace("_", " ").title(),
            content=content,
            confidence=round(confidence, 2),
            timestamp=datetime.utcnow().isoformat(),
            source_excerpt=event.get("source_excerpt"),
            metadata={"priority": event.get("priority", "medium")},
        )

    def _build_intervention_from_suggestion(
        self,
        suggestion: HostSuggestion,
        now_ts: float,
    ) -> Optional[HostInterventionCard]:
        event_key = suggestion.event_type
        cooldown_seconds = int(self._host_policy.intervention_cooldown_seconds)
        last_ts = float(self._host_event_last_published_at.get(event_key) or 0.0)
        if (now_ts - last_ts) < cooldown_seconds:
            return None

        signature = self._suggestion_signature(suggestion)
        if self._host_event_last_signature.get(event_key) == signature:
            return None

        if not self._should_intervene(suggestion):
            return None

        self._host_event_last_published_at[event_key] = now_ts
        self._host_event_last_signature[event_key] = signature
        return HostInterventionCard(
            id=str(uuid.uuid4()),
            event_type=suggestion.event_type,
            headline=suggestion.title,
            body=suggestion.content,
            priority=str(suggestion.metadata.get("priority") or "medium"),
            confidence=suggestion.confidence,
            timestamp=datetime.utcnow().isoformat(),
            linked_suggestion_id=suggestion.id,
        )

    def _should_intervene(self, suggestion: HostSuggestion) -> bool:
        role = self._host_policy.role_mode
        confidence = float(suggestion.confidence)
        event_type = suggestion.event_type
        priority = str((suggestion.metadata or {}).get("priority") or "medium").lower()

        if role == HostRoleMode.ADVISOR:
            return event_type in CORE_EVENT_TYPES or priority == "high"

        if role == HostRoleMode.FACILITATOR:
            return confidence >= max(0.72, self._host_policy.min_confidence)

        return event_type in CORE_EVENT_TYPES or confidence >= max(
            0.65, self._host_policy.min_confidence - 0.03
        )

    @staticmethod
    def _suggestion_signature(suggestion: HostSuggestion) -> str:
        text = " ".join((suggestion.content or "").strip().lower().split())
        return f"{suggestion.event_type}:{text}"

    def _normalize_host_event_type(self, event_type_value: Any) -> Optional[str]:
        raw = str(event_type_value or "").strip().lower()
        if not raw:
            return None

        aliases = {
            "open_question": "open_discussion",
            "discussion_open": "open_discussion",
            "decision": "decision_candidate",
        }
        normalized = aliases.get(raw, raw)
        normalized = normalized.replace("-", "_").replace(" ", "_")
        normalized = re.sub(r"[^a-z0-9_]", "", normalized)
        normalized = re.sub(r"_+", "_", normalized).strip("_")
        if not normalized:
            return None

        allowed_custom = set(
            self._active_skill_definition.get("allowed_custom_event_types") or []
        )
        if normalized in CORE_EVENT_TYPES or normalized in allowed_custom:
            return normalized
        return None

    def _refresh_host_state_from_events(self, events: List[Dict[str, Any]]) -> None:
        current_topic = ""
        unresolved_items: List[str] = []
        for event in events:
            content = " ".join(str(event.get("content") or "").split()).strip()
            if not content:
                continue
            if not current_topic:
                current_topic = " ".join(str(event.get("title") or "").split()).strip()
            if event.get("event_type") == "open_discussion" and content not in unresolved_items:
                unresolved_items.append(content)

        if current_topic:
            self._host_state.current_topic = current_topic
        if unresolved_items:
            self._host_state.unresolved_items = unresolved_items[:8]

    def _load_policy_from_skill(self, skill_text: str, source: str) -> HostPolicyConfig:
        policy = HostPolicyConfig(source=source)
        if not skill_text:
            return policy

        parsed = self._parse_simple_skill_text(skill_text)
        inferred = self._infer_policy_from_markdown(skill_text)
        for key, value in inferred.items():
            parsed.setdefault(key, value)

        role_raw = str(parsed.get("role_mode") or "").strip().lower()
        if role_raw in {"facilitator", "advisor", "chairperson"}:
            policy.role_mode = HostRoleMode(role_raw)

        for key in (
            "min_confidence",
            "suggestion_cooldown_seconds",
            "intervention_cooldown_seconds",
            "max_suggestions_buffer",
            "max_intervention_history",
            "max_pinned_items",
        ):
            if key in parsed:
                self._apply_policy_numeric(policy, key, parsed[key])

        if "allow_interruptions" in parsed:
            policy.allow_interruptions = str(parsed["allow_interruptions"]).strip().lower() in {
                "1",
                "true",
                "yes",
                "on",
            }

        forbidden = parsed.get("forbidden_actions")
        if forbidden:
            policy.forbidden_actions = [
                v.strip()
                for v in str(forbidden).split(",")
                if v and v.strip()
            ]

        for key, raw_value in parsed.items():
            if not str(key).startswith("threshold_"):
                continue
            event_type = str(key).replace("threshold_", "", 1).strip().lower()
            event_type = self._normalize_host_event_type(event_type) or event_type
            try:
                val = float(raw_value)
                policy.event_threshold_overrides[event_type] = max(0.0, min(1.0, val))
            except Exception:
                continue

        return policy

    @staticmethod
    def _infer_policy_from_markdown(skill_text: str) -> Dict[str, Any]:
        inferred: Dict[str, Any] = {}
        text = str(skill_text or "")
        lower = text.lower()

        # Role inference from human-readable role/identity sections
        if any(token in lower for token in ["chairperson", "team lead", "tech lead", "engineering lead"]):
            inferred["role_mode"] = "chairperson"
        elif any(token in lower for token in ["advisor", "consultant", "observer"]):
            inferred["role_mode"] = "advisor"
        elif any(token in lower for token in ["facilitator", "moderator", "host"]):
            inferred["role_mode"] = "facilitator"

        # Interaction style hints
        if "always ask clarifying questions" in lower:
            inferred.setdefault("min_confidence", "0.72")
            inferred.setdefault("threshold_open_discussion", "0.66")

        if any(token in lower for token in ["default to simplicity", "simplicity over clever", "simple over clever"]):
            inferred["forbidden_actions"] = "overengineered_solutions, shame_participants, legal_advice"

        if any(token in lower for token in ["direct and confident", "drive decisions", "time-box", "timebox"]):
            inferred.setdefault("intervention_cooldown_seconds", "90")
            inferred.setdefault("threshold_decision_candidate", "0.65")

        return inferred

    @staticmethod
    def _parse_simple_skill_text(skill_text: str) -> Dict[str, str]:
        parsed: Dict[str, str] = {}
        text = str(skill_text or "")

        # Prefer fenced code blocks (yaml/yml/toml/ini/txt) if present.
        # This allows users to paste markdown docs with an embedded config block.
        fence_blocks = re.findall(
            r"```(?:yaml|yml|toml|ini|txt)?\s*(.*?)\s*```", text, flags=re.DOTALL
        )
        candidate = fence_blocks[0] if fence_blocks else text

        for line in candidate.splitlines():
            item = line.strip()
            if not item:
                continue
            if item.startswith("#"):
                continue
            if item.startswith("- "):
                item = item[2:].strip()
            if item.startswith("* "):
                item = item[2:].strip()
            if ":" not in item:
                continue
            key, value = item.split(":", 1)
            key = key.strip().strip("`").lower()
            value = value.strip().strip("`")
            if not key:
                continue
            parsed[key] = value
        return parsed

    @staticmethod
    def _apply_policy_numeric(policy: HostPolicyConfig, key: str, value: Any) -> None:
        try:
            if key in {"min_confidence"}:
                setattr(policy, key, max(0.0, min(1.0, float(value))))
            else:
                setattr(policy, key, max(1, int(float(value))))
        except Exception:
            return

    def apply_host_skill_override(self, skill_markdown: str, source: str = "meeting") -> None:
        skill_text = (skill_markdown or "").strip()
        if not skill_text:
            return
        self._active_skill_markdown = skill_text
        self._active_skill_definition = parse_skill_markdown(skill_text)
        self._host_policy = self._load_policy_from_skill(skill_text, source=source)
        self._host_policy_source = source
        self._stats["host_policy_source"] = source

    def set_host_template(self, template_name: str, source: str = "system") -> None:
        template_key = str(template_name or "").strip().lower()
        skill_text = SYSTEM_HOST_SKILLS.get(template_key)
        if not skill_text:
            return
        self._active_skill_markdown = skill_text
        self._active_skill_definition = parse_skill_markdown(skill_text)
        self._host_policy = self._load_policy_from_skill(skill_text, source=source)
        self._host_policy_source = source
        self._stats["host_policy_source"] = source

    def pin_suggestion(self, suggestion_id: str, actor: Optional[str] = None) -> Optional[HostSuggestion]:
        suggestion_id = str(suggestion_id or "").strip()
        if not suggestion_id:
            return None

        match = None
        remaining: List[HostSuggestion] = []
        for item in self._host_state.suggested_items:
            if item.id == suggestion_id and match is None:
                match = item
            else:
                remaining.append(item)

        if not match:
            for item in self._host_state.pinned_items:
                if item.id == suggestion_id:
                    return item
            return None

        match.status = "pinned"
        meta = dict(match.metadata or {})
        if actor:
            meta["pinned_by"] = actor
        meta["pinned_at"] = datetime.utcnow().isoformat()
        match.metadata = meta

        self._host_state.suggested_items = remaining
        self._host_state.pinned_items.insert(0, match)
        self._host_state.pinned_items = self._host_state.pinned_items[: self._host_policy.max_pinned_items]
        self._host_state.counters["pinned"] = int(self._host_state.counters.get("pinned") or 0) + 1
        self._host_state.updated_at = datetime.utcnow().isoformat()
        self._stats["host_suggestions_pinned"] += 1
        return match

    def dismiss_suggestion(self, suggestion_id: str, actor: Optional[str] = None) -> bool:
        suggestion_id = str(suggestion_id or "").strip()
        if not suggestion_id:
            return False

        remaining: List[HostSuggestion] = []
        removed = False
        for item in self._host_state.suggested_items:
            if item.id == suggestion_id and not removed:
                removed = True
                continue
            remaining.append(item)

        if not removed:
            return False

        self._host_state.suggested_items = remaining
        if suggestion_id not in self._host_state.dismissed_item_ids:
            self._host_state.dismissed_item_ids.insert(0, suggestion_id)
            self._host_state.dismissed_item_ids = self._host_state.dismissed_item_ids[:200]
        self._host_state.counters["dismissed"] = int(self._host_state.counters.get("dismissed") or 0) + 1
        self._host_state.updated_at = datetime.utcnow().isoformat()
        self._stats["host_suggestions_dismissed"] += 1

        if actor:
            self._host_state.last_response_outcomes.insert(0, f"dismissed_by:{actor}")
            self._host_state.last_response_outcomes = self._host_state.last_response_outcomes[:50]
        return True

    def record_feedback(self, suggestion_id: str, feedback: str, actor: Optional[str] = None) -> None:
        entry = f"feedback:{suggestion_id}:{feedback}"
        if actor:
            entry += f":{actor}"
        self._host_state.last_response_outcomes.insert(0, entry[:300])
        self._host_state.last_response_outcomes = self._host_state.last_response_outcomes[:50]
        self._host_state.updated_at = datetime.utcnow().isoformat()

    def get_host_state_snapshot(self) -> Dict[str, Any]:
        state = self._host_state.model_dump()
        state["policy_source"] = self._host_policy_source
        state["policy_role_mode"] = self._host_policy.role_mode.value
        return state

    @staticmethod
    def _normalize_reason(reason_value: str) -> Optional[str]:
        if not reason_value:
            return None
        raw = str(reason_value).strip().lower().replace("-", "_").replace(" ", "_")
        aliases = {
            "agenda_deviation": "agenda_deviation",
            "no_decision": "no_decision",
            "long_discussion_without_decision": "no_decision",
            "unresolved_question": "unresolved_question",
            "important_unresolved_question": "unresolved_question",
            "missing_context_or_repeat": "missing_context_or_repeat",
            "missing_context": "missing_context_or_repeat",
            "repeated_topic": "missing_context_or_repeat",
        }
        return aliases.get(raw)

    @classmethod
    def _normalize_model_payload(cls, payload: Dict) -> Optional[Dict]:
        if not isinstance(payload, dict):
            return None

        intervention_required = bool(payload.get("intervention_required", False))
        if not intervention_required:
            return {"intervention_required": False}

        reason = cls._normalize_reason(payload.get("reason"))
        insight = " ".join(str(payload.get("insight") or "").split()).strip()
        confidence_raw = payload.get("confidence", 0.0)
        try:
            confidence = float(confidence_raw)
        except Exception:
            confidence = 0.0
        confidence = max(0.0, min(1.0, confidence))

        if not reason or not insight:
            return {"intervention_required": False}

        return {
            "intervention_required": True,
            "reason": reason,
            "insight": insight,
            "confidence": confidence,
        }

    @staticmethod
    def _extract_json(raw_text: str) -> Optional[Dict]:
        text = (raw_text or "").strip()
        if not text:
            return None

        try:
            obj = json.loads(text)
            return obj if isinstance(obj, dict) else None
        except Exception:
            pass

        fence_match = re.search(r"```(?:json)?\s*(\{.*?\})\s*```", text, re.DOTALL)
        if fence_match:
            try:
                obj = json.loads(fence_match.group(1))
                return obj if isinstance(obj, dict) else None
            except Exception:
                return None

        start = text.find("{")
        end = text.rfind("}")
        if start >= 0 and end > start:
            try:
                obj = json.loads(text[start : end + 1])
                return obj if isinstance(obj, dict) else None
            except Exception:
                return None

        return None

    def get_stats_snapshot(self) -> Dict[str, Any]:
        payload = dict(self._stats)
        payload["window_char_count"] = self.buffer.get_char_count()
        payload["window_duration_seconds"] = round(
            self.buffer.get_duration_seconds(), 3
        )
        payload["evaluator"] = self.evaluator.get_metrics_snapshot()
        payload["provider"] = self.provider
        payload["model"] = self.model_name
        payload["active_skill"] = self._active_skill_definition
        payload["host_policy"] = self._host_policy.model_dump()
        payload["host_state"] = self.get_host_state_snapshot()
        suggested = int(payload.get("host_suggestions_emitted") or 0)
        pinned = int(payload.get("host_suggestions_pinned") or 0)
        dismissed = int(payload.get("host_suggestions_dismissed") or 0)
        payload["host_quality"] = {
            "pin_rate": round((pinned / suggested), 4) if suggested > 0 else 0.0,
            "dismiss_rate": round((dismissed / suggested), 4) if suggested > 0 else 0.0,
            "suggested": suggested,
            "pinned": pinned,
            "dismissed": dismissed,
        }
        return payload

    def apply_manual_context(
        self,
        goal: Optional[str] = None,
        agenda_text: Optional[str] = None,
        participant_names: Optional[List[str]] = None,
    ) -> None:
        if goal is not None:
            self.meeting_context.goal = (goal or "").strip()
        if agenda_text is not None:
            self.meeting_context.agenda_text = (agenda_text or "").strip()
        if participant_names is not None:
            cleaned: List[str] = []
            for name in participant_names or []:
                value = " ".join(str(name or "").split()).strip()
                if value and value not in cleaned:
                    cleaned.append(value)
            self.meeting_context.participant_names = cleaned
