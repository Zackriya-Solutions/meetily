from fastapi import APIRouter, Depends, HTTPException
from fastapi.responses import RedirectResponse
from typing import List, Dict, Optional
import os

try:
    from ..deps import get_current_user
    from ...db.manager import DatabaseManager
    from ...schemas.meeting import ShareNotesRequest
    from ...schemas.user import User
except (ImportError, ValueError):
    from api.deps import get_current_user
    from db.manager import DatabaseManager
    from schemas.meeting import ShareNotesRequest
    from schemas.user import User

router = APIRouter(prefix="/api/sharing", tags=["sharing"])
db_manager = DatabaseManager()

@router.get("/shared-with-me")
async def get_shared_notes(current_user: User = Depends(get_current_user)):
    """List all notes shared with the authenticated user."""
    return await db_manager.get_shared_notes_for_user(current_user.email)

@router.get("/{meeting_id}")
async def get_shared_note_details(meeting_id: str, current_user: User = Depends(get_current_user)):
    """Get a specific shared meeting's summary + transcript (respecting share_config)."""
    share_record = await db_manager.get_shared_note(meeting_id, current_user.email)
    if not share_record:
        raise HTTPException(status_code=404, detail="Shared note not found or access denied")
    
    # Get the actual meeting data
    summary_data = await db_manager.get_transcript_data(meeting_id)
    meeting_details = await db_manager.get_meeting(meeting_id)

    # Respect share_config
    share_config = share_record.get("share_config", {})
    if isinstance(share_config, str):
        import json
        try:
            share_config = json.loads(share_config)
        except json.JSONDecodeError:
            share_config = {"summary": True, "transcript": False}
            
    result = {
        "meeting": {
            "id": meeting_details.get("id"),
            "title": meeting_details.get("title"),
            "created_at": meeting_details.get("created_at")
        } if meeting_details else None,
        "shared_at": share_record.get("shared_at"),
        "owner_email": share_record.get("owner_email")
    }
    
    if share_config.get("summary", True):
        result["summary"] = summary_data.get("result") if summary_data else None
    else:
        result["summary"] = None
        
    if share_config.get("transcript", False):
        result["transcripts"] = meeting_details.get("transcripts", []) if meeting_details else []
    else:
        result["transcripts"] = []
        
    return result

@router.post("/{meeting_id}/share")
async def share_meeting_notes(meeting_id: str, request: ShareNotesRequest, current_user: User = Depends(get_current_user)):
    """Manually trigger sharing with selected attendees (for regeneration flow)."""
    # Verify ownership
    meeting_details = await db_manager.get_meeting(meeting_id)
    if not meeting_details:
        raise HTTPException(status_code=404, detail="Meeting not found")
        
    share_config = {
        "summary": request.share_summary,
        "transcript": request.share_transcript
    }
    
    shared_tokens = []
    
    recipients = request.recipient_emails
    if recipients is None:
        # Fetch accepted attendees automatically
        calendar_event_context = await db_manager.get_calendar_event_context_for_meeting(
            meeting_id=meeting_id,
            user_email=current_user.email,
            provider="google",
        )
        recipients = []
        if calendar_event_context:
            attendees = calendar_event_context.get("attendees", [])
            for att in attendees:
                if isinstance(att, dict):
                    status = att.get("responseStatus", "needsAction")
                    if status in ("accepted", "tentative"):
                        email = att.get("email", "").strip().lower()
                        if email and email != current_user.email:
                            recipients.append(email)
                else:
                    email = str(att).strip().lower()
                    if email and email != current_user.email:
                        recipients.append(email)
                        
    for recipient in recipients:
        token = await db_manager.create_shared_note(
            meeting_id=meeting_id,
            owner_email=current_user.email,
            shared_with_email=recipient,
            share_config=share_config
        )
        shared_tokens.append({"email": recipient, "token": token})
        
    return {"status": "success", "shared_with": shared_tokens}

@router.get("/view/{share_token}")
async def view_shared_note_by_token(share_token: str):
    """Public endpoint for email link access (redirects to app with auth check)."""
    share_record = await db_manager.get_shared_note_by_token(share_token)
    if not share_record:
        raise HTTPException(status_code=404, detail="Invalid or expired share link")
        
    app_base_url = os.environ.get("APP_BASE_URL", "http://localhost:3118")
    meeting_id = share_record["meeting_id"]
    
    # Redirect to the frontend shared route
    return RedirectResponse(url=f"{app_base_url}/meeting-details?id={meeting_id}&shared=true&token={share_token}")

@router.patch("/{meeting_id}/viewed")
async def mark_note_viewed(meeting_id: str, current_user: User = Depends(get_current_user)):
    """Mark a shared note as viewed by the user."""
    await db_manager.mark_shared_note_viewed(meeting_id, current_user.email)
    return {"status": "success"}
