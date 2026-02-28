from fastapi import APIRouter, Depends, HTTPException, BackgroundTasks
from fastapi.responses import JSONResponse
import logging
import json
import asyncio
import time
from datetime import datetime
from pathlib import Path
import os
from typing import Dict, List

try:
    from ..deps import get_current_user
    from ...schemas.user import User
    from ...schemas.transcript import (
        DiarizeRequest,
        DiarizationStatusResponse,
        SpeakerMappingResponse,
        RenameSpeakerRequest,
    )
    from ...db import DatabaseManager
    from ...core.rbac import RBAC
    from ...services.audio.diarization import (
        get_diarization_service,
        DiarizationService,
        DiarizationResult,
        SpeakerSegment,
    )
    from ...services.audio.recorder import AudioRecorder
    from ...services.storage import StorageService
except (ImportError, ValueError):
    from api.deps import get_current_user
    from schemas.user import User
    from schemas.transcript import (
        DiarizeRequest,
        DiarizationStatusResponse,
        SpeakerMappingResponse,
        RenameSpeakerRequest,
    )
    from db import DatabaseManager
    from core.rbac import RBAC
    from services.audio.diarization import (
        get_diarization_service,
        DiarizationService,
        DiarizationResult,
        SpeakerSegment,
    )
    from services.audio.recorder import AudioRecorder
    from services.storage import StorageService

# Initialize
db = DatabaseManager()
rbac = RBAC(db)

router = APIRouter()
logger = logging.getLogger(__name__)


def _sanitize_error_for_ui(raw: str | None) -> str | None:
    """
    Convert internal/provider errors into safe, user-facing messages.
    Raw error details remain in backend logs for debugging.
    """
    if not raw:
        return None

    message = str(raw).strip()
    lower = message.lower()

    if "rate limit" in lower and "please try again in" in lower:
        return "Transcription provider rate limit reached. Please retry after a few minutes."

    if "request_too_large" in lower or "payload too large" in lower:
        return "Audio is too large for a single transcription request. Please retry."

    if "no module named" in lower:
        return "A required backend service is unavailable. Please try again shortly."

    if (
        "operator is not unique" in lower
        or "'str' object has no attribute 'get'" in lower
    ):
        return "Temporary backend processing issue. Please retry diarization."

    if "no audio data found" in lower or "recording was enabled" in lower:
        return "No recording audio was found for this meeting."

    return "Diarization failed. Please retry."


def _word_count(text: str) -> int:
    return len((text or "").strip().split())


def _compact_transcript_segments(
    segments: List[Dict],
    max_gap_seconds: float,
    max_duration_seconds: float,
    min_segment_seconds: float,
    min_words: int,
) -> List[Dict]:
    """
    Merge micro transcript segments to improve speaker alignment confidence.
    """
    if not segments:
        return []

    normalized = []
    for seg in segments:
        start = float(seg.get("start", 0.0))
        end = float(seg.get("end", start))
        text = (seg.get("text", "") or "").strip()
        if end <= start:
            continue
        normalized.append({"start": start, "end": end, "text": text})

    normalized.sort(key=lambda s: s["start"])
    if not normalized:
        return []

    merged: List[Dict] = []
    current = dict(normalized[0])

    def _flush_current():
        if current["end"] > current["start"]:
            merged.append(dict(current))

    for nxt in normalized[1:]:
        cur_duration = current["end"] - current["start"]
        nxt_duration = nxt["end"] - nxt["start"]
        cur_words = _word_count(current["text"])
        nxt_words = _word_count(nxt["text"])
        gap = nxt["start"] - current["end"]
        combined_duration = nxt["end"] - current["start"]

        micro_cur = cur_duration < min_segment_seconds or cur_words < min_words
        micro_nxt = nxt_duration < min_segment_seconds or nxt_words < min_words

        should_merge = False
        if gap <= max_gap_seconds and combined_duration <= max_duration_seconds:
            should_merge = True
        if gap <= max_gap_seconds and (micro_cur or micro_nxt):
            should_merge = True

        if should_merge:
            current["end"] = max(current["end"], nxt["end"])
            if current["text"] and nxt["text"]:
                current["text"] = f"{current['text']} {nxt['text']}".strip()
            elif nxt["text"]:
                current["text"] = nxt["text"]
        else:
            _flush_current()
            current = dict(nxt)

    _flush_current()
    return merged


