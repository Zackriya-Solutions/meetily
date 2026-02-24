from fastapi import APIRouter, Depends, HTTPException, BackgroundTasks
from fastapi.responses import JSONResponse, StreamingResponse
from typing import Optional, Tuple
import logging
import json
import uuid
import os
import asyncio
import tempfile
import re
from pathlib import Path

try:
    from ..deps import get_current_user
    from ...schemas.user import User
    from ...schemas.transcript import (
        TranscriptRequest,
        SaveTranscriptRequest,
    )
    from ...schemas.meeting import (
        SaveSummaryRequest,
        RefineNotesRequest,
        GenerateNotesRequest,
    )
    from ...db import DatabaseManager
    from ...core.rbac import RBAC
    from ...services.summarization import SummarizationService
    from ...services.audio.vad import SimpleVAD
    from ...services.audio.groq_client import GroqTranscriptionClient
    from ...services.chat import ChatService
    from ...services.gemini_client import generate_content_with_file_sync
    from ...services.storage import StorageService
    from ...services.calendar.google_oauth import GoogleCalendarOAuthService
    from ...services.calendar.reminder_email import CalendarReminderEmailService
except (ImportError, ValueError):
    from api.deps import get_current_user
    from schemas.user import User
    from schemas.transcript import (
        TranscriptRequest,
        SaveTranscriptRequest,
    )
    from schemas.meeting import (
        SaveSummaryRequest,
        RefineNotesRequest,
        GenerateNotesRequest,
    )
    from db import DatabaseManager
    from core.rbac import RBAC
    from services.summarization import SummarizationService
    from services.audio.vad import SimpleVAD
    from services.audio.groq_client import GroqTranscriptionClient
    from services.chat import ChatService
    from services.gemini_client import generate_content_with_file_sync
    from services.storage import StorageService
    from services.calendar.google_oauth import GoogleCalendarOAuthService
    from services.calendar.reminder_email import CalendarReminderEmailService

# Initialize services
db = DatabaseManager()
rbac = RBAC(db)
processor = SummarizationService()

router = APIRouter()
logger = logging.getLogger(__name__)
DIARIZED_SOURCES = {"diarized", "diarization"}


# --- Meeting Templates with Optimized Prompts ---
def get_template_prompt(template_id: str) -> str:
    """
    Get the structured prompt for a specific meeting template.
    Optimized for token efficiency while maintaining quality.
    """
    templates = {
        "standard_meeting": """Generate professional meeting notes as valid JSON:

{
  "MeetingName": "Descriptive title from content",
  "People": {"title": "Participants", "blocks": [{"content": "Name - Role"}]},
  "SessionSummary": {"title": "Executive Summary", "blocks": [{"content": "2-3 paragraphs: purpose, topics, outcomes"}]},
  "KeyItemsDecisions": {"title": "Key Decisions", "blocks": [{"content": "Decision - Rationale - Owner/Date"}]},
  "ImmediateActionItems": {"title": "Action Items", "blocks": [{"content": "[Person] will [action] by [deadline]"}]},
  "NextSteps": {"title": "Next Steps", "blocks": [{"content": "What's next - Timeline - Owner"}]},
  "CriticalDeadlines": {"title": "Deadlines", "blocks": [{"content": "[Date] - [Deliverable] - Owner"}]},
  "MeetingNotes": {"meeting_name": "Same as MeetingName", "sections": [{"title": "Topic", "blocks": [{"content": "Details"}]}]}
}

Rules: Extract names/dates/commitments. Make action items SMART. Capture discussions AND decisions. Flag blockers/risks. Be concise. Empty blocks [] if missing. Organize by topic.
""",
        "daily_standup": """Generate Daily Standup notes as valid JSON:

{
  "MeetingName": "Daily Standup - [Team] - [Date]",
  "People": {"title": "Team Members", "blocks": [{"content": "Name - Role"}]},
  "SessionSummary": {"title": "Overview", "blocks": [{"content": "Team progress, velocity, blockers"}]},
  "MeetingNotes": {"meeting_name": "Same as MeetingName", "sections": [{"title": "[Person] - Updates", "blocks": [{"content": "✅ Done: [Task]"}, {"content": "🎯 Today: [Task]"}, {"content": "🚧 Blocked: [Issue]"}]}]},
  "KeyItemsDecisions": {"title": "Decisions", "blocks": [{"content": "Decision - Context"}]},
  "ImmediateActionItems": {"title": "Actions", "blocks": [{"content": "[Person] will [action] - By when"}]},
  "CriticalDeadlines": {"title": "Sprint Deadlines", "blocks": [{"content": "[Date] - [Milestone]"}]},
  "NextSteps": {"title": "Next Standup", "blocks": [{"content": "Items to track"}]}
}

Rules: One section per person. Use ✅ done, 🎯 today, 🚧 blocked. Extract task names/IDs. Highlight dependencies/blockers. Flag recurring issues. Keep brief.
""",
        "brainstorming": """Generate Brainstorming notes as valid JSON:

{
  "MeetingName": "Brainstorming - [Topic]",
  "People": {"title": "Participants", "blocks": [{"content": "Name - Expertise"}]},
  "SessionSummary": {"title": "Overview", "blocks": [{"content": "Problem statement"}, {"content": "Approach used"}, {"content": "Ideas count & selection"}]},
  "MeetingNotes": {"meeting_name": "Same as MeetingName", "sections": [{"title": "Ideas - [Theme]", "blocks": [{"content": "💡 [Title]: Description - By [Person] - Pros/cons"}]}, {"title": "Top Ideas", "blocks": [{"content": "⭐ [Title]: Why selected - Next steps"}]}, {"title": "Parked", "blocks": [{"content": "🅿️ [Title]: Reason - Revisit conditions"}]}]},
  "KeyItemsDecisions": {"title": "Decisions", "blocks": [{"content": "Ideas to pursue - Criteria - Timeline"}]},
  "ImmediateActionItems": {"title": "Validation", "blocks": [{"content": "[Person] will [test] [idea] by [date]"}]},
  "CriticalDeadlines": {"title": "Validation Deadlines", "blocks": [{"content": "[Date] - [Milestone] - Owner"}]},
  "NextSteps": {"title": "Follow-up", "blocks": [{"content": "Reconvene when - Prepare what"}]}
}

Rules: Group by theme. Attribute ideas. Note WHY selected/parked. Document constraints. ID quick wins vs long-term. Use 💡 all, ⭐ selected, 🅿️ parked.
""",
        "interview": """Generate Interview Assessment as valid JSON:

{
  "MeetingName": "Interview - [Candidate] - [Position]",
  "People": {"title": "Panel", "blocks": [{"content": "Name - Role - Focus"}]},
  "SessionSummary": {"title": "Candidate Overview", "blocks": [{"content": "Background summary"}, {"content": "Format & areas"}, {"content": "Overall: Hire/No Hire/Maybe"}]},
  "MeetingNotes": {"meeting_name": "Same as MeetingName", "sections": [{"title": "Technical Skills", "blocks": [{"content": "✅ Strength: [Skill] - Evidence"}, {"content": "⚠️ Gap: [Skill] - Example"}]}, {"title": "Behavioral", "blocks": [{"content": "✅ Strength: [Skill] - Examples"}, {"content": "⚠️ Concern: [Area] - Behaviors"}]}, {"title": "Cultural Fit", "blocks": [{"content": "Fit assessment - Examples"}]}, {"title": "Candidate Questions", "blocks": [{"content": "Question - Quality"}]}]},
  "KeyItemsDecisions": {"title": "Assessment", "blocks": [{"content": "Recommendation: [Strong Yes/Yes/Maybe/No/Strong No] - Why"}, {"content": "Salary: [If discussed]"}, {"content": "Notice: [If discussed]"}]},
  "ImmediateActionItems": {"title": "Next Steps", "blocks": [{"content": "[Recruiter] will [action] by [date]"}]},
  "NextSteps": {"title": "Follow-up", "blocks": [{"content": "References - Who"}, {"content": "Additional rounds - Focus"}, {"content": "Comp discussion"}]},
  "CriticalDeadlines": {"title": "Timeline", "blocks": [{"content": "[Date] - Decision deadline"}]}
}

Rules: Use specific examples not impressions. Separate technical/soft skills. Note red flags professionally. Capture candidate questions. Document comp factually. Use ✅ strengths, ⚠️ gaps.
""",
        "project_kickoff": """Generate Project Kickoff notes as valid JSON:

{
  "MeetingName": "Project Kickoff - [Project]",
  "People": {"title": "Team & Stakeholders", "blocks": [{"content": "Name - Role - Responsibilities"}]},
  "SessionSummary": {"title": "Overview", "blocks": [{"content": "Vision & goals"}, {"content": "Success criteria & metrics"}, {"content": "Constraints (budget, timeline, resources)"}]},
  "MeetingNotes": {"meeting_name": "Same as MeetingName", "sections": [{"title": "Scope", "blocks": [{"content": "✅ In: [Item] - Why"}, {"content": "❌ Out: [Item] - Why"}]}, {"title": "RACI", "blocks": [{"content": "[Person] - Responsible for [area] - Accountable to [who]"}]}, {"title": "Timeline", "blocks": [{"content": "[Date/Phase] - [Milestone] - Deliverables"}]}, {"title": "Risks", "blocks": [{"content": "🚨 [Risk] - Impact: H/M/L - Mitigation - Owner"}]}, {"title": "Dependencies", "blocks": [{"content": "Depends on [what] - Impact if delayed"}]}, {"title": "Communication", "blocks": [{"content": "Meeting cadence - Attendees - Purpose"}, {"content": "Status reports - Format - Frequency"}]}]},
  "KeyItemsDecisions": {"title": "Decisions", "blocks": [{"content": "Decision on [what] - Rationale - Alternatives"}]},
  "ImmediateActionItems": {"title": "Immediate Actions", "blocks": [{"content": "[Person] will [action] by [date]"}]},
  "CriticalDeadlines": {"title": "Milestones", "blocks": [{"content": "[Date] - [Milestone] - Owner - Dependencies"}]},
  "NextSteps": {"title": "Follow-up", "blocks": [{"content": "Next meeting: [Date] - Agenda"}, {"content": "Docs to create: [What] - Owner - Due"}]}
}

Rules: Clear in/out scope. Explicit roles. Assess risks early. Document decision rationale. Use ✅ in-scope, ❌ out, 🚨 risks. Flag dependencies.
""",
    }

    return templates.get(template_id, templates["standard_meeting"])


