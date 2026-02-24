from fastapi import (
    APIRouter,
    WebSocket,
    WebSocketDisconnect,
    Depends,
    HTTPException,
    File,
    UploadFile,
    Form,
    BackgroundTasks,
)
from typing import Optional
import uuid
import logging
import time
import os
import struct
import json
import asyncio
import re
from datetime import datetime
from pathlib import Path
import aiofiles

try:
    from ..deps import get_current_user
    from ...schemas.user import User
    from ...db import DatabaseManager
    from ...core.rbac import RBAC
    from ...core.security import verify_google_token
    from ...services.audio.manager import StreamingTranscriptionManager
    from ...services.audio.recorder import get_or_create_recorder, stop_recorder
    from ...services.audio.post_recording import get_post_recording_service
    from ...services.audio.pipeline_state import get_audio_pipeline_state_service
    from ...services.storage import StorageService
except (ImportError, ValueError):
    from api.deps import get_current_user
    from schemas.user import User
    from db import DatabaseManager
    from core.rbac import RBAC
    from core.security import verify_google_token
    from services.audio.manager import StreamingTranscriptionManager
    from services.audio.recorder import get_or_create_recorder, stop_recorder
    from services.audio.post_recording import get_post_recording_service
    from services.audio.pipeline_state import get_audio_pipeline_state_service
    from services.storage import StorageService

db = DatabaseManager()
rbac = RBAC(db)

router = APIRouter()
logger = logging.getLogger(__name__)

# Track active streaming sessions
streaming_managers = {}
active_connections = {}
session_cleanup_tasks = {}
session_context = {}
session_finalize_locks = {}
session_finalized = set()

RESUME_GRACE_SECONDS = int(os.getenv("STREAMING_RESUME_GRACE_SECONDS", "45"))
STREAMING_AUDIO_QUEUE_MAX_CHUNKS = int(
    os.getenv("STREAMING_AUDIO_QUEUE_MAX_CHUNKS", "256")
)
STREAMING_AUDIO_DROP_POLICY = os.getenv(
    "STREAMING_AUDIO_DROP_POLICY", "drop_oldest"
).lower()
SAFE_ID_PATTERN = re.compile(r"^[A-Za-z0-9_-]{1,128}$")
CHUNK_FILENAME_PATTERN = re.compile(r"chunk_(\d+)\.pcm$")
AUDIO_CELERY_ENABLED = os.getenv("AUDIO_CELERY_ENABLED", "false").lower() == "true"
CHUNK_UPLOAD_VIA_CELERY = (
    os.getenv("AUDIO_CHUNK_UPLOAD_VIA_CELERY", "false").lower() == "true"
)
STORAGE_TYPE = os.getenv("STORAGE_TYPE", "local").lower()

state_service = get_audio_pipeline_state_service()


def _cancel_pending_cleanup(session_id: str):
    task = session_cleanup_tasks.pop(session_id, None)
    if task and not task.done():
        task.cancel()