async def run_diarization_job(meeting_id: str, provider: str, user_email: str):
    """
    Background job that runs speaker diarization.
    Simplified: Download -> Decode -> Parallel Diarization -> Alignment -> Save
    """
    logger.info(
        f"💎 Starting Diarization Job for {meeting_id} with provider: {provider}"
    )
    try:
        start_time_total = datetime.utcnow()
        logger.info(f"🎯 Starting simplified diarization job for meeting {meeting_id}")

        diarization_service = get_diarization_service()
        storage_path = os.getenv("RECORDINGS_STORAGE_PATH", "./data/recordings")
        storage_type = os.getenv("STORAGE_TYPE", "local").lower()

        # 1. Get Audio Data (Bytes only)
        audio_data = None
        if storage_type == "gcp":
            logger.info(f"☁️ Downloading audio from GCS for {meeting_id}")
            recording_candidates = [
                f"{meeting_id}/recording.wav",
                f"{meeting_id}/recording.m4a",
                f"{meeting_id}/recording.opus",
            ]
            selected_path = None
            for candidate in recording_candidates:
                if await StorageService.check_file_exists(candidate):
                    selected_path = candidate
                    break

            if not selected_path:
                raise ValueError(f"No recording found in GCS for {meeting_id}")

            audio_data = await StorageService.download_bytes(selected_path)
        else:
            # Local mode
            recording_dir = Path(storage_path) / meeting_id
            merged_wav = recording_dir / "merged_recording.wav"
            if merged_wav.exists():
                import aiofiles

                async with aiofiles.open(merged_wav, "rb") as af:
                    audio_data = await af.read()
            else:
                audio_data = await AudioRecorder.merge_chunks(meeting_id, storage_path)

        if not audio_data:
            raise ValueError(f"No audio data found for meeting {meeting_id}")

        # 2. Prepare WAV Audio
        wav_data = await diarization_service.ensure_wav_audio(audio_data, meeting_id)
        logger.info(f"📦 Audio prepared: {len(wav_data)} bytes")

        # CHECK CANCELLATION
        async with db._get_connection() as conn:
            status = await conn.fetchval(
                "SELECT status FROM diarization_jobs WHERE meeting_id = $1", meeting_id
            )
            if status == "stopped":
                return

        # 3. Parallel Diarization & Gold Whisper Transcription
        logger.info(
            f"🚀 [Phase 1/2] Running parallel Groq-Whisper + {provider}-Diarization"
        )
        try:
            diarization_task = asyncio.create_task(
                diarization_service.diarize_meeting(
                    meeting_id=meeting_id,
                    audio_data=wav_data,
                    provider=provider,
                    user_email=user_email,
                )
            )
            whisper_task = asyncio.create_task(
                diarization_service.transcribe_with_whisper(
                    wav_data, user_email=user_email
                )
            )

            # Wait for both with a combined timeout
            diarization_result, whisper_output = await asyncio.gather(
                diarization_task, whisper_task
            )
        except Exception as e:
            logger.error(f"Parallel processing failed: {e}")
            raise

        # 4. Result Verification
        if not whisper_output:
            raise ValueError("Whisper transcription returned no data")
        if (
            not diarization_result
            or diarization_result.status == "failed"
            or not diarization_result.segments
        ):
            # Fallback: Just use Whisper results with 'Unknown' speaker
            logger.warning(
                "Diarization failed or returned no segments. Falling back to Whisper only."
            )
            diarization_result = DiarizationResult(
                status="completed",
                meeting_id=meeting_id,
                speaker_count=0,
                segments=[],
                processing_time_seconds=0.0,
                provider=provider,
                error="Fallback to Whisper (Diarization failed/empty)",
            )

        # 5. Extract Whisper Segments
        whisper_segments = (
            whisper_output.get("segments", [])
            if isinstance(whisper_output, dict)
            else whisper_output
        )

        # CHECK CANCELLATION again before expensive alignment
        async with db._get_connection() as conn:
            status = await conn.fetchval(
                "SELECT status FROM diarization_jobs WHERE meeting_id = $1", meeting_id
            )
            if status == "stopped":
                return

        # 🚀 [Phase 2/2] Align Gold Whisper segments with Speaker Diarization
        logger.info(
            f"🎯 [Phase 2/2] Aligning {len(whisper_segments)} Whisper segments with {len(diarization_result.segments)} speaker segments"
        )

        # Using the optimized 3-tier Alignment Engine
        (
            final_segments,
            alignment_metrics,
        ) = await diarization_service.align_with_transcripts(
            meeting_id, diarization_result, whisper_segments
        )

        # Step D: Translate to English (since we now use raw transcription)
        final_segments = await diarization_service.translate_aligned_transcript(
            meeting_id, final_segments, user_email
        )

        # Step E: Save to DB (single connection + batch inserts + atomic completion)
        db_save_start = time.perf_counter()

        async def _persist_and_complete():
            async with db._get_connection() as conn:
                async with conn.transaction():
                    # 1. Clear old diarized transcripts only (Preserve live)
                    await conn.execute(
                        "DELETE FROM transcript_segments WHERE meeting_id = $1 AND source = 'diarized'",
                        meeting_id,
                    )

                    # 2. Insert new aligned segments in batch
                    insert_rows = []
                    for t in final_segments:
                        start_val = t.get("start", 0)
                        timestamp_str = (
                            f"({int(start_val // 60):02d}:{int(start_val % 60):02d})"
                        )
                        insert_rows.append(
                            (
                                meeting_id,
                                t.get("text", ""),
                                timestamp_str,
                                t.get("start"),
                                t.get("end"),
                                (t.get("end", 0) - (t.get("start") or 0)),
                                "diarized",
                                t.get("speaker", "Speaker 0"),
                                t.get("speaker_confidence", 1.0),
                                t.get("alignment_state"),
                            )
                        )

                    if insert_rows:
                        await conn.executemany(
                            """
                            INSERT INTO transcript_segments (
                                meeting_id, transcript, timestamp,
                                audio_start_time, audio_end_time, duration,
                                source, speaker, speaker_confidence, alignment_state
                            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
                            """,
                            insert_rows,
                        )

                    # 3. Save version snapshot using the same connection
                    version_num = await conn.fetchval(
                        """
                        SELECT COALESCE(MAX(version_num), 0) + 1
                        FROM transcript_versions
                        WHERE meeting_id = $1
                        """,
                        meeting_id,
                    )
                    confidence_metrics = db._calculate_confidence_metrics(
                        final_segments
                    )
                    await conn.execute(
                        """
                        UPDATE transcript_versions
                        SET is_authoritative = FALSE
                        WHERE meeting_id = $1 AND is_authoritative = TRUE
                        """,
                        meeting_id,
                    )
                    await conn.execute(
                        """
                        INSERT INTO transcript_versions (
                            meeting_id, version_num, source, content_json,
                            is_authoritative, created_by, alignment_config, confidence_metrics
                        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
                        """,
                        meeting_id,
                        version_num,
                        "diarized",
                        json.dumps(final_segments, default=str),
                        True,
                        user_email,
                        json.dumps(
                            {
                                "provider": provider,
                                "alignment_metrics": alignment_metrics,
                            }
                        ),
                        json.dumps(confidence_metrics),
                    )

                    # 4. Mark job complete atomically with data write
                    segments_summary = [
                        {
                            "speaker": s.get("speaker", "Speaker 0"),
                            "start": s.get("start", 0.0),
                            "end": s.get("end", 0.0),
                        }
                        for s in final_segments
                    ]
                    await conn.execute(
                        """
                        UPDATE diarization_jobs
                        SET status = 'completed', completed_at = $1, result_json = $2
                        WHERE meeting_id = $3
                        """,
                        datetime.utcnow(),
                        json.dumps(segments_summary),
                        meeting_id,
                    )
                    await conn.execute(
                        "UPDATE meetings SET diarization_status = 'completed' WHERE id = $1",
                        meeting_id,
                    )

        db_timeout_seconds = int(
            os.getenv("DIARIZATION_DB_SAVE_TIMEOUT_SECONDS", "180")
        )
        await asyncio.wait_for(_persist_and_complete(), timeout=db_timeout_seconds)

        logger.info(
            "✅ Diarization persistence complete for %s: aligned=%s version_saved=true db_time=%.2fs",
            meeting_id,
            len(final_segments),
            time.perf_counter() - db_save_start,
        )

    except Exception as e:
        logger.error(f"Diarization job error: {e}")
        # Update DB to failed
        try:
            async with db._get_connection() as conn:
                await conn.execute(
                    "UPDATE diarization_jobs SET status = 'failed', error_message = $1 WHERE meeting_id = $2",
                    str(e),
                    meeting_id,
                )
                await conn.execute(
                    "UPDATE meetings SET diarization_status = 'failed' WHERE id = $1",
                    meeting_id,
                )
        except Exception as db_err:
            logger.error(f"Failed to update job status after error: {db_err}")


