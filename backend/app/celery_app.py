import os
import importlib.util
import importlib
from celery import Celery


def _build_broker_url() -> str:
    return os.getenv("CELERY_BROKER_URL", "redis://localhost:6379/0")


def _build_result_backend() -> str:
    return os.getenv("CELERY_RESULT_BACKEND", _build_broker_url())


celery_app = Celery(
    "meeting_copilot",
    broker=_build_broker_url(),
    backend=_build_result_backend(),
)

celery_app.conf.update(
    task_serializer="json",
    result_serializer="json",
    accept_content=["json"],
    timezone="UTC",
    enable_utc=True,
    task_acks_late=True,
    task_reject_on_worker_lost=True,
    worker_prefetch_multiplier=1,
    task_track_started=True,
)

task_packages = []
try:
    if importlib.util.find_spec("app.tasks") is not None:
        task_packages.append("app.tasks")
except ModuleNotFoundError:
    pass
try:
    if importlib.util.find_spec("tasks") is not None:
        task_packages.append("tasks")
except ModuleNotFoundError:
    pass
if task_packages:
    celery_app.autodiscover_tasks(task_packages)

# Explicit task module imports for mixed import layouts ("/app" vs "app.*")
# so worker always registers audio tasks even when autodiscovery is brittle.
for module_name in ("tasks.audio_pipeline", "app.tasks.audio_pipeline"):
    try:
        importlib.import_module(module_name)
    except ModuleNotFoundError:
        continue