async def _finalize_session(
    session_id: str, flush: bool = True, process_audio: bool = True
):
    """
    Finalize recorder + manager for a session.
    Safe to call multiple times; no-op if already cleaned.
    """
    lock = session_finalize_locks.setdefault(session_id, asyncio.Lock())
    async with lock:
        if session_id in session_finalized:
            return
        if active_connections.get(session_id, 0) > 0:
            return

        ctx = session_context.get(session_id, {})
        recorder_key = ctx.get("recorder_key") or session_id
        user_email = ctx.get("user_email")
        mgr = streaming_managers.get(session_id)
        if flush and mgr:
            try:
                await mgr.force_flush()
            except Exception as e:
                logger.error(f"Force flush failed for session {session_id}: {e}")

        try:
            recorder_metadata = await stop_recorder(recorder_key)
            if recorder_metadata and isinstance(recorder_metadata, dict):
                chunks = recorder_metadata.get("chunks", []) or []
                for ch in chunks:
                    try:
                        chunk_index = ch.get("chunk_index")
                        storage_path = ch.get("storage_path")
                        size_bytes = int(ch.get("size_bytes") or 0)
                        if chunk_index is None or not storage_path:
                            continue
                        should_enqueue_chunk_upload = (
                            AUDIO_CELERY_ENABLED
                            and CHUNK_UPLOAD_VIA_CELERY
                            and STORAGE_TYPE != "gcp"
                        )
                        upload_status = "pending" if should_enqueue_chunk_upload else "uploaded"
                        await db.upsert_recording_chunk(
                            session_id=session_id,
                            chunk_index=int(chunk_index),
                            byte_size=size_bytes,
                            storage_path=storage_path,
                            upload_status=upload_status,
                        )
                        if should_enqueue_chunk_upload:
                            try:
                                try:
                                    from ...tasks.audio_pipeline import (
                                        enqueue_upload_chunk_task,
                                    )
                                except (ImportError, ValueError):
                                    from tasks.audio_pipeline import (
                                        enqueue_upload_chunk_task,
                                    )
                                enqueue_upload_chunk_task(
                                    session_id=session_id,
                                    chunk_index=int(chunk_index),
                                    storage_path=storage_path,
                                    byte_size=size_bytes,
                                )
                            except Exception:
                                pass
                    except Exception:
                        continue
            if process_audio:
                post_service = get_post_recording_service()
                asyncio.create_task(
                    post_service.finalize_recording(
                        recorder_key,
                        trigger_diarization=False,
                        user_email=user_email,
                    )
                )
                logger.info(
                    f"[Streaming] Scheduled post-recording processing for {recorder_key}"
                )
            else:
                logger.info(
                    f"[Streaming] Recorder closed for {recorder_key} (post-processing deferred until save)."
                )
        except Exception as e:
            logger.warning(
                f"[Streaming] Recorder finalize failed for {recorder_key}: {e}"
            )

        if mgr:
            try:
                mgr.cleanup()
            except Exception:
                pass
            streaming_managers.pop(session_id, None)

        active_connections.pop(session_id, None)
        session_context.pop(session_id, None)
        session_finalized.add(session_id)
        _cancel_pending_cleanup(session_id)
        try:
            if process_audio:
                await state_service.transition(session_id, "completed")
            else:
                await state_service.transition(session_id, "uploading_chunks")
        except Exception:
            pass


def _is_safe_identifier(value: str) -> bool:
    if not value:
        return False
    try:
        uuid.UUID(value)
        return True
    except Exception:
        return bool(SAFE_ID_PATTERN.match(value))


async def _authenticate_websocket(auth_token: Optional[str]) -> Optional[User]:
    if not auth_token:
        return None
    payload = await verify_google_token(auth_token)
    email = payload.get("email")
    if not email:
        raise HTTPException(status_code=401, detail="Token missing email")
    if not email.endswith("@appointy.com"):
        raise HTTPException(
            status_code=403,
            detail=f"Access restricted to @appointy.com users (found {email})",
        )
    return User(email=email, name=payload.get("name"), picture=payload.get("picture"))


def _schedule_deferred_cleanup(session_id: str):
    _cancel_pending_cleanup(session_id)

    async def _cleanup_after_grace():
        try:
            await asyncio.sleep(RESUME_GRACE_SECONDS)
            if active_connections.get(session_id, 0) <= 0:
                logger.info(
                    f"[Streaming] Resume grace expired for {session_id}; finalizing session."
                )
                if AUDIO_CELERY_ENABLED:
                    await _finalize_session(session_id, flush=False, process_audio=False)
                    try:
                        try:
                            from ...tasks.audio_pipeline import (
                                enqueue_finalize_session_task,
                            )
                        except (ImportError, ValueError):
                            from tasks.audio_pipeline import enqueue_finalize_session_task

                        await state_service.transition(session_id, "finalizing")
                        task_id = enqueue_finalize_session_task(session_id)
                        await state_service.db.merge_recording_session_metadata(
                            session_id,
                            {
                                "finalize_task_id": task_id,
                                "finalize_enqueued": True,
                            },
                        )
                    except Exception as enqueue_err:
                        logger.error(
                            "[Streaming] Deferred finalize enqueue failed for %s: %s",
                            session_id,
                            enqueue_err,
                        )
                else:
                    await _finalize_session(session_id, flush=False, process_audio=True)
        except asyncio.CancelledError:
            return
        except Exception as e:
            logger.error(f"[Streaming] Deferred cleanup failed for {session_id}: {e}")
        finally:
            session_cleanup_tasks.pop(session_id, None)

    session_cleanup_tasks[session_id] = asyncio.create_task(_cleanup_after_grace())


