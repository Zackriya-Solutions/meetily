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

try:
    from ..schemas.ai_participant import (
        GuardrailAlert,
        GuardrailLLMOutput,
        GuardrailReason,
    )
    from .gemini_client import generate_content_text_async
except (ImportError, ValueError):
    from schemas.ai_participant import (
        GuardrailAlert,
        GuardrailLLMOutput,
        GuardrailReason,
    )
    from services.gemini_client import generate_content_text_async

logger = logging.getLogger(__name__)


@dataclass
class MeetingContext:
    meeting_id: str
    title: str = ""
    goal: str = ""
    description: str = ""
    agenda_text: str = ""
    participant_names: List[str] = None


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
            logger.debug("[AIParticipant][Buffer] Pruned old text due to char limit. Current count: %s", self._char_count)

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
        by_reason[assessment.reason.value] = (
            int(by_reason.get(assessment.reason.value) or 0) + 1
        )

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
        by_reason = dict(self._metrics.get("published_by_reason") or {})
        payload = dict(self._metrics)
        payload["published_by_reason"] = by_reason
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
        self.model_name = os.getenv("AI_PARTICIPANT_MODEL", "gemini-3-pro-preview")
        fallback_models = os.getenv(
            "AI_PARTICIPANT_FALLBACK_MODELS", "gemini-3-pro-preview,gemini-3-flash-preview"
        )
        self.fallback_models = [
            m.strip() for m in fallback_models.split(",") if (m or ""   ).strip()
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

        logger.info(
            "[AIParticipant] Engine initialized meeting_id=%s user=%s window=%ss max_chars=%s interval=%ss",
            self.meeting_context.meeting_id,
            self.user_email,
            window_seconds,
            max_chars,
            self.analysis_interval_seconds,
        )

        self._last_analysis_at = 0.0
        self._lock = asyncio.Lock()
        self._gemini_api_key: Optional[str] = None
        self._missing_key_logged = False
        self._last_alert_summary = "None"
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
            "last_assessment_intervention_required": None,
            "last_assessment_reason": None,
            "last_assessment_confidence": None,
            "last_analysis_at": None,
        }

    async def ingest_transcript(
        self,
        text: str,
        transcript_time_seconds: Optional[float] = None,
    ) -> Optional[GuardrailAlert]:
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
            if self.verbose_logs:
                logger.info(
                    "[AIParticipant][Cycle] skipped_small_window meeting=%s chars=%s min_chars=%s",
                    self.meeting_context.meeting_id,
                    self.buffer.get_char_count(),
                    self.min_chars_before_analysis,
                )
            return None

        if now_ts - self._last_analysis_at < self.analysis_interval_seconds:
            self._stats["analysis_skipped_interval"] += 1
            if self.verbose_logs:
                logger.info(
                    "[AIParticipant][Cycle] skipped_interval meeting=%s since_last=%.1fs interval=%ss chars=%s",
                    self.meeting_context.meeting_id,
                    now_ts - self._last_analysis_at,
                    self.analysis_interval_seconds,
                    self.buffer.get_char_count(),
                )
            return None

        async with self._lock:
            now_ts = time.time()
            if now_ts - self._last_analysis_at < self.analysis_interval_seconds:
                self._stats["analysis_skipped_interval"] += 1
                if self.verbose_logs:
                    logger.info(
                        "[AIParticipant][Cycle] skipped_interval_locked meeting=%s since_last=%.1fs interval=%ss",
                        self.meeting_context.meeting_id,
                        now_ts - self._last_analysis_at,
                        self.analysis_interval_seconds,
                    )
                return None
            self._last_analysis_at = now_ts
            self._stats["analysis_attempts"] += 1
            self._stats["last_analysis_at"] = datetime.utcnow().isoformat()
            if self.verbose_logs:
                logger.info(
                    "[AIParticipant][Cycle] analysis_start meeting=%s attempt=%s chars=%s duration=%.1fs model=%s",
                    self.meeting_context.meeting_id,
                    self._stats["analysis_attempts"],
                    self.buffer.get_char_count(),
                    self.buffer.get_duration_seconds(),
                    self.model_name,
                )

            assessment = await self._reason_with_llm()
            if not assessment:
                self._stats["assessment_none"] += 1
                if self.verbose_logs:
                    logger.info(
                        "[AIParticipant][Cycle] assessment_none meeting=%s llm_failures=%s parse_failures=%s",
                        self.meeting_context.meeting_id,
                        self._stats.get("llm_failures", 0),
                        self._stats.get("parse_failures", 0),
                    )
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
            if self.verbose_logs:
                logger.info(
                    "[AIParticipant] Assessed meeting=%s intervention=%s reason=%s confidence=%.2f window_chars=%s window_duration=%.1fs model=%s",
                    self.meeting_context.meeting_id,
                    bool(assessment.intervention_required),
                    assessment.reason.value if assessment.reason else None,
                    float(assessment.confidence or 0.0),
                    self.buffer.get_char_count(),
                    self.buffer.get_duration_seconds(),
                    self._stats.get("last_model_used") or self.model_name,
                )

            alert = self.evaluator.evaluate(
                assessment=assessment,
                window_duration_seconds=self.buffer.get_duration_seconds(),
                now_ts=now_ts,
            )
            if alert:
                self._last_alert_summary = f"{alert.reason.value}: {alert.insight}"
            return alert

    async def _reason_with_llm(self) -> Optional[GuardrailLLMOutput]:
        api_key = await self._get_gemini_api_key()
        if not api_key:
            return None

        transcript_window = self.buffer.get_text()
        if not transcript_window:
            return None

        prompt = self._build_prompt(transcript_window)

        model_candidates: List[str] = []
        for model in [self.model_name, *self.fallback_models]:
            if model and model not in model_candidates:
                model_candidates.append(model)

        last_exc: Optional[Exception] = None
        raw_text = ""
        used_model = self.model_name
        for idx, model in enumerate(model_candidates):
            try:
                self._stats["llm_calls"] += 1
                used_model = model
                raw_text = await asyncio.wait_for(
                    generate_content_text_async(
                        api_key=api_key,
                        model=model,
                        contents=prompt,
                        config={"temperature": 0.1},
                    ),
                    timeout=self.llm_timeout_seconds,
                )
                if idx > 0:
                    self._stats["model_fallbacks"] += 1
                    logger.info(
                        "[AIParticipant] Fallback model selected: %s (primary: %s)",
                        model,
                        self.model_name,
                    )
                break
            except (asyncio.TimeoutError, TimeoutError):
                self._stats["llm_failures"] += 1
                self._stats["llm_timeouts"] += 1
                logger.warning(
                    "[AIParticipant] Reasoning timeout on model %s after %.2fs",
                    model,
                    self.llm_timeout_seconds,
                )
                if idx == len(model_candidates) - 1:
                    return None
                continue
            except Exception as exc:
                self._stats["llm_failures"] += 1
                message = str(exc)
                logger.warning("[AIParticipant] Reasoning failed on model %s: %s", model, message)
                
                not_found = "NOT_FOUND" in message or "not found" in message.lower()
                if idx < len(model_candidates) - 1:
                    logger.info("[AIParticipant] Trying fallback model due to error...")
                    continue
                return None

        try:
            self._stats["last_model_used"] = used_model
            parsed = self._extract_json(raw_text)
            if not parsed:
                self._stats["parse_failures"] += 1
                logger.warning(
                    "[AIParticipant] JSON parsing failed. Raw response: %s",
                    raw_text[:200],
                )
                return None
            normalized = self._normalize_model_payload(parsed)
            if not normalized:
                self._stats["normalize_silent_fallbacks"] += 1
                logger.warning(
                    "[AIParticipant] Response normalization failed: %s", parsed
                )
                return None
            if (
                isinstance(parsed, dict)
                and bool(parsed.get("intervention_required", False))
                and not bool(normalized.get("intervention_required", False))
            ):
                self._stats["normalize_silent_fallbacks"] += 1
            return GuardrailLLMOutput.model_validate(normalized)
        except asyncio.TimeoutError:
            self._stats["llm_failures"] += 1
            self._stats["llm_timeouts"] += 1
            logger.warning(
                "[AIParticipant] Reasoning timeout after %.2fs",
                self.llm_timeout_seconds,
            )
            return None
        except Exception as exc:
            self._stats["llm_failures"] += 1
            logger.warning("[AIParticipant] Reasoning failed: %s", exc)
            return None

    async def _get_gemini_api_key(self) -> Optional[str]:
        if self._gemini_api_key:
            return self._gemini_api_key

        # Prioritize Environment Variables over Database
        key = os.getenv("GEMINI_API_KEY", "").strip() or os.getenv("GOOGLE_API_KEY", "").strip()

        if not key:
            key = (await self.db.get_api_key("gemini", user_email=self.user_email)) or ""
            key = key.strip()

        if not key and not self._missing_key_logged:
            logger.info(
                "[AIParticipant] Gemini API key not found for %s", self.user_email
            )
            self._missing_key_logged = True

        self._gemini_api_key = key or None
        return self._gemini_api_key

    def _build_prompt(self, transcript_window: str) -> str:
        title = self.meeting_context.title or ""
        goal = self.meeting_context.goal or ""
        description = self.meeting_context.description or ""
        agenda_text = self.meeting_context.agenda_text or ""
        participant_names = self.meeting_context.participant_names or []
        participant_line = (
            ", ".join(participant_names[:25]) if participant_names else "None"
        )

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
- If transcript is repetitive/noisy/filler or clearly unrelated to the meeting goal/agenda, prefer reason=agenda_deviation.
- Do not stay silent just because transcript quality is poor; poor or irrelevant discussion is still a guardrail signal.
- Use only provided meeting context and transcript text.
- Return JSON only. No markdown.

Recent transcript window:
{transcript_window}
""".strip()

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

        # Direct JSON path
        try:
            obj = json.loads(text)
            return obj if isinstance(obj, dict) else None
        except Exception:
            pass

        # Code fence path
        fence_match = re.search(r"```(?:json)?\s*(\{.*?\})\s*```", text, re.DOTALL)
        if fence_match:
            try:
                obj = json.loads(fence_match.group(1))
                return obj if isinstance(obj, dict) else None
            except Exception:
                return None

        # First object heuristic
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
        payload["model"] = self.model_name
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
