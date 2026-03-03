"""
Cloud transcription routes — upload audio for transcription via Deepgram/OpenAI Whisper.
"""

import logging
import uuid
from datetime import datetime, timezone
from typing import Optional

from fastapi import APIRouter, HTTPException, Depends, UploadFile, File, Form, BackgroundTasks
from auth_middleware import get_current_user
from cloud_transcription_service import get_transcription_provider, TranscriptResult
from mongodb import get_meetings_collection, get_transcription_jobs_collection

logger = logging.getLogger(__name__)

router = APIRouter(prefix="/api/transcription", tags=["transcription-cloud"])

MAX_UPLOAD_SIZE = 100 * 1024 * 1024  # 100 MB
ALLOWED_FORMATS = {"wav", "m4a", "mp3", "webm", "mp4", "ogg", "flac"}


async def _process_transcription(
    job_id: str,
    meeting_id: str,
    audio_data: bytes,
    audio_format: str,
    provider_name: str,
    language: Optional[str],
):
    """Background task: run transcription and update meeting."""
    jobs_col = get_transcription_jobs_collection()
    meetings_col = get_meetings_collection()

    try:
        # Update job status
        await jobs_col.update_one(
            {"job_id": job_id},
            {"$set": {"status": "processing", "updated_at": datetime.now(timezone.utc).isoformat()}},
        )

        # Update meeting status
        await meetings_col.update_one(
            {"meeting_id": meeting_id},
            {"$set": {"status": "transcribing"}},
        )

        # Run transcription
        provider = get_transcription_provider(provider_name)
        result: TranscriptResult = await provider.transcribe(audio_data, audio_format, language)

        # Save transcript to meeting
        now = datetime.now(timezone.utc).isoformat()
        await meetings_col.update_one(
            {"meeting_id": meeting_id},
            {
                "$set": {
                    "transcript_text": result.text,
                    "transcript_segments": result.segments,
                    "duration_seconds": result.duration_seconds,
                    "transcription_provider": provider_name,
                    "status": "completed",
                    "updated_at": now,
                },
                "$inc": {"version": 1},
            },
        )

        # Update job as completed
        await jobs_col.update_one(
            {"job_id": job_id},
            {
                "$set": {
                    "status": "completed",
                    "updated_at": now,
                    "result": {
                        "text": result.text,
                        "segments": result.segments,
                        "duration_seconds": result.duration_seconds,
                        "language": result.language,
                    },
                }
            },
        )

        logger.info(f"Transcription completed for job {job_id}, meeting {meeting_id}")

    except Exception as e:
        error_msg = str(e)
        logger.error(f"Transcription failed for job {job_id}: {error_msg}", exc_info=True)
        now = datetime.now(timezone.utc).isoformat()
        await jobs_col.update_one(
            {"job_id": job_id},
            {"$set": {"status": "failed", "error": error_msg, "updated_at": now}},
        )
        await meetings_col.update_one(
            {"meeting_id": meeting_id},
            {"$set": {"status": "error", "updated_at": now}},
        )


@router.post("/upload")
async def upload_for_transcription(
    background_tasks: BackgroundTasks,
    audio: UploadFile = File(...),
    meeting_id: str = Form(...),
    provider: str = Form("deepgram"),
    language: Optional[str] = Form(None),
    current_user: dict = Depends(get_current_user),
):
    """Upload audio file for cloud transcription."""
    # Validate format
    ext = audio.filename.rsplit(".", 1)[-1].lower() if audio.filename else "m4a"
    if ext not in ALLOWED_FORMATS:
        raise HTTPException(status_code=400, detail=f"Unsupported format: {ext}")

    # Read audio data
    audio_data = await audio.read()
    if len(audio_data) > MAX_UPLOAD_SIZE:
        raise HTTPException(status_code=413, detail="File too large (max 100MB)")
    if len(audio_data) == 0:
        raise HTTPException(status_code=400, detail="Empty audio file")

    # Verify meeting belongs to user
    meetings_col = get_meetings_collection()
    meeting = await meetings_col.find_one({
        "meeting_id": meeting_id,
        "user_id": current_user["user_id"],
    })
    if not meeting:
        raise HTTPException(status_code=404, detail="Meeting not found")

    # Create transcription job
    job_id = str(uuid.uuid4())
    now = datetime.now(timezone.utc).isoformat()
    jobs_col = get_transcription_jobs_collection()
    await jobs_col.insert_one({
        "job_id": job_id,
        "meeting_id": meeting_id,
        "user_id": current_user["user_id"],
        "provider": provider,
        "language": language,
        "audio_format": ext,
        "audio_size_bytes": len(audio_data),
        "status": "pending",
        "created_at": now,
        "updated_at": now,
        "result": None,
        "error": None,
    })

    # Process in background
    background_tasks.add_task(
        _process_transcription,
        job_id,
        meeting_id,
        audio_data,
        ext,
        provider,
        language,
    )

    logger.info(f"Transcription job {job_id} queued for meeting {meeting_id}")
    return {"transcription_id": job_id, "status": "pending"}


@router.get("/{job_id}/status")
async def get_transcription_status(
    job_id: str,
    current_user: dict = Depends(get_current_user),
):
    """Poll transcription job status."""
    jobs_col = get_transcription_jobs_collection()
    job = await jobs_col.find_one({
        "job_id": job_id,
        "user_id": current_user["user_id"],
    })

    if not job:
        raise HTTPException(status_code=404, detail="Transcription job not found")

    response = {
        "id": job["job_id"],
        "status": job["status"],
        "error": job.get("error"),
    }

    if job["status"] == "completed" and job.get("result"):
        response["transcript"] = job["result"]

    return response


@router.get("/quota")
async def get_transcription_quota(
    current_user: dict = Depends(get_current_user),
):
    """Check user's remaining transcription quota."""
    # TODO: Implement plan-based quota from usage_summaries collection
    # For now, return unlimited
    return {
        "remaining_minutes": 999,
        "plan_limit": 999,
        "used_minutes": 0,
    }