@router.websocket("/ws/streaming-audio")
async def websocket_streaming_audio(
    websocket: WebSocket,
    session_id: Optional[str] = None,
    meeting_id: Optional[str] = None,
    auth_token: Optional[str] = None,
):
    """
    Real-time streaming transcription with Groq Whisper Large v3.
    Includes heartbeat and force-flush on disconnect.
    """
    try:
        current_user = await _authenticate_websocket(auth_token)
        if not current_user:
            await websocket.close(code=1008, reason="Authentication required")
            return
    except Exception:
        await websocket.close(code=1008, reason="Authentication failed")
        return

    if session_id and not _is_safe_identifier(session_id):
        await websocket.close(code=1008, reason="Invalid session id")
        return
    if meeting_id and not _is_safe_identifier(meeting_id):
        await websocket.close(code=1008, reason="Invalid meeting id")
        return

    await websocket.accept()
    user_email = current_user.email

    # Initialize manager to avoid unbound errors
    manager = None

    # Check if resuming session
    is_resume = False
    if session_id and session_id in streaming_managers:
        existing_ctx = session_context.get(session_id)
        if existing_ctx and existing_ctx.get("user_email") != user_email:
            await websocket.send_json(
                {"type": "error", "code": "SESSION_FORBIDDEN", "message": "Forbidden"}
            )
            await websocket.close(code=1008)
            return
        manager = streaming_managers[session_id]
        is_resume = True
        _cancel_pending_cleanup(session_id)
        logger.info(f"[Streaming] 🔄 Resuming session {session_id}")
    else:
        # Create new session
        session_id = str(uuid.uuid4()) if not session_id else session_id
        session_finalized.discard(session_id)
        is_resume = False
    active_meeting_id = meeting_id or session_id
    if active_meeting_id and not _is_safe_identifier(active_meeting_id):
        await websocket.send_json(
            {
                "type": "error",
                "code": "INVALID_MEETING_ID",
                "message": "Invalid meeting id",
            }
        )
        await websocket.close(code=1008)
        return
    if meeting_id:
        try:
            existing_meeting = await db.get_meeting(active_meeting_id)
            if existing_meeting and not await rbac.can(current_user, "edit", active_meeting_id):
                await websocket.send_json(
                    {
                        "type": "error",
                        "code": "MEETING_FORBIDDEN",
                        "message": "Permission denied for meeting",
                    }
                )
                await websocket.close(code=1008)
                return
        except Exception as authz_err:
            logger.error(f"[Streaming] Meeting authorization failed: {authz_err}")
            await websocket.send_json(
                {
                    "type": "error",
                    "code": "MEETING_AUTH_ERROR",
                    "message": "Failed meeting authorization",
                }
            )
            await websocket.close(code=1011)
            return

    # Audio recorder setup
    audio_recorder = None
    enable_recording = os.getenv("ENABLE_AUDIO_RECORDING", "true").lower() == "true"

    logger.info(
        f"[Streaming] Audio setup: enable_recording={enable_recording}, meeting_id={meeting_id}, session_id={session_id}, active_meeting_id={active_meeting_id}"
    )

    if enable_recording:
        try:
            recorder_key = active_meeting_id
            logger.info(
                f"[Streaming] Attempting to start recorder for key: {recorder_key}"
            )
            audio_recorder = await get_or_create_recorder(recorder_key)
            if audio_recorder:
                logger.info(
                    f"[Streaming] 🎙️ Audio recording active using key: {recorder_key}"
                )
            else:
                logger.error(
                    f"[Streaming] get_or_create_recorder returned None for {recorder_key}"
                )
        except Exception as e:
            logger.error(
                f"[Streaming] Failed to start audio recorder: {e}", exc_info=True
            )

        if not is_resume:
            groq_api_key = (
                (await db.get_api_key("groq", user_email=user_email)) if user_email else ""
            ) or os.getenv("GROQ_API_KEY", "")
            groq_api_key = groq_api_key.strip()
            if not groq_api_key:
                logger.warning(
                    f"[Streaming] No Groq API key resolved for user: {user_email}"
                )
                await websocket.send_json(
                    {
                        "type": "error",
                        "code": "GROQ_KEY_REQUIRED",
                        "message": "Groq API key required. Add it in Settings → Personal Keys (or admin model settings).",
                    }
                )
                try:
                    await stop_recorder(recorder_key)
                except Exception:
                    pass
                await websocket.close()
                return

            manager = StreamingTranscriptionManager(groq_api_key)
            streaming_managers[session_id] = manager
            logger.info(f"[Streaming] ✅ Session {session_id} started (HYBRID mode)")

    try:
        await state_service.ensure_session(
            session_id=session_id,
            user_email=user_email,
            meeting_id=active_meeting_id,
            metadata={"mode": "streaming_ws", "celery_enabled": AUDIO_CELERY_ENABLED},
        )
    except Exception as state_err:
        logger.warning("[Streaming] Failed to initialize session state: %s", state_err)

    # Register active connection
    if session_id not in active_connections:
        active_connections[session_id] = 0
    active_connections[session_id] += 1
    session_context[session_id] = {
        "meeting_id": active_meeting_id,
        "recorder_key": active_meeting_id,
        "user_email": user_email,
    }

    # Heartbeat setup (configurable with safe default).
    # Keep this above common browser timer-throttling windows to avoid false disconnects.
    last_heartbeat = time.time()
    HEARTBEAT_TIMEOUT = float(os.getenv("STREAMING_HEARTBEAT_TIMEOUT_SECONDS", "60"))
    HEARTBEAT_DB_UPDATE_INTERVAL = float(
        os.getenv("STREAMING_HEARTBEAT_DB_UPDATE_SECONDS", "10")
    )
    HEARTBEAT_DB_UPDATE_TIMEOUT = float(
        os.getenv("STREAMING_HEARTBEAT_DB_UPDATE_TIMEOUT_SECONDS", "0.25")
    )
    last_heartbeat_db_touch = 0.0

    async def _touch_session_heartbeat_best_effort():
        nonlocal last_heartbeat_db_touch
        now = time.time()
        if (now - last_heartbeat_db_touch) < HEARTBEAT_DB_UPDATE_INTERVAL:
            return
        last_heartbeat_db_touch = now
        try:
            await asyncio.wait_for(
                state_service.db.touch_recording_session_heartbeat(session_id),
                timeout=HEARTBEAT_DB_UPDATE_TIMEOUT,
            )
        except Exception:
            pass

    async def heartbeat_monitor():
        try:
            while True:
                await asyncio.sleep(5)
                if time.time() - last_heartbeat > HEARTBEAT_TIMEOUT:
                    logger.warning(f"Session {session_id}: Heartbeat timeout, closing")
                    await websocket.close()
                    break
        except Exception:
            pass

    monitor_task = asyncio.create_task(heartbeat_monitor())

    # Send connection confirmation
    await websocket.send_json(
        {
            "type": "connected",
            "session_id": session_id,
            "message": "Groq streaming ready (HYBRID mode)",
            "timestamp": datetime.utcnow().isoformat(),
        }
    )

    # Define callbacks
    async def on_partial(data):
        try:
            await websocket.send_json(
                {
                    "type": "partial",
                    "text": data["text"],
                    "confidence": data["confidence"],
                    "is_stable": data.get("is_stable", False),
                    "timestamp": datetime.utcnow().isoformat(),
                }
            )
        except Exception:
            pass

    async def on_final(data):
        event_ts = datetime.utcnow().isoformat()

        # WebSocket delivery must not be blocked by DB persistence.
        try:
            response = {
                "type": "final",
                "text": data["text"],
                "confidence": data["confidence"],
                "reason": data.get("reason", "unknown"),
                "timestamp": event_ts,
                "audio_start_time": data.get("audio_start_time"),
                "audio_end_time": data.get("audio_end_time"),
                "duration": data.get("duration"),
            }
            if data.get("original_text"):
                response["original_text"] = data["original_text"]
                response["translated"] = data.get("translated", False)
            await websocket.send_json(response)
        except Exception as ws_e:
            logger.error(f"Failed to send final transcript over websocket: {ws_e}")

        # Do not persist live transcript segments on streaming path.
        # Transcript persistence is handled by explicit save-transcript flow.

    async def on_error(message: str, code: Optional[str] = None):
        try:
            error_payload = {
                "type": "error",
                "message": message,
                "timestamp": datetime.utcnow().isoformat(),
            }
            if code:
                error_payload["code"] = code

            await websocket.send_json(error_payload)
        except Exception:
            pass

    # Audio Queue
    audio_queue = asyncio.Queue(maxsize=STREAMING_AUDIO_QUEUE_MAX_CHUNKS)
    dropped_audio_chunks = 0

    async def audio_worker():
        try:
            while True:
                item = await audio_queue.get()
                if item is None:
                    audio_queue.task_done()
                    break

                if isinstance(item, tuple):
                    chunk, ts = item
                else:
                    chunk = item
                    ts = None

                try:
                    # Ensure manager is available
                    current_mgr = manager or streaming_managers.get(session_id)
                    if current_mgr:
                        await current_mgr.process_audio_chunk(
                            audio_data=chunk,
                            client_timestamp=ts,
                            on_partial=on_partial,
                            on_final=on_final,
                            on_error=on_error,
                        )
                except Exception as e:
                    logger.error(f"[Streaming] Worker transcription error: {e}")

                audio_queue.task_done()
        except Exception as e:
            logger.error(f"[Streaming] Audio worker crashed: {e}")

    worker_task = asyncio.create_task(audio_worker())
    explicit_stop = False

    try:
        while True:
            try:
                message = await asyncio.wait_for(
                    websocket.receive(), timeout=HEARTBEAT_TIMEOUT
                )
            except asyncio.TimeoutError:
                logger.warning(
                    f"[Streaming] No message received in {HEARTBEAT_TIMEOUT}s"
                )
                break

            if message.get("type") == "websocket.disconnect":
                logger.info(f"[Streaming] Session {session_id} received disconnect frame")
                break

            if "text" in message:
                try:
                    data = json.loads(message["text"])
                    if data.get("type") == "ping":
                        last_heartbeat = time.time()
                        await _touch_session_heartbeat_best_effort()
                        await websocket.send_json({"type": "pong"})
                        continue
                    if data.get("type") == "stop":
                        logger.info(
                            f"[Streaming] Received explicit stop for session {session_id}"
                        )
                        try:
                            await state_service.mark_stop_requested(session_id)
                        except Exception:
                            pass
                        await websocket.send_json({"type": "stop_ack"})
                        explicit_stop = True
                        break
                except:
                    pass

            if "bytes" in message:
                last_heartbeat = time.time()
                await _touch_session_heartbeat_best_effort()

                message_bytes = message["bytes"]
                timestamp = None
                audio_chunk = message_bytes

                if len(message_bytes) >= 8:
                    try:
                        timestamp_bytes = message_bytes[:8]
                        (timestamp,) = struct.unpack("<d", timestamp_bytes)
                        audio_chunk = message_bytes[8:]
                    except:
                        audio_chunk = message_bytes

                if audio_recorder:
                    saved_chunk_path = await audio_recorder.add_chunk(audio_chunk)
                    if saved_chunk_path:
                        chunk_name = saved_chunk_path.split("/")[-1]
                        idx_match = CHUNK_FILENAME_PATTERN.match(chunk_name)
                        chunk_index = int(idx_match.group(1)) if idx_match else None
                        if chunk_index is not None:
                            should_enqueue_chunk_upload = (
                                AUDIO_CELERY_ENABLED
                                and CHUNK_UPLOAD_VIA_CELERY
                                and STORAGE_TYPE != "gcp"
                            )
                            upload_status = "pending" if should_enqueue_chunk_upload else "uploaded"
                            try:
                                await db.upsert_recording_chunk(
                                    session_id=session_id,
                                    chunk_index=chunk_index,
                                    byte_size=len(audio_chunk),
                                    storage_path=saved_chunk_path,
                                    upload_status=upload_status,
                                )
                                if should_enqueue_chunk_upload:
                                    try:
                                        try:
                                            from ...tasks.audio_pipeline import (
                                                enqueue_upload_chunk_task,
                                            )
                                        except (ImportError, ValueError):
                                            from tasks.audio_pipeline import (
                                                enqueue_upload_chunk_task,
                                            )
                                        upload_task_id = enqueue_upload_chunk_task(
                                            session_id=session_id,
                                            chunk_index=chunk_index,
                                            storage_path=saved_chunk_path,
                                            byte_size=len(audio_chunk),
                                        )
                                        await state_service.db.merge_recording_session_metadata(
                                            session_id,
                                            {
                                                "last_chunk_upload_task_id": upload_task_id,
                                                "chunk_upload_enqueued": True,
                                            },
                                        )
                                    except Exception as upload_enqueue_err:
                                        logger.error(
                                            "[Streaming] Failed to enqueue chunk upload for %s#%s: %s",
                                            session_id,
                                            chunk_index,
                                            upload_enqueue_err,
                                        )
                            except Exception as chunk_track_err:
                                logger.warning(
                                    "[Streaming] Failed tracking chunk %s for session %s: %s",
                                    chunk_name,
                                    session_id,
                                    chunk_track_err,
                                )
                try:
                    audio_queue.put_nowait((audio_chunk, timestamp))
                except asyncio.QueueFull:
                    dropped_audio_chunks += 1
                    if STREAMING_AUDIO_DROP_POLICY == "drop_oldest":
                        try:
                            _ = audio_queue.get_nowait()
                            audio_queue.task_done()
                        except asyncio.QueueEmpty:
                            pass
                        try:
                            audio_queue.put_nowait((audio_chunk, timestamp))
                        except asyncio.QueueFull:
                            pass
                    else:
                        await on_error(
                            "Audio backlog is too high. Please check connection quality.",
                            code="AUDIO_BACKPRESSURE",
                        )
                    if dropped_audio_chunks % 25 == 0:
                        logger.warning(
                            "[Streaming] Dropped %s audio chunks for session %s due to backpressure",
                            dropped_audio_chunks,
                            session_id,
                        )
                        try:
                            await state_service.db.update_recording_session_counters(
                                session_id=session_id, dropped_chunk_delta=25
                            )
                        except Exception:
                            pass

    except WebSocketDisconnect:
        logger.info(f"[Streaming] Session {session_id} disconnected by client")
    except Exception as e:
        logger.error(f"[Streaming] Error in receiver loop {session_id}: {e}")

    finally:
        monitor_task.cancel()

        # Cleanup queues and workers
        await audio_queue.put(None)
        try:
            await asyncio.wait_for(worker_task, timeout=5.0)
        except:
            pass

        # Connection tracking cleanup / deferred finalize for resume support
        if session_id in active_connections:
            active_connections[session_id] -= 1
            if active_connections[session_id] <= 0:
                try:
                    stats = await db.get_recording_chunk_stats(session_id)
                    await db.update_recording_session_counters(
                        session_id=session_id,
                        expected_chunk_count=stats.get("total", 0),
                        finalized_chunk_count=stats.get("uploaded", 0),
                    )
                except Exception:
                    pass
                if explicit_stop:
                    if AUDIO_CELERY_ENABLED:
                        await _finalize_session(
                            session_id, flush=True, process_audio=False
                        )
                        try:
                            try:
                                from ...tasks.audio_pipeline import (
                                    enqueue_finalize_session_task,
                                )
                            except (ImportError, ValueError):
                                from tasks.audio_pipeline import (
                                    enqueue_finalize_session_task,
                                )
                            await state_service.transition(session_id, "finalizing")
                            task_id = enqueue_finalize_session_task(session_id)
                            await state_service.db.merge_recording_session_metadata(
                                session_id,
                                {
                                    "finalize_task_id": task_id,
                                    "finalize_enqueued": True,
                                },
                            )
                        except Exception as enqueue_err:
                            logger.error(
                                "[Streaming] Failed enqueueing celery finalize for %s: %s",
                                session_id,
                                enqueue_err,
                            )
                    else:
                        await _finalize_session(
                            session_id, flush=True, process_audio=True
                        )
                else:
                    logger.info(
                        f"[Streaming] Session {session_id} disconnected; waiting {RESUME_GRACE_SECONDS}s for resume before finalize."
                    )
                    _schedule_deferred_cleanup(session_id)