# Override prompt set with stricter anti-redundancy and meeting-title quality rules.
def get_template_prompt(template_id: str) -> str:
    global_rules = """
Global rules (apply to every template):
1) Output valid JSON only. No markdown, no prose outside JSON.
2) MeetingName must be specific and human-friendly from content. Never use generic names like "Live Meeting", "Untitled Meeting", "General Discussion", or "Team Sync" unless no context exists.
3) Do not repeat the same point across SessionSummary, KeyItemsDecisions, ImmediateActionItems, NextSteps, and MeetingNotes.
4) Action items must be unique, owner-first, concrete, and include deadline when available.
5) MeetingNotes sections should include discussion detail (context, rationale, risks, open questions), not copy top-level bullets verbatim.
6) If data is missing, return empty blocks [] instead of invented text.
7) Keep output concise and factual.
"""

    templates = {
        "standard_meeting": f"""Generate professional meeting notes as valid JSON:
{{
  "MeetingName": "Specific meeting title from content",
  "People": {{"title": "Participants", "blocks": [{{"content": "Name - Role"}}]}},
  "SessionSummary": {{"title": "Executive Summary", "blocks": [{{"content": "2-4 concise paragraphs"}}]}},
  "KeyItemsDecisions": {{"title": "Key Decisions", "blocks": [{{"content": "Decision - rationale - owner/date"}}]}},
  "ImmediateActionItems": {{"title": "Action Items", "blocks": [{{"content": "[Owner] will [action] by [deadline]"}}]}},
  "NextSteps": {{"title": "Next Steps", "blocks": [{{"content": "What happens next - owner - timeline"}}]}},
  "CriticalDeadlines": {{"title": "Deadlines", "blocks": [{{"content": "[Date] - [Deliverable] - Owner"}}]}},
  "MeetingNotes": {{"meeting_name": "Same as MeetingName", "sections": [{{"title": "Topic", "blocks": [{{"content": "Discussion detail"}}]}}]}}
}}
Template-specific rules:
- Put each decision exactly once in KeyItemsDecisions.
- Put each action exactly once in ImmediateActionItems.
- Do not create MeetingNotes sections named exactly "Participants", "Executive Summary", "Key Decisions", "Action Items", "Next Steps", "Deadlines".
{global_rules}
""",
        "daily_standup": f"""Generate Daily Standup notes as valid JSON:
{{
  "MeetingName": "Team Standup - [Team/Project] - [Date]",
  "People": {{"title": "Team Members", "blocks": [{{"content": "Name - Role"}}]}},
  "SessionSummary": {{"title": "Overview", "blocks": [{{"content": "Progress, blockers, risks"}}]}},
  "MeetingNotes": {{"meeting_name": "Same as MeetingName", "sections": [{{"title": "[Person] Update", "blocks": [{{"content": "✅ Done: ..."}}, {{"content": "🎯 Today: ..."}}, {{"content": "🚧 Blocked: ..."}}]}}]}},
  "KeyItemsDecisions": {{"title": "Decisions", "blocks": [{{"content": "Decision - context"}}]}},
  "ImmediateActionItems": {{"title": "Actions", "blocks": [{{"content": "[Owner] will [action] by [date]"}}]}},
  "CriticalDeadlines": {{"title": "Sprint Deadlines", "blocks": [{{"content": "[Date] - [Milestone]"}}]}},
  "NextSteps": {{"title": "Next Standup", "blocks": [{{"content": "Carry-forward items"}}]}}
}}
Template-specific rules:
- One section per person whenever possible.
- Do not duplicate the same task in both MeetingNotes and ImmediateActionItems.
{global_rules}
""",
        "brainstorming": f"""Generate Brainstorming notes as valid JSON:
{{
  "MeetingName": "Brainstorming - [Problem/Theme]",
  "People": {{"title": "Participants", "blocks": [{{"content": "Name - Expertise"}}]}},
  "SessionSummary": {{"title": "Overview", "blocks": [{{"content": "Problem, method, outcomes"}}]}},
  "MeetingNotes": {{"meeting_name": "Same as MeetingName", "sections": [{{"title": "Ideas - [Theme]", "blocks": [{{"content": "💡 Idea - owner - tradeoff"}}]}}, {{"title": "Parked Ideas", "blocks": [{{"content": "🅿️ Idea - reason"}}]}}]}},
  "KeyItemsDecisions": {{"title": "Selected Ideas", "blocks": [{{"content": "Selected idea - why - owner"}}]}},
  "ImmediateActionItems": {{"title": "Validation Actions", "blocks": [{{"content": "[Owner] will [test] by [date]"}}]}},
  "CriticalDeadlines": {{"title": "Validation Deadlines", "blocks": [{{"content": "[Date] - [Milestone]"}}]}},
  "NextSteps": {{"title": "Follow-up", "blocks": [{{"content": "Next review checkpoint"}}]}}
}}
Template-specific rules:
- Keep selected vs parked ideas separated.
- Avoid repeating identical idea descriptions in multiple sections.
{global_rules}
""",
        "interview": f"""Generate Interview assessment notes as valid JSON:
{{
  "MeetingName": "Interview - [Candidate] - [Role]",
  "People": {{"title": "Interview Panel", "blocks": [{{"content": "Name - Role - Focus"}}]}},
  "SessionSummary": {{"title": "Candidate Overview", "blocks": [{{"content": "Background + overall signal"}}]}},
  "MeetingNotes": {{"meeting_name": "Same as MeetingName", "sections": [{{"title": "Technical", "blocks": [{{"content": "Evidence-based strength/gap"}}]}}, {{"title": "Behavioral", "blocks": [{{"content": "Evidence-based strength/concern"}}]}}, {{"title": "Candidate Questions", "blocks": [{{"content": "Question - signal"}}]}}]}},
  "KeyItemsDecisions": {{"title": "Hiring Recommendation", "blocks": [{{"content": "Recommendation - rationale"}}]}},
  "ImmediateActionItems": {{"title": "Next Steps", "blocks": [{{"content": "[Owner] will [action] by [date]"}}]}},
  "CriticalDeadlines": {{"title": "Timeline", "blocks": [{{"content": "[Date] - decision checkpoint"}}]}},
  "NextSteps": {{"title": "Follow-up", "blocks": [{{"content": "References/next round/offer flow"}}]}}
}}
Template-specific rules:
- Keep recommendation only in KeyItemsDecisions.
- Use concrete evidence; avoid vague impressions.
{global_rules}
""",
        "project_kickoff": f"""Generate Project Kickoff notes as valid JSON:
{{
  "MeetingName": "Project Kickoff - [Project/Initiative]",
  "People": {{"title": "Team & Stakeholders", "blocks": [{{"content": "Name - Role - Responsibility"}}]}},
  "SessionSummary": {{"title": "Project Overview", "blocks": [{{"content": "Goals, success metrics, constraints"}}]}},
  "MeetingNotes": {{"meeting_name": "Same as MeetingName", "sections": [{{"title": "Scope", "blocks": [{{"content": "In-scope / out-of-scope details"}}]}}, {{"title": "Risks & Dependencies", "blocks": [{{"content": "Risk/dependency - impact - mitigation"}}]}}, {{"title": "Execution Plan", "blocks": [{{"content": "Phases, milestones, owners"}}]}}]}},
  "KeyItemsDecisions": {{"title": "Decisions", "blocks": [{{"content": "Decision - rationale - owner"}}]}},
  "ImmediateActionItems": {{"title": "Immediate Actions", "blocks": [{{"content": "[Owner] will [action] by [date]"}}]}},
  "CriticalDeadlines": {{"title": "Milestones", "blocks": [{{"content": "[Date] - [Milestone] - Owner"}}]}},
  "NextSteps": {{"title": "Follow-up", "blocks": [{{"content": "Next meeting/date/agenda"}}]}}
}}
Template-specific rules:
- Keep scope and responsibilities explicit.
- Do not duplicate milestone/action text across sections.
{global_rules}
""",
    }

    return templates.get(template_id, templates["standard_meeting"])


