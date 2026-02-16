from fastapi import APIRouter, Depends, HTTPException, BackgroundTasks
from fastapi.responses import JSONResponse
import logging
import json
from datetime import datetime
from pathlib import Path
import os

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
    from services.audio.diarization import get_diarization_service, DiarizationService
    from services.audio.recorder import AudioRecorder
    from services.storage import StorageService

# Initialize
db = DatabaseManager()
rbac = RBAC(db)

router = APIRouter()
logger = logging.getLogger(__name__)


async def run_diarization_job(meeting_id: str, provider: str, user_email: str):
    """
    Background job that runs speaker diarization.
    """
    try:
        logger.info(
            f"🎯 Starting Gold Standard Diarization job for meeting {meeting_id}"
        )

        diarization_service = get_diarization_service()
        storage_path = os.getenv("RECORDINGS_STORAGE_PATH", "./data/recordings")
        storage_type = os.getenv("STORAGE_TYPE", "local").lower()

        def _looks_like_container_audio(data: bytes) -> bool:
            """
            Detect common container/file headers (WAV/OGG/MP3/MP4).
            Whisper helper expects raw PCM bytes, so we skip that path for containers.
            """
            if not data or len(data) < 12:
                return False
            return (
                data.startswith(b"RIFF")
                or data.startswith(b"OggS")
                or data.startswith(b"ID3")
                or data.startswith(b"\xff\xfb")
                or data[4:8] == b"ftyp"
            )

        # 1. Get Audio URL (GCS) or local bytes
        audio_url = None
        audio_data = None
        selected_recording_path = None

        if storage_type == "gcp":
            logger.info(f"☁️ Using GCS audio for {meeting_id}")

            recording_candidates = [
                f"{meeting_id}/recording.opus",
                f"{meeting_id}/recording.m4a",
                f"{meeting_id}/recording.wav",
            ]

            for candidate in recording_candidates:
                if await StorageService.check_file_exists(candidate):
                    selected_recording_path = candidate
                    break

            if not selected_recording_path:
                raise ValueError(
                    f"No recording artifact found in GCS for meeting {meeting_id}. "
                    "Expected one of recording.opus / recording.m4a / recording.wav."
                )

            audio_url = await StorageService.generate_signed_url(
                selected_recording_path, 3600
            )
            if not audio_url:
                raise ValueError(
                    f"Failed to generate signed URL for {selected_recording_path} in meeting {meeting_id}."
                )

            # Optional bytes download: only for whisper baseline when raw PCM is available.
            # For container formats we skip whisper baseline and use diarization segments for alignment.
            audio_data = await StorageService.download_bytes(selected_recording_path)
            if not audio_data:
                raise ValueError(
                    f"Failed to download audio bytes from GCS for {meeting_id} ({selected_recording_path})"
                )
        else:
            # Local mode fallback
            recording_dir = Path(storage_path) / meeting_id
            recording_dir.mkdir(parents=True, exist_ok=True)
            merged_wav = recording_dir / "merged_recording.wav"

            if merged_wav.exists():
                import aiofiles

                async with aiofiles.open(merged_wav, "rb") as af:
                    audio_data = await af.read()
            else:
                logger.info(f"🧩 Merging live audio chunks for {meeting_id}")
                audio_data = await AudioRecorder.merge_chunks(meeting_id, storage_path)

            if not audio_data:
                raise ValueError(
                    f"No audio data found for meeting {meeting_id} (Local)"
                )

        # CHECK CANCELLATION
        async with db._get_connection() as conn:
            job_status = await conn.fetchval(
                "SELECT status FROM diarization_jobs WHERE meeting_id = $1", meeting_id
            )
            if job_status == "stopped":
                logger.info(
                    f"🛑 Diarization job for {meeting_id} stopped before transcription."
                )
                return

        # 3. Run Diarization via Service (Logic moved to DiarizationService)
        # Note: In the new DiarizationService.diarize_meeting, it handles fetching audio if not provided.
        # But we provided it.
        # It also does alignment internally now?
        # Wait, the previous logic in main.py was doing A LOT of manual orchestration (Step 3, 4, 5, 6, 7).
        # DiarizationService.diarize_meeting only returns segments.
        # So we DO need to orchestrate the alignment and DB saving here (Controller Logic).

        # However, checking DiarizationService code I moved:
        # It has `align_with_transcripts` method.
        # It has `transcribe_with_whisper`.

        # So I need to replicate the main.py orchestration here.

        # Step A: High-fidelity Whisper (The Words) via Groq
        # This provides the accurate text baseline that we map speaker labels onto
        whisper_segments = []
        if audio_data and not _looks_like_container_audio(audio_data):
            logger.info(f"💎 Running High-Fidelity Groq Whisper for {meeting_id}...")
            whisper_segments = await diarization_service.transcribe_with_whisper(
                audio_data
            )
        else:
            logger.info(
                f"Skipping Groq whisper baseline for {meeting_id}; using diarization output directly for alignment."
            )

        # CHECK CANCELLATION
        async with db._get_connection() as conn:
            job_status = await conn.fetchval(
                "SELECT status FROM diarization_jobs WHERE meeting_id = $1", meeting_id
            )
            if job_status == "stopped":
                logger.info(
                    f"🛑 Diarization job for {meeting_id} stopped before diarization."
                )
                return

        # Step B: Diarization (The Speakers)
        # Note: We pass user_email so it can fetch the user-provided Deepgram API key
        result = await diarization_service.diarize_meeting(
            meeting_id=meeting_id,
            storage_path=storage_path,
            provider=provider,
            audio_data=audio_data,
            audio_url=audio_url,
            user_email=user_email,
        )

        # CHECK CANCELLATION
        async with db._get_connection() as conn:
            job_status = await conn.fetchval(
                "SELECT status FROM diarization_jobs WHERE meeting_id = $1", meeting_id
            )
            if job_status == "stopped":
                logger.info(
                    f"🛑 Diarization job for {meeting_id} stopped before alignment."
                )
                return

        if result.status == "completed":
            # Step C: Align (Using Groq Whisper as the high-accuracy baseline)
            (
                final_segments,
                alignment_metrics,
            ) = await diarization_service.align_with_transcripts(
                meeting_id,
                result,
                whisper_segments
                if whisper_segments
                else [
                    {"start": s.start_time, "end": s.end_time, "text": s.text}
                    for s in result.segments
                ],
            )

            # Step D: Save to DB
            async with db._get_connection() as conn:
                async with conn.transaction():
                    # 1. Clear old diarized transcripts only (Preserve live)
                    await conn.execute(
                        "DELETE FROM transcript_segments WHERE meeting_id = $1 AND source = 'diarized'",
                        meeting_id,
                    )

                    # 2. Insert new aligned segments
                    for t in final_segments:
                        start_val = t.get("start", 0)
                        timestamp_str = (
                            f"({int(start_val // 60):02d}:{int(start_val % 60):02d})"
                        )

                        await conn.execute(
                            """
                            INSERT INTO transcript_segments (
                                meeting_id, transcript, timestamp,
                                audio_start_time, audio_end_time, duration,
                                source, speaker, speaker_confidence, alignment_state
                            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
                            """,
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

                    # 3. Save Version (Always create a new version as per user request)
                    await db.save_transcript_version(
                        meeting_id=meeting_id,
                        source="diarized",
                        content=final_segments,
                        is_authoritative=True,
                        created_by=user_email,
                    )

            # 4. Update Jobs table
            async with db._get_connection() as conn:
                segments_json = [
                    {"speaker": s.speaker, "start": s.start_time, "end": s.end_time}
                    for s in result.segments
                ]
                await conn.execute(
                    "UPDATE diarization_jobs SET status = 'completed', completed_at = $1, result_json = $2 WHERE meeting_id = $3",
                    datetime.utcnow(),
                    json.dumps(segments_json),
                    meeting_id,
                )
                await conn.execute(
                    "UPDATE meetings SET diarization_status = 'completed' WHERE id = $1",
                    meeting_id,
                )
        else:
            # Failed
            async with db._get_connection() as conn:
                await conn.execute(
                    "UPDATE diarization_jobs SET status = 'failed', error_message = $1 WHERE meeting_id = $2",
                    result.error,
                    meeting_id,
                )
                await conn.execute(
                    "UPDATE meetings SET diarization_status = 'failed' WHERE id = $1",
                    meeting_id,
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
        raise HTTPException(status_code=500, detail=str(e))


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
        raise HTTPException(status_code=500, detail=str(e))


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
                error=row["error_message"],
                completed_at=row["completed_at"].isoformat()
                if row["completed_at"]
                else None,
            )

        return DiarizationStatusResponse(meeting_id=meeting_id, status="pending")

    except Exception as e:
        logger.error(f"Failed to get status: {e}")
        raise HTTPException(status_code=500, detail=str(e))


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
        raise HTTPException(status_code=500, detail=str(e))


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
        raise HTTPException(status_code=500, detail=str(e))
