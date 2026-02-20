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
    )
    from ...schemas.user import User
    from ...services.calendar.google_oauth import GoogleCalendarOAuthService
except (ImportError, ValueError):
    from api.deps import get_current_user
    from db import DatabaseManager
    from schemas.settings import (
        CalendarAutomationSettingsRequest,
        CalendarConnectRequest,
        CalendarDisconnectRequest,
    )
    from schemas.user import User
    from services.calendar.google_oauth import GoogleCalendarOAuthService


logger = logging.getLogger(__name__)
router = APIRouter(prefix="/api/calendar")
db = DatabaseManager()
oauth_service = GoogleCalendarOAuthService(db=db)


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
    if request.audio_summary_policy not in {"high_impact_only", "always", "never"}:
        raise HTTPException(
            status_code=400,
            detail="audio_summary_policy must be one of: high_impact_only, always, never",
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