def get_template_structure(template_id: str) -> dict:
    """
    Returns the base structure for each template type.
    This allows for template-specific output structures.
    """
    base_structure = {
        "MeetingName": "",
        "People": {"title": "Participants", "blocks": []},
        "SessionSummary": {"title": "Executive Summary", "blocks": []},
        "KeyItemsDecisions": {"title": "Key Decisions", "blocks": []},
        "ImmediateActionItems": {"title": "Action Items", "blocks": []},
        "NextSteps": {"title": "Next Steps", "blocks": []},
        "CriticalDeadlines": {"title": "Important Deadlines", "blocks": []},
        "MeetingNotes": {"meeting_name": "", "sections": []},
    }

    # Template-specific customizations
    template_structures = {
        "standard_meeting": base_structure,
        "daily_standup": {
            **base_structure,
            "People": {"title": "Team Members Present", "blocks": []},
            "SessionSummary": {"title": "Standup Overview", "blocks": []},
            "CriticalDeadlines": {"title": "Sprint Deadlines", "blocks": []},
        },
        "brainstorming": {
            **base_structure,
            "KeyItemsDecisions": {"title": "Selected Ideas", "blocks": []},
            "ImmediateActionItems": {"title": "Validation Actions", "blocks": []},
        },
        "interview": {
            **base_structure,
            "People": {"title": "Interview Panel", "blocks": []},
            "SessionSummary": {"title": "Candidate Overview", "blocks": []},
            "KeyItemsDecisions": {"title": "Hiring Recommendation", "blocks": []},
        },
        "project_kickoff": {
            **base_structure,
            "People": {"title": "Project Team & Stakeholders", "blocks": []},
            "SessionSummary": {"title": "Project Overview", "blocks": []},
            "CriticalDeadlines": {"title": "Project Milestones", "blocks": []},
        },
    }

    return template_structures.get(template_id, base_structure)


def _normalize_text_for_dedupe(value: str) -> str:
    return " ".join((value or "").strip().lower().split())


def _dedupe_blocks(blocks: list) -> list:
    seen = set()
    result = []
    for block in blocks or []:
        content = (block or {}).get("content", "")
        key = _normalize_text_for_dedupe(content)
        if not key or key in seen:
            continue
        seen.add(key)
        result.append(block)
    return result


def _dedupe_summary_content(summary: dict) -> dict:
    top_level_sections = [
        "People",
        "SessionSummary",
        "KeyItemsDecisions",
        "ImmediateActionItems",
        "NextSteps",
        "CriticalDeadlines",
    ]

    for key in top_level_sections:
        section = summary.get(key, {})
        if isinstance(section, dict):
            section["blocks"] = _dedupe_blocks(section.get("blocks", []))

    notes = summary.get("MeetingNotes", {})
    if isinstance(notes, dict):
        merged_sections = {}
        for section in notes.get("sections", []) or []:
            title = (section or {}).get("title", "").strip()
            if not title:
                continue
            if title not in merged_sections:
                merged_sections[title] = {"title": title, "blocks": []}
            merged_sections[title]["blocks"].extend((section or {}).get("blocks", []))

        notes["sections"] = []
        for title, section in merged_sections.items():
            deduped_blocks = _dedupe_blocks(section.get("blocks", []))
            if deduped_blocks:
                notes["sections"].append({"title": title, "blocks": deduped_blocks})

    return summary


def _build_transcript_text_from_version_content(content: list) -> str:
    lines = []
    for segment in content or []:
        if not isinstance(segment, dict):
            continue
        text = (segment.get("text") or segment.get("transcript") or "").strip()
        if not text:
            continue
        speaker = (segment.get("speaker") or "").strip()
        lines.append(f"[{speaker}]: {text}" if speaker else text)
    return "\n".join(lines).strip()


async def _resolve_notes_transcript(
    meeting_id: str,
    prefer_diarized: bool,
    explicit_transcript: str = "",
) -> Tuple[str, str, bool]:
    """
    Resolve transcript text for notes generation.
    Returns: (transcript_text, source_label, diarized_available)
    """
    explicit = (explicit_transcript or "").strip()
    if explicit:
        versions = await db.get_transcript_versions(meeting_id)
        diarized_available = any(
            ((v.get("source") or "").lower() in DIARIZED_SOURCES) for v in versions
        )
        return explicit, "provided", diarized_available

    versions = await db.get_transcript_versions(meeting_id)
    diarized_version = next(
        (v for v in versions if (v.get("source") or "").lower() in DIARIZED_SOURCES), None
    )
    diarized_available = diarized_version is not None

    if prefer_diarized and diarized_version:
        content = await db.get_transcript_version_content(
            meeting_id, diarized_version["version_num"]
        )
        text = _build_transcript_text_from_version_content(content or [])
        if text:
            return text, "diarized", True

    meeting_data = await db.get_meeting(meeting_id)
    transcripts = (meeting_data or {}).get("transcripts") or []
    live_text = "\n".join([(t.get("text") or "").strip() for t in transcripts]).strip()
    if live_text:
        return live_text, "live", diarized_available

    return "", "missing", diarized_available


