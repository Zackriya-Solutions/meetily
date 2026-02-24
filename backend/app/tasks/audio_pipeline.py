import asyncio
import logging
import uuid
import json
from typing import Optional
import os
from pathlib import Path

from celery import shared_task, group

try:
    from ..db import DatabaseManager
    from ..services.audio.post_recording import get_post_recording_service
    from ..services.audio.pipeline_state import get_audio_pipeline_state_service
    from ..services.audio.diarization import DiarizationService
    from ..services.storage import StorageService
except (ImportError, ValueError):
    from db import DatabaseManager
    from services.audio.post_recording import get_post_recording_service
    from services.audio.pipeline_state import get_audio_pipeline_state_service
    from services.audio.diarization import DiarizationService
    from services.storage import StorageService

logger = logging.getLogger(__name__)


def enqueue_diarization_chunk_group(
    meeting_id: str,
    provider: str,
    api_key: str,
    chunk_jobs: list[dict],
):
    """
    Enqueue a Celery group for per-chunk diarization processing.
    """
    sigs = [
        diarization_process_chunk_task.s(
            meeting_id=meeting_id,
            provider=provider,
            api_key=api_key,
            chunk_index=job["chunk_index"],
            chunk_file_path=job["chunk_file_path"],
            start_sec=float(job["start_sec"]),
            end_sec=float(job["end_sec"]),
        )
        for job in chunk_jobs
    ]
    return group(sigs).apply_async()


def enqueue_finalize_session_task(session_id: str) -> str:
    """
    Helper used by API/reconciler to enqueue a durable finalize task.
    """
    result = finalize_session_task.delay(session_id)
    return str(result.id)


def enqueue_postprocess_session_task(session_id: str, user_email: Optional[str]) -> str:
    result = postprocess_session_task.delay(session_id, user_email=user_email)
    return str(result.id)


def enqueue_upload_chunk_task(
    session_id: str,
    chunk_index: int,
    storage_path: str,
    byte_size: int,
    checksum: Optional[str] = None,
) -> str:
    result = upload_chunk_task.delay(
        session_id=session_id,
        chunk_index=chunk_index,
        storage_path=storage_path,
        byte_size=byte_size,
        checksum=checksum,
    )
    return str(result.id)


@shared_task(
    bind=True,
    name="diarization.process_chunk",
    autoretry_for=(Exception,),
    retry_backoff=True,
    retry_jitter=True,
    max_retries=3,
)
def diarization_process_chunk_task(
    self,
    meeting_id: str,
    provider: str,
    api_key: str,
    chunk_index: int,
    chunk_file_path: str,
    start_sec: float,
    end_sec: float,
):
    asyncio.run(
        _diarization_process_chunk_async(
            task_id=str(self.request.id),
            meeting_id=meeting_id,
            provider=provider,
            api_key=api_key,
            chunk_index=chunk_index,
            chunk_file_path=chunk_file_path,
            start_sec=start_sec,
            end_sec=end_sec,
        )
    )


async def _diarization_process_chunk_async(
    task_id: str,
    meeting_id: str,
    provider: str,
    api_key: str,
    chunk_index: int,
    chunk_file_path: str,
    start_sec: float,
    end_sec: float,
):
    db = DatabaseManager()
    await db.upsert_diarization_chunk_job(
        meeting_id=meeting_id,
        chunk_index=chunk_index,
        status="processing",
        start_sec=start_sec,
        end_sec=end_sec,
        task_id=task_id,
    )

    try:
        chunk_path = Path(chunk_file_path)
        if not chunk_path.exists():
            raise FileNotFoundError(f"Chunk file missing: {chunk_file_path}")

        audio_data = chunk_path.read_bytes()
        service = DiarizationService(provider=provider)
        segments = await service._diarize_with_deepgram(
            audio_data=audio_data,
            meeting_id=f"{meeting_id}-chunk-{chunk_index}",
            api_key=api_key,
            audio_url=None,
        )
        payload = [
            {
                "speaker": s.speaker,
                "start_time": s.start_time,
                "end_time": s.end_time,
                "text": s.text,
                "confidence": s.confidence,
                "word_count": s.word_count,
            }
            for s in segments
        ]
        await db.upsert_diarization_chunk_job(
            meeting_id=meeting_id,
            chunk_index=chunk_index,
            status="completed",
            start_sec=start_sec,
            end_sec=end_sec,
            task_id=task_id,
            segment_count=len(payload),
            result_json={"segments": payload},
        )
        return {
            "meeting_id": meeting_id,
            "chunk_index": chunk_index,
            "start_sec": start_sec,
            "end_sec": end_sec,
            "status": "completed",
            "segment_count": len(payload),
        }
    except Exception as exc:
        await db.upsert_diarization_chunk_job(
            meeting_id=meeting_id,
            chunk_index=chunk_index,
            status="failed",
            start_sec=start_sec,
            end_sec=end_sec,
            task_id=task_id,
            error_message=str(exc),
        )
        logger.error(
            "[DiarizationChunk] Failed meeting=%s chunk=%s: %s",
            meeting_id,
            chunk_index,
            exc,
        )
        raise