@router.post("/meetings/{meeting_id}/diarize")
async def diarize_meeting(
    meeting_id: str,
    request: DiarizeRequest = None,
    background_tasks: BackgroundTasks = None,
    current_user: User = Depends(get_current_user),
):
    """Trigger speaker diarization for a meeting."""
    if not await rbac.can(current_user, "ai_interact", meeting_id):
        raise HTTPException(status_code=403, detail="Permission denied")

    try:
        meeting = await db.get_meeting(meeting_id)
        if not meeting:
            raise HTTPException(status_code=404, detail="Meeting not found")

        provider = request.provider if request else "deepgram"

        # Create job entry
        async with db._get_connection() as conn:
            await conn.execute(
                """
                INSERT INTO diarization_jobs (meeting_id, status, provider, started_at)
                VALUES ($1, 'processing', $2, $3)
                ON CONFLICT (meeting_id) 
                DO UPDATE SET status = 'processing', provider = $2, started_at = $3, error_message = NULL
            """,
                meeting_id,
                provider,
                datetime.utcnow(),
            )
            await conn.execute(
                "UPDATE meetings SET diarization_status = 'processing' WHERE id = $1",
                meeting_id,
            )

        background_tasks.add_task(
            run_diarization_job, meeting_id, provider, current_user.email
        )

        return JSONResponse(
            {
                "status": "processing",
                "message": f"Diarization started with {provider}",
                "meeting_id": meeting_id,
            }
        )

    except Exception as e:
        logger.error(f"Failed to start diarization: {e}")
        raise HTTPException(
            status_code=500, detail="Failed to start diarization. Please try again."
        )