async def process_transcript_background(
    process_id: str,
    transcript: TranscriptRequest,
    custom_prompt: str,
    user_email: Optional[str] = None,
):
    """Background task to process transcript"""
    try:
        logger.info(f"Starting background processing for process_id: {process_id}")

        # Early validation for common issues
        if not transcript.text or not transcript.text.strip():
            raise ValueError("Empty transcript text provided")

        # Default to Gemini if no model specified
        transcript.model = transcript.model or "gemini"
        transcript.model_name = transcript.model_name or "gemini-2.5-flash"

        if transcript.model in ["claude", "groq", "openai", "gemini"]:
            # Check if API key is available in DB or Environment
            api_key = await db.get_api_key(transcript.model, user_email=user_email)
            if not api_key:
                import os

                env_keys = {
                    "gemini": ["GOOGLE_API_KEY", "GEMINI_API_KEY"],
                    "groq": ["GROQ_API_KEY"],
                    "openai": ["OPENAI_API_KEY"],
                    "claude": ["ANTHROPIC_API_KEY"],
                }

                if not any(os.getenv(k) for k in env_keys.get(transcript.model, [])):
                    provider_names = {
                        "claude": "Anthropic",
                        "groq": "Groq",
                        "openai": "OpenAI",
                        "gemini": "Gemini",
                    }
                    raise ValueError(
                        f"{provider_names.get(transcript.model, transcript.model)} API key not configured. Please set your API key in the environmental variables or settings."
                    )

        # Use template-specific prompt if templateId is provided
        template_prompt = custom_prompt
        template_id = getattr(transcript, "templateId", None) or getattr(
            transcript, "template_id", None
        )
        if template_id:
            template_prompt = get_template_prompt(template_id)

        _, all_json_data = await processor.process_transcript(
            text=transcript.text,
            model=transcript.model,
            model_name=transcript.model_name,
            chunk_size=transcript.chunk_size,
            overlap=transcript.overlap,
            custom_prompt=template_prompt,
            user_email=user_email,
        )

        # Get template-specific structure
        final_summary = get_template_structure(template_id or "standard_meeting")

        # Process each chunk's data
        for json_str in all_json_data:
            try:
                logger.info(
                    f"Parsing JSON chunk (len={len(json_str)}): {json_str[:200]}..."
                )
                json_dict = json.loads(json_str)
                logger.info(f"Chunk keys: {list(json_dict.keys())}")

                # Update meeting name
                if "MeetingName" in json_dict and json_dict["MeetingName"]:
                    final_summary["MeetingName"] = json_dict["MeetingName"]

                # Process each section
                for key in final_summary:
                    if key == "MeetingName":
                        continue

                    if key == "MeetingNotes" and key in json_dict:
                        # Handle MeetingNotes sections
                        if isinstance(json_dict[key].get("sections"), list):
                            for section in json_dict[key]["sections"]:
                                if not section.get("blocks"):
                                    section["blocks"] = []
                            final_summary[key]["sections"].extend(
                                json_dict[key]["sections"]
                            )
                        if json_dict[key].get("meeting_name"):
                            final_summary[key]["meeting_name"] = json_dict[key][
                                "meeting_name"
                            ]
                    elif (
                        key in json_dict
                        and isinstance(json_dict[key], dict)
                        and "blocks" in json_dict[key]
                    ):
                        if isinstance(json_dict[key]["blocks"], list):
                            final_summary[key]["blocks"].extend(
                                json_dict[key]["blocks"]
                            )
            except json.JSONDecodeError as e:
                logger.error(
                    f"Failed to parse JSON chunk for {process_id}: {e}. Chunk: {json_str[:100]}..."
                )
            except Exception as e:
                logger.error(
                    f"Error processing chunk data for {process_id}: {e}. Chunk: {json_str[:100]}..."
                )

        # Update database with meeting name using meeting_id
        if final_summary["MeetingName"]:
            await db.update_meeting_name(
                transcript.meeting_id, final_summary["MeetingName"]
            )

        final_summary = _dedupe_summary_content(final_summary)

        # Save final result
        if all_json_data:
            await db.update_process(
                process_id, status="completed", result=final_summary
            )
            logger.info(f"Background processing completed for process_id: {process_id}")
        else:
            error_msg = "Summary generation failed: No chunks were processed successfully. Check logs for specific errors."
            await db.update_process(process_id, status="failed", error=error_msg)
            logger.error(
                f"Background processing failed for process_id: {process_id} - {error_msg}"
            )

    except ValueError as e:
        # Handle specific value errors (like API key issues)
        error_msg = str(e)
        logger.error(
            f"Configuration error in background processing for {process_id}: {error_msg}",
            exc_info=True,
        )
        try:
            await db.update_process(process_id, status="failed", error=error_msg)
        except Exception as db_e:
            logger.error(
                f"Failed to update DB status to failed for {process_id}: {db_e}",
                exc_info=True,
            )
    except Exception as e:
        # Handle all other exceptions
        error_msg = f"Processing error: {str(e)}"
        logger.error(
            f"Error in background processing for {process_id}: {error_msg}",
            exc_info=True,
        )
        try:
            await db.update_process(process_id, status="failed", error=error_msg)
        except Exception as db_e:
            logger.error(
                f"Failed to update DB status to failed for {process_id}: {db_e}",
                exc_info=True,
            )


