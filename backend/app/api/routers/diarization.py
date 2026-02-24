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

    if "operator is not unique" in lower or "'str' object has no attribute 'get'" in lower:
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


async def _run_deepgram_chunk_celery_workflow(
    diarization_service: DiarizationService,
    meeting_id: str,
    provider: str,
    wav_data: bytes,
    api_key: str,
) -> List[SpeakerSegment]:
    """
    Phase 5 chunk orchestration:
    - Persist chunk jobs
    - Enqueue Celery group (one task per chunk)
    - Wait for completion
    - Stitch + speaker reconciliation
    """
    chunks = diarization_service._split_wav_for_parallel(
        wav_data=wav_data,
        chunk_minutes=diarization_service.deepgram_parallel_chunk_minutes,
        overlap_seconds=diarization_service.deepgram_parallel_overlap_seconds,
    )
    if len(chunks) <= 1:
        return await diarization_service._diarize_with_deepgram(
            audio_data=wav_data,
            meeting_id=meeting_id,
            api_key=api_key,
            audio_url=None,
        )

    work_root = Path(
        os.getenv("DIARIZATION_CHUNK_WORK_DIR", "./data/diarization_chunks")
    ).resolve()
    run_token = datetime.utcnow().strftime("%Y%m%d%H%M%S")
    run_dir = work_root / meeting_id / run_token
    run_dir.mkdir(parents=True, exist_ok=True)

    await db.reset_diarization_chunk_jobs(meeting_id)
    chunk_jobs = []
    for chunk_index, start_sec, end_sec, chunk_bytes in chunks:
        chunk_path = run_dir / f"chunk_{chunk_index:04d}.wav"
        chunk_path.write_bytes(chunk_bytes)
        await db.upsert_diarization_chunk_job(
            meeting_id=meeting_id,
            chunk_index=chunk_index,
            status="pending",
            start_sec=start_sec,
            end_sec=end_sec,
        )
        chunk_jobs.append(
            {
                "chunk_index": chunk_index,
                "start_sec": start_sec,
                "end_sec": end_sec,
                "chunk_file_path": str(chunk_path),
            }
        )

    try:
        try:
            from ...tasks.audio_pipeline import enqueue_diarization_chunk_group
        except (ImportError, ValueError):
            from tasks.audio_pipeline import enqueue_diarization_chunk_group

        group_result = enqueue_diarization_chunk_group(
            meeting_id=meeting_id,
            provider=provider,
            api_key=api_key,
            chunk_jobs=chunk_jobs,
        )
        logger.info(
            "🚀 Enqueued diarization chunk group for %s: chunks=%s group_id=%s",
            meeting_id,
            len(chunk_jobs),
            group_result.id,
        )

        timeout_seconds = int(
            os.getenv("DIARIZATION_CHUNK_GROUP_TIMEOUT_SECONDS", "1800")
        )
        deadline = time.monotonic() + timeout_seconds
        while time.monotonic() < deadline:
            stats = await db.get_diarization_chunk_stats(meeting_id)
            if stats["completed"] + stats["failed"] >= stats["total"] and stats[
                "total"
            ] > 0:
                break
            await asyncio.sleep(1.0)
        else:
            raise TimeoutError(
                f"Diarization chunk group timeout for meeting {meeting_id}"
            )

        stats = await db.get_diarization_chunk_stats(meeting_id)
        logger.info(
            "✅ Chunk workflow terminal state for %s: total=%s completed=%s failed=%s",
            meeting_id,
            stats.get("total", 0),
            stats.get("completed", 0),
            stats.get("failed", 0),
        )
        if stats.get("failed", 0) > 0:
            raise RuntimeError(
                f"Diarization chunk workflow failed for {meeting_id}: {stats}"
            )

        rows = await db.list_diarization_chunk_jobs(meeting_id)
        by_idx = {r["chunk_index"]: r for r in rows if r["status"] == "completed"}

        chunk_results = []
        for chunk_index, start_sec, end_sec, _ in chunks:
            row = by_idx.get(chunk_index)
            if not row:
                continue
            raw_result = row.get("result_json")
            if isinstance(raw_result, str):
                try:
                    raw_result = json.loads(raw_result)
                except Exception:
                    raw_result = {}
            payload = (
                raw_result.get("segments", [])
                if isinstance(raw_result, dict)
                else []
            )
            segments = [
                SpeakerSegment(
                    speaker=s.get("speaker", "Speaker 0"),
                    start_time=float(s.get("start_time", 0.0)),
                    end_time=float(s.get("end_time", 0.0)),
                    text=s.get("text", ""),
                    confidence=float(s.get("confidence", 1.0)),
                    word_count=int(s.get("word_count", 0)),
                )
                for s in payload
            ]
            chunk_results.append((chunk_index, start_sec, end_sec, segments))

        stitched = diarization_service._stitch_parallel_segments(chunk_results)
        reconciled = diarization_service._reconcile_parallel_chunk_speakers(
            stitched_segments=stitched,
            overlap_seconds=diarization_service.deepgram_parallel_overlap_seconds,
            similarity_threshold=diarization_service.deepgram_parallel_similarity_threshold,
        )
        return [
            SpeakerSegment(
                speaker=s["speaker"],
                start_time=float(s["start_time"]),
                end_time=float(s["end_time"]),
                text=s.get("text", ""),
                confidence=float(s.get("confidence", 1.0)),
                word_count=int(s.get("word_count", 0)),
            )
            for s in reconciled
        ]
    finally:
        keep_chunks = (
            os.getenv("DIARIZATION_KEEP_CHUNK_FILES", "false").lower() == "true"
        )
        if not keep_chunks:
            try:
                for p in run_dir.glob("*.wav"):
                    p.unlink(missing_ok=True)
                run_dir.rmdir()
            except Exception:
                pass


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

        # 1. Get Audio URL (GCS) or local bytes
        audio_url = None
        audio_data = None
        selected_recording_path = None

        if storage_type == "gcp":
            logger.info(f"☁️ Using GCS audio for {meeting_id}")

            # Prefer WAV for provider compatibility and deterministic diarization.
            recording_candidates = [
                f"{meeting_id}/recording.wav",
                f"{meeting_id}/recording.m4a",
                f"{meeting_id}/recording.opus",
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

        # Step A/B strategy:
        # - Default: sequential (safer for Groq free-tier ASPH limits)
        # - Optional flag: parallel mode for paid tiers / higher quotas
        groq_parallel_with_diarization_enabled = (
            os.getenv("GROQ_PARALLEL_WITH_DIARIZATION_ENABLED", "false").lower()
            == "true"
        )
        whisper_task = None
        if audio_data and groq_parallel_with_diarization_enabled:
            logger.info(f"💎 Running High-Fidelity Groq Whisper for {meeting_id}...")
            whisper_task = asyncio.create_task(
                diarization_service.transcribe_with_whisper(
                    audio_data, user_email=user_email
                )
            )
        elif audio_data:
            logger.info(
                "💎 Running Groq baseline in sequential mode for %s (set GROQ_PARALLEL_WITH_DIARIZATION_ENABLED=true to parallelize)",
                meeting_id,
            )
        else:
            logger.info(
                f"No audio bytes available for Groq baseline in {meeting_id}; using diarization output for alignment."
            )

        logger.info(f"🎯 Starting speaker diarization provider call for {meeting_id}...")
        chunk_workflow_enabled = (
            os.getenv("DIARIZATION_CELERY_CHUNK_WORKFLOW_ENABLED", "true").lower()
            == "true"
        )
        celery_enabled = os.getenv("AUDIO_CELERY_ENABLED", "false").lower() == "true"

        async def _run_diarization_provider() -> DiarizationResult:
            if provider != "deepgram":
                return await diarization_service.diarize_meeting(
                    meeting_id=meeting_id,
                    storage_path=storage_path,
                    provider=provider,
                    audio_data=audio_data,
                    audio_url=audio_url,
                    user_email=user_email,
                )

            if not (chunk_workflow_enabled and celery_enabled):
                return await diarization_service.diarize_meeting(
                    meeting_id=meeting_id,
                    storage_path=storage_path,
                    provider=provider,
                    audio_data=audio_data,
                    audio_url=audio_url,
                    user_email=user_email,
                )

            # Chunk workflow requires local WAV bytes to slice and enqueue per-chunk tasks.
            if not audio_data:
                logger.info(
                    "Chunk workflow skipped for %s: no local audio bytes available",
                    meeting_id,
                )
                return await diarization_service.diarize_meeting(
                    meeting_id=meeting_id,
                    storage_path=storage_path,
                    provider=provider,
                    audio_data=audio_data,
                    audio_url=audio_url,
                    user_email=user_email,
                )

            wav_data = await diarization_service.ensure_wav_audio(
                audio_data, meeting_id
            )
            api_key = await diarization_service._get_api_key(provider, user_email)
            if not api_key:
                return DiarizationResult(
                    status="failed",
                    meeting_id=meeting_id,
                    speaker_count=0,
                    segments=[],
                    processing_time_seconds=0,
                    provider=provider,
                    error="No Deepgram API key available for chunk workflow",
                )

            started = datetime.utcnow()
            segments = await _run_deepgram_chunk_celery_workflow(
                diarization_service=diarization_service,
                meeting_id=meeting_id,
                provider=provider,
                wav_data=wav_data,
                api_key=api_key,
            )
            unique_speakers = set(s.speaker for s in segments)
            return DiarizationResult(
                status="completed",
                meeting_id=meeting_id,
                speaker_count=len(unique_speakers),
                segments=segments,
                processing_time_seconds=(datetime.utcnow() - started).total_seconds(),
                provider=provider,
            )

        whisper_segments = []
        if whisper_task:
            diarization_task = asyncio.create_task(_run_diarization_provider())
            diarization_out, whisper_out = await asyncio.gather(
                diarization_task, whisper_task, return_exceptions=True
            )
            if isinstance(diarization_out, Exception):
                raise diarization_out
            result = diarization_out

            if isinstance(whisper_out, Exception):
                logger.warning(
                    "Groq baseline failed for %s, proceeding with diarization-only text alignment: %s",
                    meeting_id,
                    whisper_out,
                )
            else:
                whisper_segments = whisper_out or []
        else:
            if audio_data:
                logger.info(
                    "💎 Running High-Fidelity Groq Whisper for %s (sequential phase)",
                    meeting_id,
                )
                whisper_segments = await diarization_service.transcribe_with_whisper(
                    audio_data, user_email=user_email
                )

            logger.info(
                "🎯 Starting speaker diarization provider call for %s (sequential phase)",
                meeting_id,
            )
            result = await _run_diarization_provider()

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
            alignment_input_segments = (
                whisper_segments
                if whisper_segments
                else [
                    {"start": s.start_time, "end": s.end_time, "text": s.text}
                    for s in result.segments
                ]
            )

            compact_enabled = (
                os.getenv("ALIGNMENT_COMPACT_ENABLED", "true").lower() == "true"
            )
            if compact_enabled and alignment_input_segments:
                compacted = _compact_transcript_segments(
                    segments=alignment_input_segments,
                    max_gap_seconds=float(
                        os.getenv("ALIGNMENT_COMPACT_MAX_GAP_SECONDS", "0.4")
                    ),
                    max_duration_seconds=float(
                        os.getenv("ALIGNMENT_COMPACT_MAX_DURATION_SECONDS", "8.0")
                    ),
                    min_segment_seconds=float(
                        os.getenv("ALIGNMENT_COMPACT_MIN_SEGMENT_SECONDS", "0.6")
                    ),
                    min_words=int(os.getenv("ALIGNMENT_COMPACT_MIN_WORDS", "3")),
                )
                logger.info(
                    "🧱 Alignment compaction for %s: raw=%s compacted=%s",
                    meeting_id,
                    len(alignment_input_segments),
                    len(compacted),
                )
                alignment_input_segments = compacted

            # Step C: Align (Using Groq Whisper as the high-accuracy baseline)
            (
                final_segments,
                alignment_metrics,
            ) = await diarization_service.align_with_transcripts(
                meeting_id,
                result,
                alignment_input_segments,
            )

            # Step D: Save to DB (single connection + batch inserts + atomic completion)
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
                        confidence_metrics = db._calculate_confidence_metrics(final_segments)
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
                        segments_json = [
                            {"speaker": s.speaker, "start": s.start_time, "end": s.end_time}
                            for s in result.segments
                        ]
                        await conn.execute(
                            """
                            UPDATE diarization_jobs
                            SET status = 'completed', completed_at = $1, result_json = $2
                            WHERE meeting_id = $3
                            """,
                            datetime.utcnow(),
                            json.dumps(segments_json),
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
