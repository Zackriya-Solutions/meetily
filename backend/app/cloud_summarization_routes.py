"""
Cloud summarization routes — generate summaries from cloud-stored transcripts.
Reuses the existing TranscriptProcessor for LLM summarization.
"""

import logging
import json
from datetime import datetime, timezone
from typing import Optional

from fastapi import APIRouter, HTTPException, Depends, BackgroundTasks
from pydantic import BaseModel
from auth_middleware import get_current_user
from transcript_processor import TranscriptProcessor
from mongodb import get_meetings_collection

logger = logging.getLogger(__name__)

router = APIRouter(prefix="/api/summarize", tags=["summarize-cloud"])

# Shared processor instance
_processor: TranscriptProcessor | None = None


def _get_processor() -> TranscriptProcessor:
    global _processor
    if _processor is None:
        _processor = TranscriptProcessor()
    return _processor


class SummarizeRequest(BaseModel):
    meeting_id: str
    provider: str = "claude"  # claude, groq, openai
    model: Optional[str] = None
    custom_prompt: Optional[str] = None


async def _process_summary(
    meeting_id: str,
    user_id: str,
    transcript_text: str,
    provider: str,
    model_name: str,
    custom_prompt: str,
):
    """Background task: generate summary and update meeting."""
    meetings_col = get_meetings_collection()

    try:
        await meetings_col.update_one(
            {"meeting_id": meeting_id},
            {"$set": {"status": "summarizing"}},
        )

        processor = _get_processor()
        num_chunks, all_json_data = await processor.process_transcript(
            text=transcript_text,
            model=provider,
            model_name=model_name,
            chunk_size=5000,
            overlap=1000,
            custom_prompt=custom_prompt,
        )

        # Aggregate chunks into final summary (same logic as main.py)
        final_summary = {
            "MeetingName": "",
            "MeetingNotes": {"meeting_name": "", "sections": []},
        }

        for json_str in all_json_data:
            try:
                json_dict = json.loads(json_str)
                if "MeetingName" in json_dict and json_dict["MeetingName"]:
                    final_summary["MeetingName"] = json_dict["MeetingName"]

                if "MeetingNotes" in json_dict and isinstance(json_dict["MeetingNotes"], dict):
                    meeting_notes = json_dict["MeetingNotes"]
                    if isinstance(meeting_notes.get("sections"), list):
                        for section in meeting_notes["sections"]:
                            if not section.get("blocks"):
                                section["blocks"] = []
                        final_summary["MeetingNotes"]["sections"].extend(
                            meeting_notes["sections"]
                        )
                    if meeting_notes.get("meeting_name"):
                        final_summary["MeetingNotes"]["meeting_name"] = meeting_notes["meeting_name"]

                # Also process top-level sections
                for key, value in json_dict.items():
                    if key in ("MeetingName", "MeetingNotes"):
                        continue
                    if isinstance(value, dict) and "blocks" in value and "title" in value:
                        section_exists = False
                        for section in final_summary["MeetingNotes"]["sections"]:
                            if section["title"] == value["title"]:
                                section["blocks"].extend(value.get("blocks", []))
                                section_exists = True
                                break
                        if not section_exists:
                            final_summary["MeetingNotes"]["sections"].append({
                                "title": value["title"],
                                "blocks": list(value.get("blocks", [])),
                            })

            except json.JSONDecodeError as e:
                logger.error(f"Failed to parse summary chunk: {e}")

        # Transform to frontend format with _section_order
        transformed = {"MeetingName": final_summary.get("MeetingName", "")}
        section_order = []
        used_keys = set()

        for i, section in enumerate(final_summary["MeetingNotes"].get("sections", [])):
            if isinstance(section, dict) and "title" in section:
                base_key = section["title"].lower().replace(" & ", "_").replace(" ", "_")
                key = base_key
                if key in used_keys:
                    key = f"{base_key}_{i}"
                used_keys.add(key)
                transformed[key] = section
                section_order.append(key)

        transformed["_section_order"] = section_order

        # Save to meeting
        now = datetime.now(timezone.utc).isoformat()
        await meetings_col.update_one(
            {"meeting_id": meeting_id},
            {
                "$set": {
                    "summary": transformed,
                    "status": "completed",
                    "updated_at": now,
                },
                "$inc": {"version": 1},
            },
        )

        logger.info(f"Summary completed for meeting {meeting_id}")

    except Exception as e:
        error_msg = str(e)
        logger.error(f"Summarization failed for meeting {meeting_id}: {error_msg}", exc_info=True)
        now = datetime.now(timezone.utc).isoformat()
        await meetings_col.update_one(
            {"meeting_id": meeting_id},
            {"$set": {"status": "error", "updated_at": now}},
        )


@router.post("")
async def start_summarization(
    req: SummarizeRequest,
    background_tasks: BackgroundTasks,
    current_user: dict = Depends(get_current_user),
):
    """Start summarization for a meeting."""
    meetings_col = get_meetings_collection()
    meeting = await meetings_col.find_one({
        "meeting_id": req.meeting_id,
        "user_id": current_user["user_id"],
    })

    if not meeting:
        raise HTTPException(status_code=404, detail="Meeting not found")

    transcript_text = meeting.get("transcript_text")
    if not transcript_text or not transcript_text.strip():
        raise HTTPException(status_code=400, detail="Meeting has no transcript to summarize")

    # Default model names per provider
    model_defaults = {
        "claude": "claude-sonnet-4-20250514",
        "groq": "llama-3.3-70b-versatile",
        "openai": "gpt-4o",
    }
    model_name = req.model or model_defaults.get(req.provider, "claude-sonnet-4-20250514")
    custom_prompt = req.custom_prompt or "Generate a summary of the meeting transcript."

    background_tasks.add_task(
        _process_summary,
        req.meeting_id,
        current_user["user_id"],
        transcript_text,
        req.provider,
        model_name,
        custom_prompt,
    )

    return {"meeting_id": req.meeting_id, "status": "processing"}


@router.get("/{meeting_id}/status")
async def get_summary_status(
    meeting_id: str,
    current_user: dict = Depends(get_current_user),
):
    """Poll summary status for a meeting."""
    meetings_col = get_meetings_collection()
    meeting = await meetings_col.find_one({
        "meeting_id": meeting_id,
        "user_id": current_user["user_id"],
    })

    if not meeting:
        raise HTTPException(status_code=404, detail="Meeting not found")

    status = meeting.get("status", "unknown")

    response = {
        "meeting_id": meeting_id,
        "status": status,
    }

    if status == "completed" and meeting.get("summary"):
        response["data"] = meeting["summary"]
    elif status == "error":
        response["error"] = "Summary generation failed"

    return response
