import logging
import os
from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware
from dotenv import load_dotenv

# Load environment variables
load_dotenv()

# Configure logging
logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s - %(levelname)s - [%(filename)s:%(lineno)d - %(funcName)s()] - %(message)s",
    datefmt="%Y-%m-%d %H:%M:%S",
)
logger = logging.getLogger(__name__)
calendar_scheduler = None
audio_reconciler = None

# Import Routers
try:
    from app.api.routers import (
        meetings,
        transcripts,
        chat,
        audio,
        diarization,
        settings,
        calendar,
        admin,
        feedback,
    )
except ImportError:
    from api.routers import (
        meetings,
        transcripts,
        chat,
        audio,
        diarization,
        settings,
        calendar,
        admin,
        feedback,
    )

app = FastAPI(
    title="Meeting Summarizer API",
    description="API for processing and summarizing meeting transcripts",
    version="1.0.0",
)

# Configure CORS
origins = [
    "http://localhost:3118",
    "http://localhost:3000",
    "https://pnyxx.vercel.app",
    "https://meet.digest.lat",
]

app.add_middleware(
    CORSMiddleware,
    allow_origins=origins,
    allow_credentials=True,
    allow_methods=["*"],
    allow_headers=["*"],
    max_age=3600,
)

# Include Routers
app.include_router(meetings.router, tags=["Meetings"])
app.include_router(transcripts.router, tags=["Transcripts"])
app.include_router(chat.router, tags=["Chat"])
app.include_router(audio.router, tags=["Audio"])
app.include_router(diarization.router, tags=["Diarization"])
app.include_router(settings.router, tags=["Settings"])
app.include_router(calendar.router, tags=["Calendar"])
app.include_router(admin.router, tags=["Admin"])
app.include_router(feedback.router, prefix="/feedback", tags=["Feedback"])


@app.on_event("startup")
async def startup_event():
    global calendar_scheduler, audio_reconciler
    try:
        from app.services.calendar.reminder_scheduler import CalendarReminderScheduler
    except ImportError:
        from services.calendar.reminder_scheduler import CalendarReminderScheduler

    if os.getenv("CALENDAR_REMINDER_AUTOMATION_ENABLED", "true").lower() != "true":
        logger.info("[CalendarReminder] Automation disabled by env")
        return

    calendar_scheduler = CalendarReminderScheduler()
    calendar_scheduler.start()
    logger.info("[CalendarReminder] Automation worker initialized")

    if os.getenv("AUDIO_SESSION_RECONCILER_ENABLED", "true").lower() == "true":
        try:
            from app.services.audio.session_reconciler import AudioSessionReconciler
        except ImportError:
            from services.audio.session_reconciler import AudioSessionReconciler
        audio_reconciler = AudioSessionReconciler()
        audio_reconciler.start()
        logger.info("[AudioReconciler] Session reconciler initialized")


@app.on_event("shutdown")
async def shutdown_event():
    global calendar_scheduler, audio_reconciler
    if calendar_scheduler:
        await calendar_scheduler.stop()
        calendar_scheduler = None
    if audio_reconciler:
        await audio_reconciler.stop()
        audio_reconciler = None


@app.get("/health")
async def health_check():
    return {"status": "ok", "version": "1.0.0"}


if __name__ == "__main__":
    import uvicorn

    uvicorn.run("main:app", host="0.0.0.0", port=8000, reload=True)
