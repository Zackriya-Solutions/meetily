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
from typing import Optional, Dict, Any, List
import uuid
import logging
import time
import os
import struct
import json
import asyncio
import re
from collections import deque
from datetime import datetime, timedelta
from pathlib import Path
import aiofiles
import httpx

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
session_runtime_stats = {}

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
STREAMING_METRICS_LOG_INTERVAL_SECONDS = int(
    os.getenv("STREAMING_METRICS_LOG_INTERVAL_SECONDS", "30")
)
STREAMING_RECONNECT_STORM_WINDOW_SECONDS = int(
    os.getenv("STREAMING_RECONNECT_STORM_WINDOW_SECONDS", "120")
)
STREAMING_RECONNECT_STORM_THRESHOLD = int(
    os.getenv("STREAMING_RECONNECT_STORM_THRESHOLD", "6")
)
STREAMING_BACKPRESSURE_CLOSE_AFTER_DROPS = int(
    os.getenv("STREAMING_BACKPRESSURE_CLOSE_AFTER_DROPS", "300")
)
STREAMING_ALERT_HISTORY_LIMIT = int(
    os.getenv("STREAMING_ALERT_HISTORY_LIMIT", "100")
)
STREAMING_ALERT_COOLDOWN_SECONDS = int(
    os.getenv("STREAMING_ALERT_COOLDOWN_SECONDS", "45")
)
STREAMING_ALERT_WEBHOOK_URL = os.getenv("STREAMING_ALERT_WEBHOOK_URL", "").strip()
STREAMING_ALERT_WEBHOOK_TIMEOUT_SECONDS = float(
    os.getenv("STREAMING_ALERT_WEBHOOK_TIMEOUT_SECONDS", "3.0")
)
STREAMING_SLO_DEFAULT_LOOKBACK_HOURS = int(
    os.getenv("STREAMING_SLO_DEFAULT_LOOKBACK_HOURS", "24")
)
STREAMING_SLO_TARGET_SECONDS = float(os.getenv("STREAMING_SLO_TARGET_SECONDS", "8.0"))
STREAMING_SLO_MAX_SECONDS = float(os.getenv("STREAMING_SLO_MAX_SECONDS", "10.0"))

state_service = get_audio_pipeline_state_service()


def _ensure_runtime_stats(session_id: str) -> Dict[str, Any]:
    stats = session_runtime_stats.get(session_id)
    if not stats:
        stats = {
            "created_at": datetime.utcnow().isoformat(),
            "connection_count": 0,
            "resume_count": 0,
            "resume_events": deque(maxlen=50),
            "reconnect_storm_detected": False,
            "messages_received": 0,
            "audio_frames_received": 0,
            "dropped_audio_chunks": 0,
            "consecutive_dropped_audio_chunks": 0,
            "max_audio_queue_depth": 0,
            "backpressure_close_triggered": False,
            "last_disconnect_at": None,
            "last_update_at": datetime.utcnow().isoformat(),
            "last_warning": None,
            "alert_history": deque(maxlen=STREAMING_ALERT_HISTORY_LIMIT),
            "alert_counts": {},
            "alert_last_emitted_at": {},
        }
        session_runtime_stats[session_id] = stats
    return stats


def _sanitize_runtime_for_json(runtime: Dict[str, Any]) -> Dict[str, Any]:
    payload: Dict[str, Any] = dict(runtime or {})
    if isinstance(payload.get("resume_events"), deque):
        payload["recent_resume_events_count"] = len(payload["resume_events"])
        payload.pop("resume_events", None)
    if isinstance(payload.get("alert_history"), deque):
        payload["alert_history"] = list(payload["alert_history"])
    return payload


def _normalize_metadata(session_row: Optional[Dict[str, Any]]) -> Dict[str, Any]:
    if not session_row:
        return {}
    metadata = session_row.get("metadata")
    if metadata is None:
        return {}
    if isinstance(metadata, dict):
        return metadata
    if isinstance(metadata, str):
        try:
            parsed = json.loads(metadata)
            return parsed if isinstance(parsed, dict) else {}
        except Exception:
            return {}
    return {}