import tempfile
import shutil


@router.post("/upload-meeting-recording")
async def upload_meeting_recording(
    background_tasks: BackgroundTasks,
    file: UploadFile = File(...),
    title: Optional[str] = Form(None),
    current_user: User = Depends(get_current_user),
):
    """
    Upload and process an audio/video file as a new meeting.
    Directly uploads to Cloud Storage (if configured) and processes in background.
    """
    meeting_id = str(uuid.uuid4())
    meeting_title = title or file.filename or "Untitled Import"

    # 1. Create meeting entry in DB
    await db.save_meeting(
        meeting_id=meeting_id,
        title=meeting_title,
        owner_id=current_user.email if current_user else "default",
        workspace_id="default",
    )

    # 2. Save file temporarily for upload
    original_filename = file.filename or "uploaded_file"
    file_ext = os.path.splitext(original_filename)[1]
    if not file_ext:
        file_ext = ".bin"

    # Create a temp file in /tmp (or system temp)
    with tempfile.NamedTemporaryFile(delete=False, suffix=file_ext) as tmp:
        temp_path = Path(tmp.name)
        async with aiofiles.open(temp_path, "wb") as out_file:
            while content := await file.read(1024 * 1024):
                await out_file.write(content)

    # 3. Upload to Storage (GCP/Local)
    destination_path = f"{meeting_id}/original{file_ext}"
    try:
        success = await StorageService.upload_file(str(temp_path), destination_path)
        if not success:
            raise Exception("Storage upload failed")
    except Exception as e:
        logger.error(f"Failed to upload file to storage: {e}")
        # Clean up temp file
        if temp_path.exists():
            os.unlink(temp_path)
        raise HTTPException(status_code=500, detail=f"Failed to upload file: {str(e)}")

    # 4. Trigger processing (Background Task)
    # We pass the temp_path so processing can use it (optimization),
    # but we also flag that it's already in storage.
    try:
        try:
            from ...services.file_processing import get_file_processor
        except (ImportError, ValueError):
            from services.file_processing import get_file_processor

        processor = get_file_processor(db)

        # We pass the storage path/info to the processor
        # The processor will be responsible for cleaning up the temp file if passed
        background_tasks.add_task(
            processor.process_file,
            meeting_id,
            temp_path,  # Pass local cached copy for speed
            meeting_title,
            file_ext,  # Pass extension to help identify file type
        )
    except ImportError as e:
        logger.error(f"file_processing module import failed: {e}")
        # Attempt to clean up if we fail to schedule task
        if temp_path.exists():
            os.unlink(temp_path)
        raise HTTPException(status_code=500, detail="Processing service unavailable")

    return {
        "meeting_id": meeting_id,
        "status": "processing",
        "message": "File uploaded and processing started",
    }