@shared_task(
    bind=True,
    name="audio.upload_chunk",
    autoretry_for=(Exception,),
    retry_backoff=True,
    retry_jitter=True,
    max_retries=5,
)
def upload_chunk_task(
    self,
    session_id: str,
    chunk_index: int,
    storage_path: str,
    byte_size: int,
    checksum: Optional[str] = None,
):
    asyncio.run(
        _upload_chunk_async(
            session_id=session_id,
            chunk_index=chunk_index,
            storage_path=storage_path,
            byte_size=byte_size,
            checksum=checksum,
        )
    )


async def _upload_chunk_async(
    session_id: str,
    chunk_index: int,
    storage_path: str,
    byte_size: int,
    checksum: Optional[str] = None,
):
    db = DatabaseManager()
    storage_type = os.getenv("STORAGE_TYPE", "local").lower()

    session = await db.get_recording_session(session_id)
    if not session:
        logger.warning(
            "[CeleryAudio] Session not found for chunk upload: %s#%s",
            session_id,
            chunk_index,
        )
        return

    # This task currently validates persisted chunk availability and marks durability state.
    # Actual upload may already be completed in inline path.
    exists = False
    if storage_type == "gcp":
        exists = await StorageService.check_file_exists(storage_path)
    else:
        local_storage_root = Path(
            os.getenv("RECORDINGS_STORAGE_PATH", "./data/recordings")
        )
        local_chunk = local_storage_root / session["meeting_id"] / Path(storage_path).name
        exists = local_chunk.exists()

    await db.upsert_recording_chunk(
        session_id=session_id,
        chunk_index=chunk_index,
        byte_size=byte_size,
        checksum=checksum,
        storage_path=storage_path,
        upload_status="uploaded" if exists else "failed",
    )

    if not exists:
        raise RuntimeError(
            f"Chunk not found for durability mark: {session_id}#{chunk_index} at {storage_path}"
        )


@shared_task(
    bind=True,
    name="audio.finalize_session",
    autoretry_for=(Exception,),
    retry_backoff=True,
    retry_jitter=True,
    max_retries=5,
)
def finalize_session_task(self, session_id: str):
    """
    Durable finalize entrypoint (Phase 2 foundation).
    """
    asyncio.run(_finalize_session_async(session_id))