async def generate_notes_with_gemini_background(
    meeting_id: str,
    full_transcript_text: str,
    transcript_source: str,
    template_id: str,
    meeting_title: str,
    custom_context: str,
    user_email: str,
    use_audio_context: bool = True,
    audio_mode: str = "auto",
    audio_url: str = "",
    max_audio_minutes: int = 120,
):
    """
    Background task to generate notes using Gemini.
    """
    template_prompt = get_template_prompt(template_id)
    calendar_event_context = None

    def _clean_calendar_text(value: str) -> str:
        if not value:
            return ""
        no_html = re.sub(r"<[^>]+>", " ", value)
        return re.sub(r"\s+", " ", no_html).strip()

    try:
        calendar_event_context = await db.get_calendar_event_context_for_meeting(
            meeting_id=meeting_id,
            user_email=user_email,
            provider="google",
        )
    except Exception as e:
        logger.warning("Failed to fetch calendar context for %s: %s", meeting_id, e)

    calendar_context_lines = []
    if calendar_event_context:
        agenda_text = _clean_calendar_text(
            calendar_event_context.get("agenda_description") or ""
        )
        attendees = calendar_event_context.get("attendees") or []
        if calendar_event_context.get("meeting_title"):
            calendar_context_lines.append(
                f"- Calendar title: {calendar_event_context['meeting_title']}"
            )
        if calendar_event_context.get("start_time"):
            calendar_context_lines.append(
                f"- Scheduled start (UTC): {calendar_event_context['start_time']}"
            )
        if calendar_event_context.get("meeting_link"):
            calendar_context_lines.append(
                f"- Meeting link: {calendar_event_context['meeting_link']}"
            )
        calendar_context_lines.append(f"- Attendee count: {len(attendees)}")
        if agenda_text:
            calendar_context_lines.append(f"- Agenda/description: {agenda_text}")

    if custom_context:
        template_prompt += f"\n\nAdditional Context:\n{custom_context}"
    if calendar_context_lines:
        template_prompt += "\n\nCalendar Context:\n" + "\n".join(calendar_context_lines)

    def _extract_json_object(text: str) -> str:
        if not text:
            return "{}"
        cleaned = text.strip()
        if cleaned.startswith("```"):
            cleaned = cleaned.replace("```json", "").replace("```", "").strip()
        start = cleaned.find("{")
        end = cleaned.rfind("}")
        if start == -1 or end == -1 or end <= start:
            return cleaned
        return cleaned[start : end + 1]

    async def _maybe_create_compressed_audio(meeting_id_local: str) -> bool:
        """
        Create and upload meeting_id/recording.notes.opus from recording.wav if missing.
        Returns True if compressed artifact is available after this call.
        """
        primary_compressed_path = f"{meeting_id_local}/recording.opus"
        if await StorageService.check_file_exists(primary_compressed_path):
            return True

        compressed_path = f"{meeting_id_local}/recording.notes.opus"
        if await StorageService.check_file_exists(compressed_path):
            return True

        wav_path = f"{meeting_id_local}/recording.wav"
        wav_bytes = await StorageService.download_bytes(wav_path)
        if not wav_bytes:
            return False

        with tempfile.TemporaryDirectory(prefix="notes-audio-") as tmpdir:
            input_wav = Path(tmpdir) / "recording.wav"
            output_opus = Path(tmpdir) / "recording.notes.opus"
            input_wav.write_bytes(wav_bytes)

            cmd = [
                "ffmpeg",
                "-y",
                "-i",
                str(input_wav),
                "-vn",
                "-ac",
                "1",
                "-ar",
                "16000",
                "-c:a",
                "libopus",
                "-b:a",
                "24k",
                str(output_opus),
            ]
            proc = await asyncio.create_subprocess_exec(
                *cmd,
                stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.PIPE,
            )
            _, stderr = await proc.communicate()
            if proc.returncode != 0 or not output_opus.exists():
                logger.warning(
                    "Failed to create compressed notes audio for %s: %s",
                    meeting_id_local,
                    stderr.decode("utf-8", errors="ignore")[:400],
                )
                return False

            opus_bytes = output_opus.read_bytes()
            return await StorageService.upload_bytes(
                opus_bytes,
                compressed_path,
                content_type="audio/ogg",
            )

    async def _resolve_audio_asset(
        meeting_id_local: str,
        mode: str,
        allow_audio: bool,
    ) -> Tuple[Optional[str], Optional[bytes], Optional[str], Optional[int], Optional[str]]:
        """
        Returns (asset_path, audio_bytes, mime_type, duration_sec, selected_mode).
        """
        if not allow_audio:
            return None, None, None, None, "disabled"

        normalized_mode = (mode or "auto").strip().lower()
        if normalized_mode not in {"auto", "compressed", "wav", "transcript_only"}:
            normalized_mode = "auto"
        if normalized_mode == "transcript_only":
            return None, None, None, None, "transcript_only"

        if audio_url.strip():
            # URL overrides storage lookup. We do not pull remote URLs server-side.
            return audio_url.strip(), None, "audio/url", None, "url_override"

        wav_path = f"{meeting_id_local}/recording.wav"
        primary_compressed_path = f"{meeting_id_local}/recording.opus"
        compressed_path = f"{meeting_id_local}/recording.notes.opus"
        ordered_modes = ["compressed", "wav"] if normalized_mode in {"auto", "compressed"} else ["wav"]

        if "compressed" in ordered_modes:
            try:
                await _maybe_create_compressed_audio(meeting_id_local)
            except Exception as e:
                logger.warning("Compressed audio prep failed for %s: %s", meeting_id_local, e)

        selected_path = None
        selected_mode = None
        selected_mime = None

        for candidate in ordered_modes:
            if candidate == "compressed":
                if await StorageService.check_file_exists(primary_compressed_path):
                    selected_path = primary_compressed_path
                    selected_mode = "compressed"
                    selected_mime = "audio/ogg"
                    break
                if await StorageService.check_file_exists(compressed_path):
                    selected_path = compressed_path
                    selected_mode = "compressed"
                    selected_mime = "audio/ogg"
                    break
            if candidate == "wav" and await StorageService.check_file_exists(wav_path):
                selected_path = wav_path
                selected_mode = "wav"
                selected_mime = "audio/wav"
                break

        if not selected_path:
            return None, None, None, None, "missing"

        audio_bytes = await StorageService.download_bytes(selected_path)
        if not audio_bytes:
            return None, None, None, None, "download_failed"

        approx_duration_sec = None
        if selected_mode == "wav":
            # PCM 16kHz mono 16-bit ~= 32000 bytes/s payload (rough estimate)
            approx_duration_sec = int(len(audio_bytes) / 32000)

        return selected_path, audio_bytes, selected_mime, approx_duration_sec, selected_mode

    async def _generate_multimodal_json(
        transcript_text: str,
        prompt_text: str,
        model_name: str,
        user_email_local: str,
        mime_type: str,
        audio_bytes: bytes,
    ) -> Optional[str]:
        api_key = await db.get_api_key("gemini", user_email=user_email_local)
        if not api_key:
            api_key = os.getenv("GEMINI_API_KEY") or os.getenv("GOOGLE_API_KEY")
        if not api_key:
            logger.warning("Gemini API key missing for multimodal notes generation")
            return None

        transcript_limit = int(os.getenv("NOTES_AUDIO_TRANSCRIPT_CHAR_LIMIT", "120000"))
        compact_transcript = transcript_text[:transcript_limit]

        multimodal_prompt = (
            f"{prompt_text}\n\n"
            "Additional instructions:\n"
            "1) Audio is the primary source for decisions, commitments, action items, and meeting intent.\n"
            "2) Transcript is secondary and should mainly assist with speaker mapping and entities (names/dates/terms).\n"
            "3) Never invent facts not supported by transcript or audio.\n"
            "4) Return valid JSON only, matching the required template shape.\n\n"
            f"Transcript:\n{compact_transcript}"
        )

        suffix = ".opus" if mime_type == "audio/ogg" else ".wav"
        with tempfile.NamedTemporaryFile(delete=False, suffix=suffix) as tmp_audio:
            tmp_audio.write(audio_bytes)
            temp_audio_path = tmp_audio.name

        def _sync_generate() -> Optional[str]:
            return generate_content_with_file_sync(
                api_key=api_key,
                model=model_name or "gemini-2.5-flash",
                prompt=multimodal_prompt,
                file_path=temp_audio_path,
                mime_type=mime_type,
                config={"response_mime_type": "application/json"},
            )

        try:
            return await asyncio.to_thread(_sync_generate)
        finally:
            try:
                os.unlink(temp_audio_path)
            except Exception:
                pass

    try:
        # 1. Create process
        process_id = await db.create_process(meeting_id)

        metadata = {
            "audio_used": False,
            "audio_mode_requested": audio_mode or "auto",
            "audio_mode_selected": "transcript_only",
            "audio_source": None,
            "audio_duration_sec": None,
            "fallback_reason": None,
            "notes_transcript_source": transcript_source,
            "notes_agenda_used": bool(calendar_context_lines),
            "notes_prompt_version": "v1",
        }

        all_json_data = []
        model_name = "gemini-2.5-flash"

        # 2. Try multimodal generation first (behind feature flag + request flags)
        notes_audio_enabled = os.getenv("NOTES_AUDIO_ENABLED", "true").lower() == "true"
        allow_audio = bool(use_audio_context) and notes_audio_enabled
        effective_audio_mode = audio_mode or os.getenv("NOTES_AUDIO_DEFAULT_MODE", "auto")

        if allow_audio:
            try:
                audio_source, audio_bytes, audio_mime, audio_duration_sec, selected_mode = await _resolve_audio_asset(
                    meeting_id,
                    effective_audio_mode,
                    allow_audio=True,
                )
                metadata["audio_mode_selected"] = selected_mode
                metadata["audio_source"] = audio_source
                metadata["audio_duration_sec"] = audio_duration_sec

                if audio_source and audio_mime == "audio/url":
                    metadata["fallback_reason"] = "audio_url_override_not_supported_server_side"
                elif audio_bytes and audio_mime:
                    max_minutes = max(1, int(max_audio_minutes or 120))
                    if audio_duration_sec and (audio_duration_sec / 60) > max_minutes:
                        metadata["fallback_reason"] = f"audio_exceeds_max_minutes_{max_minutes}"
                    else:
                        multimodal_json = await _generate_multimodal_json(
                            transcript_text=full_transcript_text,
                            prompt_text=template_prompt,
                            model_name=model_name,
                            user_email_local=user_email,
                            mime_type=audio_mime,
                            audio_bytes=audio_bytes,
                        )
                        if multimodal_json:
                            all_json_data = [_extract_json_object(multimodal_json)]
                            metadata["audio_used"] = True
                        else:
                            metadata["fallback_reason"] = "multimodal_generation_failed"
                else:
                    metadata["fallback_reason"] = "audio_asset_unavailable"
            except Exception as e:
                logger.warning("Multimodal notes path failed for %s: %s", meeting_id, e)
                metadata["fallback_reason"] = "multimodal_exception"

        # 3. Transcript-only fallback
        if not all_json_data:
            _, all_json_data = await processor.process_transcript(
                text=full_transcript_text,
                model="gemini",
                model_name=model_name,
                chunk_size=10000,  # larger chunks for notes
                overlap=1000,
                custom_prompt=template_prompt,
                user_email=user_email,
            )

        # 4. Get template-specific structure
        final_result = get_template_structure(template_id)
        final_result["MeetingName"] = ""
        final_result["MeetingNotes"]["meeting_name"] = ""

        # 5. Aggregate results from all chunks
        for json_str in all_json_data:
            try:
                json_dict = json.loads(json_str)

                # Merge logic consistent with process_transcript_background
                for key in final_result:
                    if key == "MeetingName":
                        if json_dict.get("MeetingName"):
                            final_result["MeetingName"] = json_dict["MeetingName"]
                        continue

                    if key == "MeetingNotes" and key in json_dict:
                        if isinstance(json_dict[key].get("sections"), list):
                            for new_section in json_dict[key]["sections"]:
                                # Skip empty sections
                                if not new_section.get("blocks"):
                                    continue

                                # Check if section title already exists
                                existing_section = next(
                                    (
                                        s
                                        for s in final_result[key]["sections"]
                                        if s["title"] == new_section["title"]
                                    ),
                                    None,
                                )

                                if existing_section:
                                    # Merge blocks
                                    existing_section["blocks"].extend(
                                        new_section["blocks"]
                                    )
                                else:
                                    # Append new section
                                    final_result[key]["sections"].append(new_section)

                    elif (
                        key in json_dict
                        and isinstance(json_dict[key], dict)
                        and "blocks" in json_dict[key]
                    ):
                        if (
                            isinstance(json_dict[key]["blocks"], list)
                            and json_dict[key]["blocks"]
                        ):
                            final_result[key]["blocks"].extend(json_dict[key]["blocks"])
            except Exception as e:
                logger.error(f"Error merging chunk: {e}")

        if not final_result["MeetingName"]:
            final_result["MeetingName"] = meeting_title
        if not final_result["MeetingNotes"]["meeting_name"]:
            final_result["MeetingNotes"]["meeting_name"] = final_result["MeetingName"]

        final_result = _dedupe_summary_content(final_result)

        # 6. Convert final_result to Markdown
        markdown_output = generate_markdown_from_structure(final_result, template_id)
        final_result["markdown"] = markdown_output

        await db.update_process(
            process_id,
            status="completed",
            result=final_result,
            metadata=metadata,
        )

        # 7. Calendar post-processing hooks: recap email + optional writeback.
        try:
            if calendar_event_context:
                settings = await db.get_calendar_automation_settings(user_email)
                attendees = calendar_event_context.get("attendees") or []
                is_proper_meeting = len(attendees) > 1

                if settings.get("recap_enabled", True) and is_proper_meeting:
                    recap_service = CalendarReminderEmailService()
                    await recap_service.send_post_meeting_recap(
                        host_email=user_email,
                        meeting_title=final_result.get("MeetingName", meeting_title),
                        notes_markdown=markdown_output,
                        meeting_link=calendar_event_context.get("meeting_link"),
                        attendees=attendees,
                        include_attendees=bool(
                            settings.get("attendee_reminders_enabled", False)
                        ),
                    )

                if settings.get("writeback_enabled", False):
                    oauth_service = GoogleCalendarOAuthService(db=db)
                    await oauth_service.writeback_notes_to_event(
                        user_email=user_email,
                        event_id=calendar_event_context["event_id"],
                        notes_markdown=markdown_output,
                    )
        except Exception as post_hook_error:
            logger.error(
                "Calendar post-processing failed for %s: %s",
                meeting_id,
                post_hook_error,
            )

    except Exception as e:
        logger.error(f"Failed to generate notes: {e}")
        # Update process to failed
        try:
            await db.update_process(
                meeting_id,
                status="failed",
                error=str(e),
                metadata={
                    "audio_used": False,
                    "audio_mode_selected": "failed",
                },
            )
        except:
            pass