@router.get("/meetings/{meeting_id}/recording-url")
async def get_meeting_recording_url(
    meeting_id: str, current_user: User = Depends(get_current_user)
):
    """
    Get a secure, time-limited URL for the meeting recording.
    """
    if not await rbac.can(current_user, "view", meeting_id):
        raise HTTPException(status_code=403, detail="Permission denied")

    try:
        preferred_paths = [
            (f"{meeting_id}/recording.wav", "audio/wav"),
            (f"{meeting_id}/recording.opus", "audio/ogg"),
            (f"{meeting_id}/recording.m4a", "audio/mp4"),
        ]

        STORAGE_TYPE = os.getenv("STORAGE_TYPE", "local").lower()
        selected_path = None
        selected_mime = None
        selected_format = None

        # 1) Check configured storage first
        for path, mime in preferred_paths:
            if await StorageService.check_file_exists(path):
                selected_path = path
                selected_mime = mime
                selected_format = path.split(".")[-1]
                break

        # 2) Cross-check fallback storage mode if primary mode missed
        if not selected_path and STORAGE_TYPE == "gcp":
            for path, mime in preferred_paths:
                if await StorageService._check_local_exists(path):
                    return {
                        "url": f"/audio/{path}",
                        "expiration": 3600,
                        "format": path.split(".")[-1],
                        "mime_type": mime,
                        "filename": f"recording-{meeting_id}.{path.split('.')[-1]}",
                    }
        elif not selected_path and STORAGE_TYPE != "gcp":
            for path, mime in preferred_paths:
                if await StorageService._check_gcp_exists(path):
                    fmt = path.split(".")[-1]
                    url = await StorageService._generate_gcp_signed_url(
                        path,
                        3600,
                        download_filename=f"recording-{meeting_id}.{fmt}",
                    )
                    return {
                        "url": url,
                        "expiration": 3600,
                        "format": fmt,
                        "mime_type": mime,
                        "filename": f"recording-{meeting_id}.{fmt}",
                    }

        if not selected_path:
            raise HTTPException(status_code=404, detail="Recording not found")

        download_filename = f"recording-{meeting_id}.{selected_format}"
        url = await StorageService.generate_signed_url(
            selected_path,
            3600,
            download_filename=download_filename,
        )
        if not url:
            raise HTTPException(status_code=404, detail="Failed to generate URL")

        return {
            "url": url,
            "expiration": 3600,
            "format": selected_format,
            "mime_type": selected_mime,
            "filename": download_filename,
        }
    except HTTPException:
        raise
    except Exception as e:
        logger.error(f"Failed to get recording URL: {e}")
        raise HTTPException(status_code=500, detail="Failed to generate recording URL")