def _build_streaming_slo_snapshot(
    runtime_stats: Optional[Dict[str, Any]],
    manager_stats: Optional[Dict[str, Any]],
) -> Dict[str, Any]:
    runtime = runtime_stats or {}
    manager = manager_stats or {}
    avg_finalize_latency = manager.get("avg_segment_finalize_latency_seconds")
    first_stable_latency = manager.get("first_stable_emit_latency_seconds")
    stable_segments = int(manager.get("stable_segments") or 0)
    volatile_segments = int(manager.get("volatile_segments") or 0)
    corrections = int(manager.get("correction_events") or 0)
    drifts = int(manager.get("semantic_drift_events") or 0)
    correction_rate = (
        float(corrections) / float(stable_segments + volatile_segments)
        if (stable_segments + volatile_segments) > 0
        else 0.0
    )
    drift_rate = (
        float(drifts) / float(stable_segments)
        if stable_segments > 0
        else 0.0
    )
    return {
        "captured_at": datetime.utcnow().isoformat(),
        "first_stable_emit_latency_seconds": first_stable_latency,
        "avg_segment_finalize_latency_seconds": avg_finalize_latency,
        "slo_target_seconds": STREAMING_SLO_TARGET_SECONDS,
        "slo_max_seconds": STREAMING_SLO_MAX_SECONDS,
        "stable_segments": stable_segments,
        "volatile_segments": volatile_segments,
        "correction_events": corrections,
        "semantic_drift_events": drifts,
        "correction_rate": round(correction_rate, 4),
        "drift_rate": round(drift_rate, 4),
        "dropped_audio_chunks": int(runtime.get("dropped_audio_chunks") or 0),
        "max_audio_queue_depth": int(runtime.get("max_audio_queue_depth") or 0),
        "reconnect_storm_detected": bool(runtime.get("reconnect_storm_detected", False)),
        "backpressure_close_triggered": bool(
            runtime.get("backpressure_close_triggered", False)
        ),
        "health": {
            "latency_degraded": bool(
                (avg_finalize_latency is not None and avg_finalize_latency > STREAMING_SLO_MAX_SECONDS)
                or (first_stable_latency is not None and first_stable_latency > STREAMING_SLO_MAX_SECONDS)
            ),
            "stability_degraded": bool(
                correction_rate > 0.25
                or drift_rate > 0.30
                or volatile_segments > stable_segments
            ),
            "transport_degraded": bool(
                bool(runtime.get("reconnect_storm_detected", False))
                or bool(runtime.get("backpressure_close_triggered", False))
            ),
        },
    }


async def _persist_runtime_snapshot(
    session_id: str,
    runtime_stats: Optional[Dict[str, Any]],
    manager_stats: Optional[Dict[str, Any]],
) -> None:
    try:
        runtime_payload = _sanitize_runtime_for_json(runtime_stats or {})
        slo_snapshot = _build_streaming_slo_snapshot(runtime_stats, manager_stats)
        await state_service.db.merge_recording_session_metadata(
            session_id,
            {
                "streaming_runtime": runtime_payload,
                "streaming_slo": slo_snapshot,
            },
        )
    except Exception as exc:
        logger.debug("[Streaming] Failed persisting runtime snapshot for %s: %s", session_id, exc)


async def _route_streaming_alert(alert_event: Dict[str, Any]) -> None:
    if not STREAMING_ALERT_WEBHOOK_URL:
        return
    try:
        async with httpx.AsyncClient(timeout=STREAMING_ALERT_WEBHOOK_TIMEOUT_SECONDS) as client:
            await client.post(STREAMING_ALERT_WEBHOOK_URL, json=alert_event)
    except Exception as exc:
        logger.warning("[StreamingAlert] Webhook route failed: %s", exc)


