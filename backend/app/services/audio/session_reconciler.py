import asyncio
import logging
import os
from typing import Optional

try:
    from ...db import DatabaseManager
except (ImportError, ValueError):
    from db import DatabaseManager

logger = logging.getLogger(__name__)


class AudioSessionReconciler:
    """
    Background reconciler for sessions that remain in transitional states.
    Phase 2 foundation: detect and nudge stale sessions into finalize flow.
    """

    def __init__(self):
        self.db = DatabaseManager()
        self.interval_seconds = int(
            os.getenv("AUDIO_SESSION_RECONCILER_INTERVAL_SECONDS", "60")
        )
        self.stale_after_minutes = int(
            os.getenv("AUDIO_SESSION_STALE_AFTER_MINUTES", "10")
        )
        self.enabled = (
            os.getenv("AUDIO_SESSION_RECONCILER_ENABLED", "true").lower() == "true"
        )
        self.celery_enabled = os.getenv("AUDIO_CELERY_ENABLED", "false").lower() == "true"
        self._task: Optional[asyncio.Task] = None
        self._stop_event = asyncio.Event()

    def start(self):
        if not self.enabled:
            logger.info("[AudioReconciler] Disabled by env")
            return
        if self._task and not self._task.done():
            return
        self._stop_event.clear()
        self._task = asyncio.create_task(self._run())
        logger.info("[AudioReconciler] Started")

    async def stop(self):
        self._stop_event.set()
        if self._task and not self._task.done():
            self._task.cancel()
            try:
                await self._task
            except asyncio.CancelledError:
                pass
        self._task = None
        logger.info("[AudioReconciler] Stopped")

    async def _run(self):
        while not self._stop_event.is_set():
            try:
                await self.reconcile_once()
            except Exception as exc:
                logger.error("[AudioReconciler] Loop error: %s", exc, exc_info=True)
            await asyncio.sleep(self.interval_seconds)

    async def reconcile_once(self):
        stale = await self.db.get_stale_recording_sessions(
            statuses=["stopping_requested", "uploading_chunks", "finalizing", "postprocessing"],
            stale_after_minutes=self.stale_after_minutes,
            limit=100,
        )
        if not stale:
            return

        logger.warning("[AudioReconciler] Found %s stale sessions", len(stale))
        for session in stale:
            session_id = session["session_id"]
            status = session["status"]
            try:
                if self.celery_enabled:
                    try:
                        from ...tasks.audio_pipeline import (
                            enqueue_finalize_session_task,
                        )
                    except (ImportError, ValueError):
                        from tasks.audio_pipeline import enqueue_finalize_session_task

                    task_id = enqueue_finalize_session_task(session_id)
                    await self.db.merge_recording_session_metadata(
                        session_id,
                        {
                            "reconcile_requeued": True,
                            "reconcile_finalize_task_id": task_id,
                        },
                    )
                    logger.info(
                        "[AudioReconciler] Requeued finalize task for stale session %s (%s)",
                        session_id,
                        status,
                    )
                else:
                    logger.warning(
                        "[AudioReconciler] Session %s is stale in %s but celery is disabled",
                        session_id,
                        status,
                    )
            except Exception as exc:
                logger.error(
                    "[AudioReconciler] Failed reconciling session %s: %s",
                    session_id,
                    exc,
                )