@router.get("/sessions/{session_id}/pipeline-status")
async def get_pipeline_session_status(
    session_id: str, current_user: User = Depends(get_current_user)
):
    if not _is_safe_identifier(session_id):
        raise HTTPException(status_code=400, detail="Invalid session id")

    session = await db.get_recording_session(session_id)
    if not session:
        raise HTTPException(status_code=404, detail="Session not found")

    # Owner can always inspect; otherwise fallback to meeting-level view permission.
    if session.get("user_email") != current_user.email:
        meeting_id = session.get("meeting_id")
        if not meeting_id or not await rbac.can(current_user, "view", meeting_id):
            raise HTTPException(status_code=403, detail="Permission denied")

    chunk_stats = await db.get_recording_chunk_stats(session_id)

    return {
        "session_id": session["session_id"],
        "meeting_id": session.get("meeting_id"),
        "status": session.get("status"),
        "started_at": session.get("started_at"),
        "stop_requested_at": session.get("stop_requested_at"),
        "stopped_at": session.get("stopped_at"),
        "finalized_at": session.get("finalized_at"),
        "expected_chunk_count": session.get("expected_chunk_count", 0),
        "finalized_chunk_count": session.get("finalized_chunk_count", 0),
        "dropped_chunk_count": session.get("dropped_chunk_count", 0),
        "idempotency_finalize_key": session.get("idempotency_finalize_key"),
        "error_code": session.get("error_code"),
        "error_message": session.get("error_message"),
        "metadata": session.get("metadata") or {},
        "chunk_stats": chunk_stats,
        "updated_at": session.get("updated_at"),
    }