@router.post("/meetings/{meeting_id}/diarize/stop")
async def stop_diarization(
    meeting_id: str,
    current_user: User = Depends(get_current_user),
):
    """Stop the running diarization job."""
    if not await rbac.can(current_user, "ai_interact", meeting_id):
        raise HTTPException(status_code=403, detail="Permission denied")

    try:
        async with db._get_connection() as conn:
            # Check current status
            status = await conn.fetchval(
                "SELECT status FROM diarization_jobs WHERE meeting_id = $1", meeting_id
            )

            if status != "processing":
                return JSONResponse(
                    content={
                        "status": "ignored",
                        "message": f"Job is {status}, cannot stop",
                    },
                    status_code=400,
                )

            # Update status to stopped
            await conn.execute(
                "UPDATE diarization_jobs SET status = 'stopped', error_message = 'Stopped by user' WHERE meeting_id = $1",
                meeting_id,
            )
            await conn.execute(
                "UPDATE meetings SET diarization_status = 'stopped' WHERE id = $1",
                meeting_id,
            )

        return {"status": "success", "message": "Diarization stopping..."}

    except Exception as e:
        logger.error(f"Failed to stop diarization: {e}")
        raise HTTPException(
            status_code=500, detail="Failed to stop diarization. Please try again."
        )