async def _emit_streaming_alert(
    session_id: str,
    alert_type: str,
    severity: str,
    message: str,
    details: Optional[Dict[str, Any]] = None,
) -> None:
    stats = _ensure_runtime_stats(session_id)
    now_ts = time.time()
    last_emit_map: Dict[str, float] = stats.setdefault("alert_last_emitted_at", {})
    last_ts = float(last_emit_map.get(alert_type) or 0.0)
    if (now_ts - last_ts) < STREAMING_ALERT_COOLDOWN_SECONDS:
        return

    event = {
        "type": alert_type,
        "severity": severity,
        "message": message,
        "timestamp": datetime.utcnow().isoformat(),
        "session_id": session_id,
        "meeting_id": (session_context.get(session_id) or {}).get("meeting_id"),
        "user_email": (session_context.get(session_id) or {}).get("user_email"),
        "details": details or {},
    }
    last_emit_map[alert_type] = now_ts
    stats["last_warning"] = message
    stats["alert_history"].append(event)
    alert_counts = stats.setdefault("alert_counts", {})
    alert_counts[alert_type] = int(alert_counts.get(alert_type, 0)) + 1
    stats["last_update_at"] = datetime.utcnow().isoformat()

    logger.warning("[StreamingAlert] %s", event)

    try:
        session_row = await db.get_recording_session(session_id)
        metadata = _normalize_metadata(session_row)
        alerts = metadata.get("streaming_alerts")
        if not isinstance(alerts, list):
            alerts = []
        alerts.append(event)
        alerts = alerts[-STREAMING_ALERT_HISTORY_LIMIT:]
        await state_service.db.merge_recording_session_metadata(
            session_id,
            {
                "streaming_alerts": alerts,
                "streaming_alert_counts": alert_counts,
                "last_streaming_alert_at": event["timestamp"],
            },
        )
    except Exception as exc:
        logger.debug("[Streaming] Failed to persist alert for %s: %s", session_id, exc)

    await _route_streaming_alert(event)