async def _finalize_session_async(session_id: str):
    db = DatabaseManager()
    state = get_audio_pipeline_state_service()

    session = await db.get_recording_session(session_id)
    if not session:
        logger.warning("[CeleryAudio] Session not found for finalize: %s", session_id)
        return

    if session["status"] in ("completed", "failed"):
        return

    await state.transition(session_id, "finalizing")

    try:
        chunk_upload_via_celery = (
            os.getenv("AUDIO_CHUNK_UPLOAD_VIA_CELERY", "false").lower() == "true"
        )
        if chunk_upload_via_celery:
            stats = await db.get_recording_chunk_stats(session_id)
            await db.update_recording_session_counters(
                session_id=session_id,
                expected_chunk_count=stats.get("total", 0),
                finalized_chunk_count=stats.get("uploaded", 0),
            )
            if stats.get("failed", 0) > 0:
                await state.transition(
                    session_id,
                    "failed",
                    error_code="CHUNK_UPLOAD_FAILED",
                    error_message="One or more chunks failed durability checks",
                )
                raise RuntimeError(
                    f"Chunk upload failure detected for session {session_id}: {stats}"
                )
            if stats.get("pending", 0) > 0:
                await state.transition(session_id, "uploading_chunks")
                raise RuntimeError(
                    f"Chunk uploads still pending for session {session_id}: {stats}"
                )

        finalize_key = session.get("idempotency_finalize_key")
        if not finalize_key:
            finalize_key = f"finalize:{session_id}:{uuid.uuid4().hex[:10]}"
            await db.set_recording_finalize_key(session_id, finalize_key)

        post_service = get_post_recording_service()
        result = await post_service.finalize_recording(
            meeting_id=session["meeting_id"],
            trigger_diarization=False,
            user_email=session["user_email"],
        )
        if result.get("status") == "completed":
            postprocess_enabled = (
                os.getenv("AUDIO_POSTPROCESS_ENABLED", "true").lower() == "true"
            )
            if postprocess_enabled:
                transitioned = await state.transition(session_id, "postprocessing")
                if transitioned:
                    post_task_id = enqueue_postprocess_session_task(
                        session_id, session.get("user_email")
                    )
                    await db.merge_recording_session_metadata(
                        session_id,
                        {
                            "postprocess_task_id": post_task_id,
                            "postprocess_enqueued": True,
                        },
                    )
                else:
                    await state.transition(session_id, "completed")
            else:
                await state.transition(session_id, "completed")
        else:
            await state.transition(
                session_id,
                "failed",
                error_code="FINALIZE_FAILED",
                error_message=result.get("error") or "Finalize failed",
            )
    except Exception as exc:
        logger.error(
            "[CeleryAudio] Finalize task failed for session %s: %s", session_id, exc
        )
        await state.transition(
            session_id,
            "failed",
            error_code="FINALIZE_EXCEPTION",
            error_message=str(exc),
        )
        raise


@shared_task(
    bind=True,
    name="audio.postprocess_session",
    autoretry_for=(Exception,),
    retry_backoff=True,
    retry_jitter=True,
    max_retries=5,
)
def postprocess_session_task(self, session_id: str, user_email: Optional[str] = None):
    """
    Reserved for Phase 3: durable diarization/summarization orchestration.
    """
    asyncio.run(_postprocess_session_async(session_id, user_email))


async def _postprocess_session_async(session_id: str, user_email: Optional[str]):
    db = DatabaseManager()
    state = get_audio_pipeline_state_service()
    session = await db.get_recording_session(session_id)
    if not session:
        logger.warning("[CeleryAudio] Session not found for postprocess: %s", session_id)
        return
    if session["status"] in ("completed", "failed"):
        return

    effective_email = user_email or session.get("user_email")
    diarization_enabled = (
        os.getenv("AUDIO_POSTPROCESS_DIARIZATION_ENABLED", "false").lower() == "true"
    )
    provider = os.getenv("AUDIO_POSTPROCESS_DIARIZATION_PROVIDER", "deepgram")

    try:
        if diarization_enabled:
            try:
                from ..api.routers.diarization import run_diarization_job
            except (ImportError, ValueError):
                from api.routers.diarization import run_diarization_job
            await run_diarization_job(
                meeting_id=session["meeting_id"],
                provider=provider,
                user_email=effective_email,
            )
            await db.merge_recording_session_metadata(
                session_id,
                {
                    "diarization_triggered": True,
                    "diarization_provider": provider,
                },
            )

        await state.transition(session_id, "completed")
    except Exception as exc:
        logger.error(
            "[CeleryAudio] Postprocess failed for session %s: %s", session_id, exc
        )
        await state.transition(
            session_id,
            "failed",
            error_code="POSTPROCESS_EXCEPTION",
            error_message=str(exc),
        )
        raise