@router.get(
    "/meetings/{meeting_id}/diarization-status",
    response_model=DiarizationStatusResponse,
)
async def get_diarization_status(
    meeting_id: str, current_user: User = Depends(get_current_user)
):
    """Get the diarization status for a meeting."""
    if not await rbac.can(current_user, "view", meeting_id):
        raise HTTPException(status_code=403, detail="Permission denied")

    try:
        async with db._get_connection() as conn:
            row = await conn.fetchrow(
                """
                SELECT status, speaker_count, provider, error_message, completed_at
                FROM diarization_jobs WHERE meeting_id = $1
            """,
                meeting_id,
            )

        if row:
            return DiarizationStatusResponse(
                meeting_id=meeting_id,
                status=row["status"],
                speaker_count=row["speaker_count"],
                provider=row["provider"],
                error=_sanitize_error_for_ui(row["error_message"]),
                completed_at=row["completed_at"].isoformat()
                if row["completed_at"]
                else None,
            )

        return DiarizationStatusResponse(meeting_id=meeting_id, status="pending")

    except Exception as e:
        logger.error(f"Failed to get status: {e}")
        raise HTTPException(
            status_code=500, detail="Failed to fetch diarization status."
        )


@router.get("/meetings/{meeting_id}/diarization-progress")
async def get_diarization_progress(
    meeting_id: str, current_user: User = Depends(get_current_user)
):
    """Get chunk-level progress for Phase 5 chunked diarization workflow."""
    if not await rbac.can(current_user, "view", meeting_id):
        raise HTTPException(status_code=403, detail="Permission denied")

    try:
        stats = await db.get_diarization_chunk_stats(meeting_id)
        chunks = await db.list_diarization_chunk_jobs(meeting_id)
        total = stats.get("total", 0)
        completed = stats.get("completed", 0)
        failed = stats.get("failed", 0)
        processing = stats.get("processing", 0)
        pending = stats.get("pending", 0)
        return {
            "meeting_id": meeting_id,
            "total_chunks": total,
            "processed_chunks": completed + failed,
            "completed_chunks": completed,
            "failed_chunks": failed,
            "processing_chunks": processing,
            "pending_chunks": pending,
            "percent_complete": (
                round(((completed + failed) / total) * 100, 2) if total else 0.0
            ),
            "chunks": chunks,
        }
    except Exception as e:
        logger.error(f"Failed to get diarization progress: {e}")
        raise HTTPException(
            status_code=500, detail="Failed to fetch diarization progress."
        )


@router.get("/meetings/{meeting_id}/speakers", response_model=SpeakerMappingResponse)
async def get_meeting_speakers(
    meeting_id: str, current_user: User = Depends(get_current_user)
):
    """Get speaker label mappings for a meeting."""
    if not await rbac.can(current_user, "view", meeting_id):
        raise HTTPException(status_code=403, detail="Permission denied")

    try:
        async with db._get_connection() as conn:
            rows = await conn.fetch(
                """
                SELECT diarization_label, display_name, color
                FROM meeting_speakers 
                WHERE meeting_id = $1
                ORDER BY diarization_label
            """,
                meeting_id,
            )

        speakers = [
            {
                "label": row["diarization_label"],
                "display_name": row["display_name"] or row["diarization_label"],
                "color": row["color"],
            }
            for row in rows
        ]

        return SpeakerMappingResponse(meeting_id=meeting_id, speakers=speakers)

    except Exception as e:
        logger.error(f"Failed to get speakers: {e}")
        raise HTTPException(status_code=500, detail="Failed to fetch speakers.")


@router.patch("/meetings/{meeting_id}/speakers/{speaker_label}/rename")
async def rename_speaker(
    meeting_id: str,
    speaker_label: str,
    request: RenameSpeakerRequest,
    current_user: User = Depends(get_current_user),
):
    """Rename a speaker label."""
    if not await rbac.can(current_user, "edit", meeting_id):
        raise HTTPException(status_code=403, detail="Permission denied")

    try:
        async with db._get_connection() as conn:
            await conn.execute(
                """
                INSERT INTO meeting_speakers (meeting_id, diarization_label, display_name)
                VALUES ($1, $2, $3)
                ON CONFLICT (meeting_id, diarization_label) 
                DO UPDATE SET display_name = $3
            """,
                meeting_id,
                speaker_label,
                request.display_name,
            )

            # Also update transcripts
            await conn.execute(
                "UPDATE transcript_segments SET speaker = $1 WHERE meeting_id = $2 AND speaker = $3",
                request.display_name,
                meeting_id,
                speaker_label,
            )

        return {"status": "success", "message": "Speaker renamed"}

    except Exception as e:
        logger.error(f"Failed to rename speaker: {e}")
        raise HTTPException(status_code=500, detail="Failed to rename speaker.")
