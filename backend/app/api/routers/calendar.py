import logging
from urllib.parse import urlencode

from fastapi import APIRouter, Depends, HTTPException, Query
from fastapi.responses import RedirectResponse

try:
    from ..deps import get_current_user
    from ...db import DatabaseManager
    from ...schemas.settings import (
        CalendarAutomationSettingsRequest,
        CalendarConnectRequest,
        CalendarDisconnectRequest,
        CalendarReminderEmailRequest,
        SyncOAuthRequest,
    )
    from ...schemas.user import User
    from ...services.calendar.google_oauth import GoogleCalendarOAuthService
    from ...services.calendar.reminder_email import CalendarReminderEmailService
except (ImportError, ValueError):
    from api.deps import get_current_user
    from db import DatabaseManager
    from schemas.settings import (
        CalendarAutomationSettingsRequest,
        CalendarConnectRequest,
        CalendarDisconnectRequest,
        CalendarReminderEmailRequest,
        SyncOAuthRequest,
    )
    from schemas.user import User
    from services.calendar.google_oauth import GoogleCalendarOAuthService
    from services.calendar.reminder_email import CalendarReminderEmailService


logger = logging.getLogger(__name__)
router = APIRouter()
db = DatabaseManager()
oauth_service = GoogleCalendarOAuthService(db=db)
reminder_email_service = CalendarReminderEmailService()


@router.get("/upcoming-meetings")
async def get_upcoming_meetings(current_user: User = Depends(get_current_user)):
    try:
        events = await db.get_upcoming_calendar_events(current_user.email, provider="google")
        return {"status": "success", "events": events}
    except Exception as e:
        logger.error(f"Failed to fetch upcoming meetings: {e}")
        raise HTTPException(status_code=500, detail="Failed to fetch upcoming meetings")


@router.get("/status")
async def get_calendar_status(current_user: User = Depends(get_current_user)):
    return await db.get_calendar_integration(current_user.email, provider="google")


@router.get("/settings")
async def get_calendar_settings(current_user: User = Depends(get_current_user)):
    return await db.get_calendar_automation_settings(current_user.email)


@router.put("/settings")
async def update_calendar_settings(
    request: CalendarAutomationSettingsRequest,
    current_user: User = Depends(get_current_user),
):
    if request.reminder_offset_minutes < 1 or request.reminder_offset_minutes > 30:
        raise HTTPException(
            status_code=400, detail="reminder_offset_minutes must be between 1 and 30"
        )

    settings = await db.upsert_calendar_automation_settings(
        user_email=current_user.email,
        settings=request.dict(),
    )
    return {"status": "success", "settings": settings}


@router.post("/google/connect")
async def start_google_calendar_connect(
    request: CalendarConnectRequest,
    current_user: User = Depends(get_current_user),
):
    try:
        return await oauth_service.build_google_authorization_url(
            user_email=current_user.email,
            request_write_scope=request.request_write_scope,
        )
    except ValueError as e:
        raise HTTPException(status_code=500, detail=str(e))
    except Exception as e:
        logger.error(f"Failed to start calendar OAuth: {e}")
        raise HTTPException(status_code=500, detail="Failed to start Google OAuth flow")


@router.post("/disconnect")
async def disconnect_calendar(
    request: CalendarDisconnectRequest,
    current_user: User = Depends(get_current_user),
):
    if request.provider != "google":
        raise HTTPException(status_code=400, detail="Only google provider is supported")
    await db.disconnect_calendar_integration(current_user.email, provider=request.provider)
    return {"status": "success"}


@router.post("/reminders/send")
async def send_calendar_reminder_email(
    request: CalendarReminderEmailRequest,
    current_user: User = Depends(get_current_user),
):
    settings = await db.get_calendar_automation_settings(current_user.email)
    if not settings.get("reminders_enabled", True):
        raise HTTPException(status_code=400, detail="Calendar reminders are disabled")

    include_attendees = (
        request.include_attendees
        if request.include_attendees is not None
        else settings.get("attendee_reminders_enabled", False)
    )

    if include_attendees and not request.attendees:
        raise HTTPException(
            status_code=400,
            detail="attendees list is required when include_attendees is true",
        )

    try:
        result = await reminder_email_service.send_pre_meeting_reminder(
            host_email=current_user.email,
            meeting_title=request.meeting_title,
            meeting_start_iso=request.meeting_start_iso,
            meeting_link=request.meeting_link,
            start_meeting_url=request.start_meeting_url,
            attendees=request.attendees or [],
            include_attendees=include_attendees,
        )
        return {"status": "success", **result}
    except ValueError as e:
        raise HTTPException(status_code=500, detail=str(e))
    except Exception as e:
        logger.error(f"Failed to send reminder email: {e}")
        raise HTTPException(status_code=500, detail=f"Failed to send reminder email: {str(e)}")


@router.get("/google/callback")
async def google_calendar_callback(
    code: str = Query(default=""),
    state: str = Query(default=""),
    error: str = Query(default=""),
):
    frontend_settings_url = oauth_service._get_frontend_settings_url()

    if error:
        params = urlencode({"calendar": "error", "reason": error})
        return RedirectResponse(f"{frontend_settings_url}?{params}")

    if not code or not state:
        params = urlencode({"calendar": "error", "reason": "missing_code_or_state"})
        return RedirectResponse(f"{frontend_settings_url}?{params}")

    try:
        result = await oauth_service.complete_google_oauth(state=state, code=code)
        params = urlencode({"calendar": "connected", "email": result["account_email"]})
        return RedirectResponse(f"{result['redirect_url']}?{params}")
    except Exception as e:
        logger.error(f"Google OAuth callback failed: {e}")
        params = urlencode({"calendar": "error", "reason": "oauth_failed"})
        return RedirectResponse(f"{frontend_settings_url}?{params}")

from datetime import datetime

@router.post("/sync-oauth")
async def sync_oauth_tokens(
    request: SyncOAuthRequest,
    current_user: User = Depends(get_current_user),
):
    try:
        expires_at = None
        if request.token_expires_at:
            # Handle standard ISO formats
            expires_string = request.token_expires_at.replace("Z", "+00:00")
            expires_at = datetime.fromisoformat(expires_string).replace(tzinfo=None)
            
        await db.upsert_calendar_integration(
            user_email=current_user.email,
            provider="google",
            external_account_email=request.external_account_email,
            scopes=request.scopes,
            access_token=request.access_token,
            refresh_token=request.refresh_token or "", # Fallback if empty
            token_expires_at=expires_at,
        )
        return {"status": "success"}
    except Exception as e:
        logger.error(f"Failed to sync oauth tokens: {e}")
        raise HTTPException(status_code=500, detail="Failed to sync tokens")