@router.post("/sessions/{session_id}/retry-finalize")
async def retry_pipeline_finalize(
    session_id: str, current_user: User = Depends(get_current_user)
):
    if not _is_safe_identifier(session_id):
        raise HTTPException(status_code=400, detail="Invalid session id")

    session = await db.get_recording_session(session_id)
    if not session:
        raise HTTPException(status_code=404, detail="Session not found")

    if session.get("user_email") != current_user.email:
        meeting_id = session.get("meeting_id")
        if not meeting_id or not await rbac.can(current_user, "edit", meeting_id):
            raise HTTPException(status_code=403, detail="Permission denied")

    if not AUDIO_CELERY_ENABLED:
        raise HTTPException(
            status_code=400, detail="Celery pipeline is disabled in this environment"
        )

    try:
        try:
            from ...tasks.audio_pipeline import enqueue_finalize_session_task
        except (ImportError, ValueError):
            from tasks.audio_pipeline import enqueue_finalize_session_task

        await state_service.transition(session_id, "finalizing")
        task_id = enqueue_finalize_session_task(session_id)
        await state_service.db.merge_recording_session_metadata(
            session_id, {"finalize_task_id": task_id, "finalize_enqueued": True}
        )
        return {"session_id": session_id, "enqueued": True, "task_id": task_id}
    except Exception as exc:
        raise HTTPException(status_code=500, detail=f"Failed to enqueue finalize: {exc}")


@router.post("/sessions/reconcile")
async def reconcile_pipeline_sessions(current_user: User = Depends(get_current_user)):
    if current_user.email != "gagan@appointy.com":
        raise HTTPException(status_code=403, detail="Admin access required")

    try:
        try:
            from ...services.audio.session_reconciler import AudioSessionReconciler
        except (ImportError, ValueError):
            from services.audio.session_reconciler import AudioSessionReconciler

        reconciler = AudioSessionReconciler()
        await reconciler.reconcile_once()
        return {"status": "ok"}
    except Exception as exc:
        raise HTTPException(status_code=500, detail=f"Reconcile failed: {exc}")
