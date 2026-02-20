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
from datetime import datetime
from pathlib import Path
import aiofiles

try:
    from ..deps import get_current_user
    from ...schemas.user import User
    from ...db import DatabaseManager
    from ...core.rbac import RBAC
    from ...services.audio.manager import StreamingTranscriptionManager
    from ...services.audio.recorder import get_or_create_recorder, stop_recorder
    from ...services.audio.post_recording import get_post_recording_service
    from ...services.storage import StorageService
except (ImportError, ValueError):
    from api.deps import get_current_user
    from schemas.user import User
    from db import DatabaseManager
    from core.rbac import RBAC
    from services.audio.manager import StreamingTranscriptionManager
    from services.audio.recorder import get_or_create_recorder, stop_recorder
    from services.audio.post_recording import get_post_recording_service
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

RESUME_GRACE_SECONDS = int(os.getenv("STREAMING_RESUME_GRACE_SECONDS", "45"))


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
        await stop_recorder(recorder_key)
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
        logger.warning(f"[Streaming] Recorder finalize failed for {recorder_key}: {e}")

    if mgr:
        try:
            mgr.cleanup()
        except Exception:
            pass
        streaming_managers.pop(session_id, None)

    active_connections.pop(session_id, None)
    session_context.pop(session_id, None)
    _cancel_pending_cleanup(session_id)


def _schedule_deferred_cleanup(session_id: str):
    _cancel_pending_cleanup(session_id)

    async def _cleanup_after_grace():
        try:
            await asyncio.sleep(RESUME_GRACE_SECONDS)
            if active_connections.get(session_id, 0) <= 0:
                logger.info(
                    f"[Streaming] Resume grace expired for {session_id}; finalizing session."
                )
                await _finalize_session(
                    session_id, flush=False, process_audio=False
                )
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
    user_email: Optional[str] = None,
    meeting_id: Optional[str] = None,
):
    """
    Real-time streaming transcription with Groq Whisper Large v3.
    Includes heartbeat and force-flush on disconnect.
    """
    await websocket.accept()

    # Initialize manager to avoid unbound errors
    manager = None

    # Check if resuming session
    is_resume = False
    if session_id and session_id in streaming_managers:
        manager = streaming_managers[session_id]
        is_resume = True
        _cancel_pending_cleanup(session_id)
        logger.info(f"[Streaming] 🔄 Resuming session {session_id}")
    else:
        # Create new session
        session_id = str(uuid.uuid4()) if not session_id else session_id
        is_resume = False
    active_meeting_id = meeting_id or session_id

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
                await db.get_user_api_key(user_email, "groq") if user_email else None
            )
            if not groq_api_key:
                logger.warning(
                    f"[Streaming] No personal Groq API key for user: {user_email}"
                )
                await websocket.send_json(
                    {
                        "type": "error",
                        "code": "GROQ_KEY_REQUIRED",
                        "message": "Groq API key required. Please add your Groq API key in Settings → Personal Keys.",
                    }
                )
                await websocket.close()
                return

            manager = StreamingTranscriptionManager(groq_api_key)
            streaming_managers[session_id] = manager
            logger.info(f"[Streaming] ✅ Session {session_id} started (HYBRID mode)")

    # Register active connection
    if session_id not in active_connections:
        active_connections[session_id] = 0
    active_connections[session_id] += 1
    session_context[session_id] = {
        "meeting_id": active_meeting_id,
        "recorder_key": active_meeting_id,
        "user_email": user_email,
    }

    # Heartbeat Setup
    last_heartbeat = time.time()
    HEARTBEAT_TIMEOUT = 15.0  # seconds

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
    audio_queue = asyncio.Queue()

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

            if "text" in message:
                try:
                    data = json.loads(message["text"])
                    if data.get("type") == "ping":
                        last_heartbeat = time.time()
                        await websocket.send_json({"type": "pong"})
                        continue
                    if data.get("type") == "stop":
                        logger.info(
                            f"[Streaming] Received explicit stop for session {session_id}"
                        )
                        explicit_stop = True
                        break
                except:
                    pass

            if "bytes" in message:
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
                    await audio_recorder.add_chunk(audio_chunk)

                await audio_queue.put((audio_chunk, timestamp))

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
                if explicit_stop:
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