def _mark_runtime_resume(session_id: str) -> bool:
    now = time.time()
    stats = _ensure_runtime_stats(session_id)
    stats["resume_count"] += 1
    resume_events = stats["resume_events"]
    resume_events.append(now)
    window_start = now - STREAMING_RECONNECT_STORM_WINDOW_SECONDS
    while resume_events and resume_events[0] < window_start:
        resume_events.popleft()
    if len(resume_events) >= STREAMING_RECONNECT_STORM_THRESHOLD:
        stats["reconnect_storm_detected"] = True
        stats["last_warning"] = (
            f"Reconnect storm: {len(resume_events)} resumes in "
            f"{STREAMING_RECONNECT_STORM_WINDOW_SECONDS}s"
        )
        logger.warning(
            "[Streaming] Reconnect storm detected for %s: %s resumes in %ss",
            session_id,
            len(resume_events),
            STREAMING_RECONNECT_STORM_WINDOW_SECONDS,
        )
        stats["last_update_at"] = datetime.utcnow().isoformat()
        return True
    stats["last_update_at"] = datetime.utcnow().isoformat()
    return False


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
        runtime_snapshot = session_runtime_stats.get(session_id, {})
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

        manager_stats = mgr.get_stats() if mgr else {}
        await _persist_runtime_snapshot(
            session_id=session_id,
            runtime_stats=runtime_snapshot,
            manager_stats=manager_stats,
        )

        if mgr:
            try:
                mgr.cleanup()
            except Exception:
                pass
            streaming_managers.pop(session_id, None)

        active_connections.pop(session_id, None)
        session_context.pop(session_id, None)
        session_runtime_stats.pop(session_id, None)
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
        reconnect_storm = _mark_runtime_resume(session_id)
        if reconnect_storm:
            asyncio.create_task(
                _emit_streaming_alert(
                    session_id=session_id,
                    alert_type="reconnect_storm",
                    severity="warning",
                    message=(
                        f"Reconnect storm detected: >= {STREAMING_RECONNECT_STORM_THRESHOLD} "
                        f"resumes in {STREAMING_RECONNECT_STORM_WINDOW_SECONDS}s"
                    ),
                    details={"resume_count_window": STREAMING_RECONNECT_STORM_THRESHOLD},
                )
            )
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
    runtime_stats = _ensure_runtime_stats(session_id)
    runtime_stats["connection_count"] = active_connections[session_id]
    runtime_stats["last_update_at"] = datetime.utcnow().isoformat()
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
                "stability_score": data.get("stability_score"),
                "stability_class": data.get("stability_class", "stable"),
                "segment_finalize_latency_seconds": data.get(
                    "segment_finalize_latency_seconds"
                ),
                "boundary_score": data.get("boundary_score"),
            }
            if data.get("stability_breakdown"):
                response["stability_breakdown"] = data.get("stability_breakdown")
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
                        runtime_stats["max_audio_queue_depth"] = max(
                            runtime_stats.get("max_audio_queue_depth", 0), audio_queue.qsize()
                        )
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
    last_snapshot_persist_at = 0.0

    async def metrics_monitor():
        nonlocal last_snapshot_persist_at
        try:
            while True:
                await asyncio.sleep(STREAMING_METRICS_LOG_INTERVAL_SECONDS)
                current_mgr = manager or streaming_managers.get(session_id)
                mgr_stats = current_mgr.get_stats() if current_mgr else {}
                runtime_stats["connection_count"] = active_connections.get(session_id, 0)
                runtime_stats["queue_depth"] = audio_queue.qsize()
                runtime_stats["last_update_at"] = datetime.utcnow().isoformat()
                avg_finalize_latency = mgr_stats.get("avg_segment_finalize_latency_seconds")
                first_emit_latency = mgr_stats.get("first_stable_emit_latency_seconds")
                stable_segments = int(mgr_stats.get("stable_segments") or 0)
                volatile_segments = int(mgr_stats.get("volatile_segments") or 0)
                corrections = int(mgr_stats.get("correction_events") or 0)
                correction_rate = (
                    float(corrections) / float(stable_segments + volatile_segments)
                    if (stable_segments + volatile_segments) > 0
                    else 0.0
                )
                logger.info(
                    "[StreamingMetrics] session=%s conn=%s queue=%s dropped=%s resumes=%s storm=%s stable=%s volatile=%s drift=%s corrections=%s",
                    session_id,
                    runtime_stats.get("connection_count", 0),
                    runtime_stats.get("queue_depth", 0),
                    runtime_stats.get("dropped_audio_chunks", 0),
                    runtime_stats.get("resume_count", 0),
                    runtime_stats.get("reconnect_storm_detected", False),
                    mgr_stats.get("stable_segments"),
                    mgr_stats.get("volatile_segments"),
                    mgr_stats.get("semantic_drift_events"),
                    mgr_stats.get("correction_events"),
                )
                if avg_finalize_latency is not None and avg_finalize_latency > STREAMING_SLO_MAX_SECONDS:
                    await _emit_streaming_alert(
                        session_id=session_id,
                        alert_type="slo_finalize_latency",
                        severity="warning",
                        message=(
                            f"Average segment finalization latency {avg_finalize_latency:.2f}s "
                            f"exceeds {STREAMING_SLO_MAX_SECONDS:.1f}s"
                        ),
                        details={"avg_segment_finalize_latency_seconds": avg_finalize_latency},
                    )
                if first_emit_latency is not None and first_emit_latency > STREAMING_SLO_MAX_SECONDS:
                    await _emit_streaming_alert(
                        session_id=session_id,
                        alert_type="slo_first_emit_latency",
                        severity="warning",
                        message=(
                            f"First stable segment latency {first_emit_latency:.2f}s "
                            f"exceeds {STREAMING_SLO_MAX_SECONDS:.1f}s"
                        ),
                        details={"first_stable_emit_latency_seconds": first_emit_latency},
                    )
                if (stable_segments + volatile_segments) >= 8 and correction_rate > 0.25:
                    await _emit_streaming_alert(
                        session_id=session_id,
                        alert_type="transcript_instability",
                        severity="warning",
                        message=(
                            f"Transcript instability detected (correction_rate={correction_rate:.2f})"
                        ),
                        details={
                            "correction_rate": correction_rate,
                            "stable_segments": stable_segments,
                            "volatile_segments": volatile_segments,
                        },
                    )
                now_ts = time.time()
                if (now_ts - last_snapshot_persist_at) >= 60:
                    await _persist_runtime_snapshot(
                        session_id=session_id,
                        runtime_stats=runtime_stats,
                        manager_stats=mgr_stats,
                    )
                    last_snapshot_persist_at = now_ts
        except asyncio.CancelledError:
            return
        except Exception as e:
            logger.debug("[Streaming] metrics monitor ended for %s: %s", session_id, e)

    metrics_task = asyncio.create_task(metrics_monitor())

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
                runtime_stats["messages_received"] = runtime_stats.get("messages_received", 0) + 1
                runtime_stats["audio_frames_received"] = runtime_stats.get("audio_frames_received", 0) + 1

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
                    runtime_stats["consecutive_dropped_audio_chunks"] = 0
                    runtime_stats["max_audio_queue_depth"] = max(
                        runtime_stats.get("max_audio_queue_depth", 0), audio_queue.qsize()
                    )
                except asyncio.QueueFull:
                    dropped_audio_chunks += 1
                    runtime_stats["dropped_audio_chunks"] = dropped_audio_chunks
                    runtime_stats["consecutive_dropped_audio_chunks"] = (
                        runtime_stats.get("consecutive_dropped_audio_chunks", 0) + 1
                    )
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
                    if (
                        STREAMING_BACKPRESSURE_CLOSE_AFTER_DROPS > 0
                        and runtime_stats.get("consecutive_dropped_audio_chunks", 0)
                        >= STREAMING_BACKPRESSURE_CLOSE_AFTER_DROPS
                    ):
                        runtime_stats["backpressure_close_triggered"] = True
                        runtime_stats["last_warning"] = (
                            "Consecutive dropped chunks exceeded threshold"
                        )
                        await on_error(
                            "Connection unstable: too many dropped audio chunks. Please rejoin the meeting.",
                            code="AUDIO_BACKPRESSURE_HARD_LIMIT",
                        )
                        logger.warning(
                            "[Streaming] Closing %s due to sustained backpressure (consecutive_drops=%s)",
                            session_id,
                            runtime_stats.get("consecutive_dropped_audio_chunks", 0),
                        )
                        await _emit_streaming_alert(
                            session_id=session_id,
                            alert_type="backpressure_hard_limit",
                            severity="critical",
                            message=(
                                "Session closed due to sustained backpressure and dropped audio chunks."
                            ),
                            details={
                                "consecutive_dropped_audio_chunks": runtime_stats.get(
                                    "consecutive_dropped_audio_chunks", 0
                                ),
                                "drop_threshold": STREAMING_BACKPRESSURE_CLOSE_AFTER_DROPS,
                            },
                        )
                        break
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
        metrics_task.cancel()
        runtime_stats["last_disconnect_at"] = datetime.utcnow().isoformat()
        runtime_stats["last_update_at"] = datetime.utcnow().isoformat()

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