def generate_markdown_from_structure(data: dict, template_id: str) -> str:
    """
    Generate professional markdown notes from the structured data.
    Format varies based on template type.
    """
    markdown = f"# {data.get('MeetingName', 'Meeting Notes')}\n\n"

    # Add metadata
    markdown += "---\n\n"

    # Executive Summary / Overview
    if data.get("SessionSummary", {}).get("blocks"):
        markdown += f"## {data['SessionSummary']['title']}\n\n"
        for block in data["SessionSummary"]["blocks"]:
            markdown += f"{block['content']}\n\n"

    # Participants
    if data.get("People", {}).get("blocks"):
        markdown += f"## {data['People']['title']}\n\n"
        for block in data["People"]["blocks"]:
            markdown += f"- {block['content']}\n"
        markdown += "\n"

    # Template-specific sections
    if template_id == "daily_standup":
        markdown += generate_standup_markdown(data)
    elif template_id == "brainstorming":
        markdown += generate_brainstorming_markdown(data)
    elif template_id == "interview":
        markdown += generate_interview_markdown(data)
    elif template_id == "project_kickoff":
        markdown += generate_project_kickoff_markdown(data)
    else:
        markdown += generate_standard_markdown(data)

    return markdown


def generate_standard_markdown(data: dict) -> str:
    """Generate markdown for standard meetings"""
    md = ""

    # Key Discussion Points
    if data.get("MeetingNotes", {}).get("sections"):
        md += "## Key Discussion Points\n\n"
        for section in data["MeetingNotes"]["sections"]:
            if not section.get("blocks"):
                continue
            md += f"### {section['title']}\n\n"
            for block in section["blocks"]:
                md += f"- {block['content']}\n"
            md += "\n"

    # Decisions
    if data.get("KeyItemsDecisions", {}).get("blocks"):
        md += f"## {data['KeyItemsDecisions']['title']}\n\n"
        for block in data["KeyItemsDecisions"]["blocks"]:
            md += f"- {block['content']}\n"
        md += "\n"

    # Action Items
    if data.get("ImmediateActionItems", {}).get("blocks"):
        md += f"## {data['ImmediateActionItems']['title']}\n\n"
        for block in data["ImmediateActionItems"]["blocks"]:
            md += f"- [ ] {block['content']}\n"
        md += "\n"

    # Deadlines
    if data.get("CriticalDeadlines", {}).get("blocks"):
        md += f"## {data['CriticalDeadlines']['title']}\n\n"
        for block in data["CriticalDeadlines"]["blocks"]:
            md += f"- {block['content']}\n"
        md += "\n"

    # Next Steps
    if data.get("NextSteps", {}).get("blocks"):
        md += f"## {data['NextSteps']['title']}\n\n"
        for block in data["NextSteps"]["blocks"]:
            md += f"- {block['content']}\n"
        md += "\n"

    return md


def generate_standup_markdown(data: dict) -> str:
    """Generate markdown for daily standups"""
    md = ""

    # Individual updates
    if data.get("MeetingNotes", {}).get("sections"):
        md += "## Team Updates\n\n"
        for section in data["MeetingNotes"]["sections"]:
            if not section.get("blocks"):
                continue
            md += f"### {section['title']}\n\n"
            for block in section["blocks"]:
                md += f"{block['content']}\n"
            md += "\n"

    # Actions and deadlines
    if data.get("ImmediateActionItems", {}).get("blocks"):
        md += f"## {data['ImmediateActionItems']['title']}\n\n"
        for block in data["ImmediateActionItems"]["blocks"]:
            md += f"- [ ] {block['content']}\n"
        md += "\n"

    if data.get("CriticalDeadlines", {}).get("blocks"):
        md += f"## {data['CriticalDeadlines']['title']}\n\n"
        for block in data["CriticalDeadlines"]["blocks"]:
            md += f"- {block['content']}\n"
        md += "\n"

    return md


def generate_brainstorming_markdown(data: dict) -> str:
    """Generate markdown for brainstorming sessions"""
    md = ""

    # Ideas organized by theme
    if data.get("MeetingNotes", {}).get("sections"):
        md += "## Ideas Generated\n\n"
        for section in data["MeetingNotes"]["sections"]:
            if not section.get("blocks"):
                continue
            md += f"### {section['title']}\n\n"
            for block in section["blocks"]:
                md += f"{block['content']}\n"
            md += "\n"

    # Selected ideas and next steps
    if data.get("KeyItemsDecisions", {}).get("blocks"):
        md += f"## {data['KeyItemsDecisions']['title']}\n\n"
        for block in data["KeyItemsDecisions"]["blocks"]:
            md += f"- {block['content']}\n"
        md += "\n"

    if data.get("ImmediateActionItems", {}).get("blocks"):
        md += f"## {data['ImmediateActionItems']['title']}\n\n"
        for block in data["ImmediateActionItems"]["blocks"]:
            md += f"- [ ] {block['content']}\n"
        md += "\n"

    return md


def generate_interview_markdown(data: dict) -> str:
    """Generate markdown for interviews"""
    md = ""

    # Assessment sections
    if data.get("MeetingNotes", {}).get("sections"):
        md += "## Interview Assessment\n\n"
        for section in data["MeetingNotes"]["sections"]:
            if not section.get("blocks"):
                continue
            md += f"### {section['title']}\n\n"
            for block in section["blocks"]:
                md += f"{block['content']}\n"
            md += "\n"

    # Recommendation
    if data.get("KeyItemsDecisions", {}).get("blocks"):
        md += f"## {data['KeyItemsDecisions']['title']}\n\n"
        for block in data["KeyItemsDecisions"]["blocks"]:
            md += f"**{block['content']}**\n\n"

    # Next steps
    if data.get("ImmediateActionItems", {}).get("blocks"):
        md += f"## {data['ImmediateActionItems']['title']}\n\n"
        for block in data["ImmediateActionItems"]["blocks"]:
            md += f"- [ ] {block['content']}\n"
        md += "\n"

    return md


def generate_project_kickoff_markdown(data: dict) -> str:
    """Generate markdown for project kickoffs"""
    md = ""

    # Project details
    if data.get("MeetingNotes", {}).get("sections"):
        for section in data["MeetingNotes"]["sections"]:
            if not section.get("blocks"):
                continue
            md += f"## {section['title']}\n\n"
            for block in section["blocks"]:
                md += f"{block['content']}\n"
            md += "\n"

    # Decisions, actions, milestones
    if data.get("KeyItemsDecisions", {}).get("blocks"):
        md += f"## {data['KeyItemsDecisions']['title']}\n\n"
        for block in data["KeyItemsDecisions"]["blocks"]:
            md += f"- {block['content']}\n"
        md += "\n"

    if data.get("ImmediateActionItems", {}).get("blocks"):
        md += f"## {data['ImmediateActionItems']['title']}\n\n"
        for block in data["ImmediateActionItems"]["blocks"]:
            md += f"- [ ] {block['content']}\n"
        md += "\n"

    if data.get("CriticalDeadlines", {}).get("blocks"):
        md += f"## {data['CriticalDeadlines']['title']}\n\n"
        for block in data["CriticalDeadlines"]["blocks"]:
            md += f"- {block['content']}\n"
        md += "\n"

    return md


# --- API Endpoints (rest remains the same) ---


@router.get("/meetings/{meeting_id}/versions")
async def get_transcript_versions(
    meeting_id: str, current_user: User = Depends(get_current_user)
):
    """Get all transcript versions for a meeting."""
    if not await rbac.can(current_user, "view", meeting_id):
        raise HTTPException(status_code=403, detail="Permission denied")

    try:
        versions = await db.get_transcript_versions(meeting_id)
        return {"meeting_id": meeting_id, "versions": versions}
    except Exception as e:
        logger.error(f"Error getting transcript versions: {str(e)}", exc_info=True)
        raise HTTPException(status_code=500, detail=str(e))


@router.get("/meetings/{meeting_id}/versions/{version_num}")
async def get_transcript_version_content(
    meeting_id: str, version_num: int, current_user: User = Depends(get_current_user)
):
    """Get the content of a specific transcript version."""
    if not await rbac.can(current_user, "view", meeting_id):
        raise HTTPException(status_code=403, detail="Permission denied")

    try:
        content = await db.get_transcript_version_content(meeting_id, version_num)
        if content is None:
            raise HTTPException(status_code=404, detail="Version not found")
        return {
            "meeting_id": meeting_id,
            "version_num": version_num,
            "content": content,
        }
    except HTTPException:
        raise
    except Exception as e:
        logger.error(
            f"Error getting transcript version content: {str(e)}", exc_info=True
        )
        raise HTTPException(status_code=500, detail=str(e))