@router.get("/sessions/{session_id}/streaming-health")
async def get_streaming_session_health(
    session_id: str, current_user: User = Depends(get_current_user)
):
    if not _is_safe_identifier(session_id):
        raise HTTPException(status_code=400, detail="Invalid session id")

    session = await db.get_recording_session(session_id)
    if not session:
        raise HTTPException(status_code=404, detail="Session not found")

    if session.get("user_email") != current_user.email:
        meeting_id = session.get("meeting_id")
        if not meeting_id or not await rbac.can(current_user, "view", meeting_id):
            raise HTTPException(status_code=403, detail="Permission denied")

    runtime = _sanitize_runtime_for_json(session_runtime_stats.get(session_id, {}))

    mgr = streaming_managers.get(session_id)
    manager_stats = mgr.get_stats() if mgr else {}

    return {
        "session_id": session_id,
        "meeting_id": session.get("meeting_id"),
        "session_status": session.get("status"),
        "active_connections": active_connections.get(session_id, 0),
        "runtime": runtime,
        "manager_stats": manager_stats,
    }


@router.get("/streaming/slo-report")
async def get_streaming_slo_report(
    lookback_hours: int = STREAMING_SLO_DEFAULT_LOOKBACK_HOURS,
    limit: int = 500,
    user_email: Optional[str] = None,
    current_user: User = Depends(get_current_user),
):
    lookback_hours = max(1, min(168, int(lookback_hours)))
    limit = max(10, min(1000, int(limit)))
    is_admin = current_user.email == "gagan@appointy.com"

    target_user = current_user.email
    if user_email and user_email != current_user.email:
        if not is_admin:
            raise HTTPException(status_code=403, detail="Admin access required for user filter")
        target_user = user_email
    elif user_email:
        target_user = user_email

    started_after = datetime.utcnow() - timedelta(hours=lookback_hours)
    sessions = await db.list_recording_sessions_since(
        started_after=started_after,
        user_email=target_user if not is_admin or user_email else None,
        limit=limit,
    )

    summaries: List[Dict[str, Any]] = []
    for session in sessions:
        metadata = _normalize_metadata(session)
        streaming_slo = metadata.get("streaming_slo")
        if not isinstance(streaming_slo, dict):
            continue
        summaries.append(
            {
                "session_id": session.get("session_id"),
                "meeting_id": session.get("meeting_id"),
                "user_email": session.get("user_email"),
                "status": session.get("status"),
                "started_at": (
                    session.get("started_at").isoformat()
                    if session.get("started_at")
                    else None
                ),
                "streaming_slo": streaming_slo,
                "alerts_count": len(metadata.get("streaming_alerts") or []),
                "alert_counts": metadata.get("streaming_alert_counts") or {},
            }
        )

    total = len(summaries)
    degraded_latency = 0
    degraded_stability = 0
    degraded_transport = 0
    first_emit_values: List[float] = []
    finalize_values: List[float] = []
    for item in summaries:
        slo = item["streaming_slo"]
        health = slo.get("health") or {}
        if health.get("latency_degraded"):
            degraded_latency += 1
        if health.get("stability_degraded"):
            degraded_stability += 1
        if health.get("transport_degraded"):
            degraded_transport += 1
        first_val = slo.get("first_stable_emit_latency_seconds")
        if isinstance(first_val, (int, float)):
            first_emit_values.append(float(first_val))
        fin_val = slo.get("avg_segment_finalize_latency_seconds")
        if isinstance(fin_val, (int, float)):
            finalize_values.append(float(fin_val))

    def _avg(values: List[float]) -> Optional[float]:
        if not values:
            return None
        return round(sum(values) / len(values), 4)

    return {
        "scope": {
            "lookback_hours": lookback_hours,
            "started_after": started_after.isoformat(),
            "user_filter": target_user if (not is_admin or user_email) else "all",
            "session_limit": limit,
        },
        "summary": {
            "sessions_with_slo": total,
            "latency_degraded_sessions": degraded_latency,
            "stability_degraded_sessions": degraded_stability,
            "transport_degraded_sessions": degraded_transport,
            "avg_first_stable_emit_latency_seconds": _avg(first_emit_values),
            "avg_segment_finalize_latency_seconds": _avg(finalize_values),
            "slo_target_seconds": STREAMING_SLO_TARGET_SECONDS,
            "slo_max_seconds": STREAMING_SLO_MAX_SECONDS,
        },
        "sessions": summaries,
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