@router.delete("/meetings/{meeting_id}/versions/{version_num}")
async def delete_transcript_version(
    meeting_id: str, version_num: int, current_user: User = Depends(get_current_user)
):
    """Delete a specific transcript version snapshot."""
    if not await rbac.can(current_user, "edit", meeting_id):
        raise HTTPException(status_code=403, detail="Permission denied")

    try:
        success = await db.delete_transcript_version(meeting_id, version_num)
        if not success:
            raise HTTPException(status_code=404, detail="Version not found")
        return {"message": f"Version {version_num} deleted successfully"}
    except HTTPException:
        raise
    except Exception as e:
        logger.error(f"Error deleting transcript version: {str(e)}", exc_info=True)
        raise HTTPException(status_code=500, detail=str(e))


@router.post("/process-transcript")
async def process_transcript_api(
    transcript: TranscriptRequest,
    background_tasks: BackgroundTasks,
    current_user: User = Depends(get_current_user),
):
    """Process a transcript text with background processing"""
    try:
        # 0. Ensure meeting exists and check permissions
        meeting = await db.get_meeting(transcript.meeting_id)
        if not meeting:
            # New Meeting: Claim Ownership
            await db.save_meeting(
                meeting_id=transcript.meeting_id,
                title="Untitled Meeting",
                owner_id=current_user.email,
                workspace_id=None,
            )
            logger.info(
                f"Created new meeting {transcript.meeting_id} for owner {current_user.email}"
            )
        else:
            # Existing Meeting: Check Edit Permission
            if not await rbac.can(current_user, "edit", transcript.meeting_id):
                raise HTTPException(
                    status_code=403, detail="Permission denied to edit this meeting"
                )

        # Create new process linked to meeting_id
        process_id = await db.create_process(transcript.meeting_id)

        # Save transcript data associated with meeting_id
        await db.save_transcript(
            transcript.meeting_id,
            transcript.text,
            transcript.model,
            transcript.model_name,
            transcript.chunk_size,
            transcript.overlap,
        )

        # Use template-specific prompt if templateId is provided, otherwise use custom_prompt
        custom_prompt = transcript.custom_prompt
        if (
            hasattr(transcript, "templateId")
            and transcript.templateId
            and not custom_prompt
        ):
            custom_prompt = get_template_prompt(transcript.templateId)

        # Start background processing
        background_tasks.add_task(
            process_transcript_background,
            process_id,
            transcript,
            custom_prompt,
            current_user.email,
        )

        return JSONResponse({"message": "Processing started", "process_id": process_id})

    except Exception as e:
        logger.error(f"Error in process_transcript_api: {str(e)}", exc_info=True)
        raise HTTPException(status_code=500, detail=str(e))


@router.post("/save-summary")
async def save_summary(
    data: SaveSummaryRequest, current_user: User = Depends(get_current_user)
):
    """Save or update meeting summary/notes"""
    if not await rbac.can(current_user, "edit", data.meeting_id):
        raise HTTPException(status_code=403, detail="Permission denied to edit summary")

    try:
        logger.info(f"Saving summary for meeting {data.meeting_id}")

        # Update the summary_processes table with the new content
        await db.update_process(
            meeting_id=data.meeting_id, status="completed", result=data.summary
        )

        logger.info(f"Successfully saved summary for meeting {data.meeting_id}")
        return {"message": "Summary saved successfully"}
    except Exception as e:
        logger.error(f"Error saving summary: {str(e)}", exc_info=True)
        raise HTTPException(status_code=500, detail=str(e))


@router.post("/save-transcript")
async def save_transcript(
    data: SaveTranscriptRequest,
    background_tasks: BackgroundTasks,
    current_user: User = Depends(get_current_user),
):
    """Save transcript segments manually (from frontend)"""
    meeting_id = data.meeting_id or data.session_id or str(uuid.uuid4())
    source_session_id = data.session_id if data.session_id else None

    try:
        # Check if meeting exists
        meeting = await db.get_meeting(meeting_id)
        if meeting:
            if not await rbac.can(current_user, "edit", meeting_id):
                raise HTTPException(status_code=403, detail="Permission denied")
        else:
            # Create new meeting
            await db.save_meeting(
                meeting_id=meeting_id,
                title=data.meeting_title,
                owner_id=current_user.email,
            )

        # Consolidate transcripts from source session meeting into canonical meeting_id.
        if source_session_id and source_session_id != meeting_id:
            try:
                async with db._get_connection() as conn:
                    await conn.execute(
                        """
                        UPDATE transcript_segments
                        SET meeting_id = $1
                        WHERE meeting_id = $2
                    """,
                        meeting_id,
                        source_session_id,
                    )
                    await conn.execute(
                        """
                        DELETE FROM meetings
                        WHERE id = $1
                    """,
                        source_session_id,
                    )
                logger.info(
                    f"✅ Consolidated source session meeting {source_session_id} into {meeting_id}"
                )
            except Exception as merge_error:
                logger.warning(
                    f"Could not consolidate meeting {source_session_id} into {meeting_id}: {merge_error}"
                )

        # Save segments (batch)
        await db.save_meeting_transcripts_batch(meeting_id, data.transcripts)

        # CRITICAL FIX: Ensure audio folder matches meeting_id
        # The recorder might have stored files under session_id. We must rename it to meeting_id
        # so that finalize_recording and diarization can find it.
        if data.session_id and data.session_id != meeting_id:
            try:
                try:
                    from ...services.audio.recorder import AudioRecorder
                except (ImportError, ValueError):
                    from services.audio.recorder import AudioRecorder

                renamed = await AudioRecorder.rename_recorder_folder(
                    data.session_id, meeting_id
                )
                if renamed:
                    logger.info(
                        f"✅ Renamed recording folder from {data.session_id} to {meeting_id}"
                    )
                else:
                    logger.warning(
                        f"Could not rename folder from {data.session_id} to {meeting_id} (might not exist or empty)"
                    )
            except Exception as rename_error:
                logger.error(f"Failed to rename recording folder: {rename_error}")

        # Trigger post-recording processing (merge, upload to GCP, cleanup) in background
        try:
            # Avoid premature finalize while WS session is still active.
            # In celery/session-pipeline mode, recording finalization is handled by the
            # audio pipeline state machine after stop/resume-grace.
            skip_post_finalize = False
            if data.session_id:
                session = await db.get_recording_session(data.session_id)
                if session and session.get("status") in {
                    "recording",
                    "stopping_requested",
                    "uploading_chunks",
                    "finalizing",
                    "postprocessing",
                }:
                    skip_post_finalize = True
                    logger.info(
                        "Skipping save-transcript finalize for active session %s (status=%s)",
                        data.session_id,
                        session.get("status"),
                    )

            if skip_post_finalize:
                return {"message": "Transcript saved successfully", "meeting_id": meeting_id}

            try:
                from ...services.audio.post_recording import get_post_recording_service
            except (ImportError, ValueError):
                from services.audio.post_recording import get_post_recording_service

            post_service = get_post_recording_service()
            background_tasks.add_task(
                post_service.finalize_recording,
                meeting_id,
                trigger_diarization=False,
                user_email=current_user.email,
            )
            logger.info(f"Scheduled post-recording processing for meeting {meeting_id}")
        except Exception as post_e:
            logger.warning(f"Post-recording service unavailable: {post_e}")

        return {"message": "Transcript saved successfully", "meeting_id": meeting_id}
    except Exception as e:
        logger.error(f"Error saving transcript: {e}")
        raise HTTPException(status_code=500, detail=str(e))


@router.get("/get-summary/{meeting_id}")
async def get_summary(meeting_id: str, current_user: User = Depends(get_current_user)):
    """Get the summary for a given meeting ID"""
    if not await rbac.can(current_user, "view", meeting_id):
        raise HTTPException(status_code=403, detail="Access denied")

    try:
        result = await db.get_transcript_data(meeting_id)
        if not result:
            return JSONResponse(
                status_code=404,
                content={
                    "status": "error",
                    "meetingName": None,
                    "meeting_id": meeting_id,
                    "data": None,
                    "start": None,
                    "end": None,
                    "error": "Meeting ID not found",
                },
            )

        status = result.get("status", "unknown").lower()

        versions = await db.get_transcript_versions(meeting_id)
        diarized_available = any(
            ((v.get("source") or "").lower() in DIARIZED_SOURCES) for v in versions
        )

        # Parse result data if available
        summary_data = None
        if result.get("result"):
            try:
                if isinstance(result["result"], dict):
                    summary_data = result["result"]
                else:
                    parsed_result = json.loads(result["result"])
                    if isinstance(parsed_result, str):
                        summary_data = json.loads(parsed_result)
                    else:
                        summary_data = parsed_result
            except Exception as e:
                logger.error(f"Error parsing summary data: {e}")
                status = "failed"

        # Transform summary data into frontend format if available
        transformed_data = {}
        metadata = result.get("metadata") if isinstance(result.get("metadata"), dict) else {}
        notes_transcript_source = metadata.get("notes_transcript_source")
        recommend_regenerate_with_diarized = bool(
            diarized_available and notes_transcript_source and notes_transcript_source != "diarized"
        )
        if isinstance(summary_data, dict) and status == "completed":
            transformed_data["MeetingName"] = summary_data.get("MeetingName", "")
            if "markdown" in summary_data:
                transformed_data["markdown"] = summary_data["markdown"]

            # Map backend sections
            section_mapping = {}
            for backend_key, frontend_key in section_mapping.items():
                if backend_key in summary_data:
                    transformed_data[frontend_key] = summary_data[backend_key]

            if "MeetingNotes" in summary_data:
                transformed_data["MeetingNotes"] = summary_data["MeetingNotes"]

        return JSONResponse(
            content={
                "status": status,
                "meetingName": transformed_data.get("MeetingName"),
                "meeting_id": meeting_id,
                "data": transformed_data,
                "start": result.get("start_time").isoformat()
                if result.get("start_time")
                else None,
                "end": result.get("end_time").isoformat()
                if result.get("end_time")
                else None,
                "notes_generation": {
                    "transcript_source": notes_transcript_source,
                    "audio_used": metadata.get("audio_used"),
                    "agenda_used": metadata.get("notes_agenda_used"),
                    "prompt_version": metadata.get("notes_prompt_version"),
                    "diarized_available": diarized_available,
                    "recommend_regenerate_with_diarized": recommend_regenerate_with_diarized,
                },
            }
        )

    except Exception as e:
        logger.error(f"Error getting summary: {str(e)}", exc_info=True)
        raise HTTPException(status_code=500, detail=str(e))


@router.post("/generate-detailed-notes")
async def generate_detailed_notes(
    request: GenerateNotesRequest,
    background_tasks: BackgroundTasks,
    current_user: User = Depends(get_current_user),
):
    """Generates detailed meeting notes using Gemini."""
    try:
        if not await rbac.can(current_user, "ai_interact", request.meeting_id):
            raise HTTPException(
                status_code=403, detail="Permission denied to generate notes"
            )

        logger.info(
            f"Generating detailed notes for meeting {request.meeting_id} using template {request.template_id}"
        )

        full_transcript_text, transcript_source, _ = await _resolve_notes_transcript(
            meeting_id=request.meeting_id,
            prefer_diarized=request.prefer_diarized_transcript,
            explicit_transcript=request.transcript,
        )

        if not full_transcript_text.strip():
            raise HTTPException(status_code=400, detail="Transcript text is empty.")

        meeting_data = await db.get_meeting(request.meeting_id)
        if not meeting_data:
            raise HTTPException(status_code=404, detail="Meeting not found.")
        meeting_title = meeting_data.get("title", "Untitled Meeting")

        # 2. Start background processing (non-blocking)
        background_tasks.add_task(
            generate_notes_with_gemini_background,
            request.meeting_id,
            full_transcript_text,
            transcript_source,
            request.template_id,
            meeting_title,
            "",
            current_user.email,
            request.use_audio_context,
            request.audio_mode,
            request.audio_url,
            request.max_audio_minutes,
        )

        return JSONResponse(
            content={
                "message": "Notes generation started",
                "meeting_id": request.meeting_id,
                "template_id": request.template_id,
                "status": "processing",
            }
        )

    except HTTPException as e:
        raise e
    except Exception as e:
        logger.error(f"Error starting notes generation: {str(e)}", exc_info=True)
        raise HTTPException(status_code=500, detail=str(e))


@router.post("/meetings/{meeting_id}/generate-notes")
async def generate_notes_for_meeting(
    meeting_id: str,
    request: GenerateNotesRequest = None,
    background_tasks: BackgroundTasks = None,
    current_user: User = Depends(get_current_user),
):
    """Generate meeting notes for a specific meeting using the selected template."""
    if not await rbac.can(current_user, "ai_interact", meeting_id):
        raise HTTPException(
            status_code=403, detail="Permission denied to generate notes"
        )

    try:
        actual_meeting_id = meeting_id
        template_id = "standard_meeting"
        custom_context = ""
        use_audio_context = os.getenv("NOTES_AUDIO_ENABLED", "true").lower() == "true"
        audio_mode = os.getenv("NOTES_AUDIO_DEFAULT_MODE", "auto")
        audio_url = ""
        max_audio_minutes = 120

        if request:
            template_id = request.template_id or "standard_meeting"
            custom_context = request.custom_context or ""
            use_audio_context = request.use_audio_context
            audio_mode = request.audio_mode or audio_mode
            audio_url = request.audio_url or ""
            max_audio_minutes = request.max_audio_minutes or max_audio_minutes
            prefer_diarized_transcript = request.prefer_diarized_transcript
            explicit_transcript = request.transcript or ""
        else:
            prefer_diarized_transcript = True
            explicit_transcript = ""

        logger.info(
            f"Generating notes for meeting {actual_meeting_id} using template {template_id}"
        )

        # 1. Resolve transcript source (prefer diarized when available)
        full_transcript_text, transcript_source, _ = await _resolve_notes_transcript(
            meeting_id=actual_meeting_id,
            prefer_diarized=prefer_diarized_transcript,
            explicit_transcript=explicit_transcript,
        )

        if not full_transcript_text.strip():
            raise HTTPException(status_code=400, detail="Transcript text is empty.")

        meeting_data = await db.get_meeting(actual_meeting_id)
        if not meeting_data:
            raise HTTPException(status_code=404, detail="Meeting not found.")
        meeting_title = meeting_data.get("title", "Untitled Meeting")

        # 2. Start background processing (non-blocking)
        background_tasks.add_task(
            generate_notes_with_gemini_background,
            actual_meeting_id,
            full_transcript_text,
            transcript_source,
            template_id,
            meeting_title,
            custom_context,
            current_user.email,
            use_audio_context,
            audio_mode,
            audio_url,
            max_audio_minutes,
        )

        return JSONResponse(
            content={
                "message": "Notes generation started",
                "meeting_id": actual_meeting_id,
                "template_id": template_id,
                "status": "processing",
            }
        )

    except HTTPException as e:
        raise e
    except Exception as e:
        logger.error(
            f"Error starting notes generation for meeting {meeting_id}: {str(e)}",
            exc_info=True,
        )
        raise HTTPException(status_code=500, detail=str(e))


@router.post("/refine-notes")
async def refine_notes(
    request: RefineNotesRequest, current_user: User = Depends(get_current_user)
):
    """
    Refine existing meeting notes based on user instructions and transcript context.
    Streams the refined notes back.
    """
    if not await rbac.can(current_user, "ai_interact", request.meeting_id):
        raise HTTPException(status_code=403, detail="Permission denied to refine notes")

    try:
        logger.info(
            f"Refining notes for meeting {request.meeting_id} with instruction: {request.user_instruction[:50]}..."
        )

        # 1. Fetch meeting transcripts for context
        meeting_data = await db.get_meeting(request.meeting_id)
        full_transcript = ""
        if meeting_data and meeting_data.get("transcripts"):
            full_transcript = "\n".join(
                [t["text"] for t in meeting_data["transcripts"]]
            )

        # 2. Construct Prompt
        refine_prompt = f"""You are an expert meeting notes editor.
Your task is to REFINE the Current Meeting Notes based strictly on the User Instruction and the provided Context (Transcript).

Context (Meeting Transcript):
---
{full_transcript[:30000]} {(len(full_transcript) > 30000) and "...(truncated)" or ""}
---

Current Meeting Notes:
---
{request.current_notes}
---

User Instruction: {request.user_instruction}

Guidelines:
1. You MUST start your response with a detailed bulleted list of changes made.
2. You MUST then output exactly: "|||SEPARATOR|||" (without quotes).
3. After the separator, provide the FULL updated notes content.
"""

        chat_service = ChatService(db)

        generator = await chat_service.refine_notes(
            notes=request.current_notes,
            instruction=request.user_instruction,
            transcript_context=full_transcript,
            model=request.model,
            model_name=request.model_name,
            user_email=current_user.email,
        )

        return StreamingResponse(generator, media_type="text/plain")

    except Exception as e:
        logger.error(f"Error in refine_notes: {str(e)}", exc_info=True)
        raise HTTPException(status_code=500, detail=str(e))
