import asyncio
import logging
import os
from datetime import UTC, datetime, timedelta
from typing import Dict, List, Optional

import httpx

try:
    from ...db import DatabaseManager
    from .google_oauth import GoogleCalendarOAuthService
    from .reminder_email import CalendarReminderEmailService
except (ImportError, ValueError):
    try:
        from app.db import DatabaseManager
        from app.services.calendar.google_oauth import GoogleCalendarOAuthService
        from app.services.calendar.reminder_email import CalendarReminderEmailService
    except (ImportError, ValueError):
        from db import DatabaseManager
        from services.calendar.google_oauth import GoogleCalendarOAuthService
        from services.calendar.reminder_email import CalendarReminderEmailService

logger = logging.getLogger(__name__)


class CalendarReminderScheduler:
    def __init__(self):
        self.db = DatabaseManager()
        self.oauth = GoogleCalendarOAuthService(self.db)
        self.reminder_email = CalendarReminderEmailService()
        self._task: Optional[asyncio.Task] = None
        self._stopped = asyncio.Event()
        self._interval_seconds = int(os.getenv("CALENDAR_REMINDER_LOOP_SECONDS", "60"))

    @staticmethod
    def _parse_event_time(value: Optional[str]) -> Optional[datetime]:
        if not value:
            return None
        normalized = value.replace("Z", "+00:00")
        parsed = datetime.fromisoformat(normalized)
        if parsed.tzinfo is not None:
            # Convert aware timestamp to UTC and store as naive UTC in DB.
            return parsed.astimezone(UTC).replace(tzinfo=None)
        return parsed

    @staticmethod
    def _extract_meeting_link(event: Dict) -> Optional[str]:
        hangout = event.get("hangoutLink")
        if hangout:
            return hangout
        location = event.get("location", "")
        if isinstance(location, str) and ("http://" in location or "https://" in location):
            return location
        return None

    async def _sync_user_upcoming_events(self, integration: Dict):
        user_email = integration["user_email"]
        provider = integration["provider"]
        access_token = integration.get("access_token", "")
        refresh_token = integration.get("refresh_token", "")

        if not access_token:
            logger.warning(f"[CalendarSync] Missing access token for {user_email}")
            return

        time_min = datetime.utcnow() - timedelta(minutes=10)
        time_max = datetime.utcnow() + timedelta(days=2)
        params = {
            "timeMin": time_min.isoformat() + "Z",
            "timeMax": time_max.isoformat() + "Z",
            "singleEvents": "true",
            "orderBy": "startTime",
            "maxResults": "100",
        }

        async with httpx.AsyncClient(timeout=20.0) as client:
            response = await client.get(
                "https://www.googleapis.com/calendar/v3/calendars/primary/events",
                params=params,
                headers={"Authorization": f"Bearer {access_token}"},
            )

            if response.status_code == 401 and refresh_token:
                refreshed = await self.oauth.refresh_access_token(refresh_token)
                access_token = refreshed["access_token"]
                await self.db.update_calendar_access_token(
                    user_email=user_email,
                    provider=provider,
                    access_token=access_token,
                    token_expires_at=refreshed["token_expires_at"],
                )
                response = await client.get(
                    "https://www.googleapis.com/calendar/v3/calendars/primary/events",
                    params=params,
                    headers={"Authorization": f"Bearer {access_token}"},
                )

            response.raise_for_status()
            payload = response.json()

        items = payload.get("items", [])
        events: List[Dict] = []
        for item in items:
            if item.get("status") == "cancelled":
                continue

            start_data = item.get("start", {})
            end_data = item.get("end", {})
            start_time = self._parse_event_time(start_data.get("dateTime"))
            end_time = self._parse_event_time(end_data.get("dateTime"))
            if not start_time:
                continue

            attendees = item.get("attendees", []) or []
            attendee_emails = [
                a.get("email", "").strip().lower()
                for a in attendees
                if a.get("email")
            ]
            organizer_email = (item.get("organizer", {}) or {}).get("email")

            events.append(
                {
                    "event_id": item.get("id"),
                    "meeting_title": item.get("summary") or "Untitled Calendar Meeting",
                    "meeting_link": self._extract_meeting_link(item),
                    "organizer_email": organizer_email,
                    "attendee_emails": attendee_emails,
                    "start_time": start_time,
                    "end_time": end_time,
                }
            )

        await self.db.upsert_calendar_events(
            user_email=user_email,
            provider=provider,
            events=events,
        )

    async def _process_due_reminders(self):
        due = await self.db.get_due_calendar_reminders()
        logger.info(f"[CalendarReminder] Due reminders this cycle: {len(due)}")
        for reminder in due:
            try:
                result = await self.reminder_email.send_pre_meeting_reminder(
                    host_email=reminder["user_email"],
                    meeting_title=reminder["meeting_title"],
                    meeting_start_iso=reminder["start_time"].isoformat() + "Z",
                    meeting_link=reminder.get("meeting_link"),
                    attendees=reminder.get("attendees", []),
                    include_attendees=bool(reminder["attendee_reminders_enabled"]),
                )
                await self.db.mark_calendar_reminder_sent(
                    user_email=reminder["user_email"],
                    provider=reminder["provider"],
                    event_id=reminder["event_id"],
                    event_start_time=reminder["start_time"],
                    recipients=result.get("recipients", []),
                )
                logger.info(
                    f"[CalendarReminder] Sent reminder for {reminder['event_id']} to {len(result.get('recipients', []))} recipients"
                )
            except Exception as e:
                logger.error(
                    f"[CalendarReminder] Failed reminder for {reminder['event_id']}: {e}"
                )

    async def run_once(self):
        integrations = await self.db.get_active_calendar_integrations(provider="google")
        for integration in integrations:
            try:
                await self._sync_user_upcoming_events(integration)
            except Exception as e:
                logger.error(
                    f"[CalendarSync] Failed for {integration['user_email']}: {e}"
                )
        await self._process_due_reminders()

    async def _run_loop(self):
        logger.info(
            f"[CalendarReminder] Worker started (interval={self._interval_seconds}s)"
        )
        while not self._stopped.is_set():
            try:
                await self.run_once()
            except Exception as e:
                logger.error(f"[CalendarReminder] Worker loop error: {e}")
            try:
                await asyncio.wait_for(self._stopped.wait(), timeout=self._interval_seconds)
            except asyncio.TimeoutError:
                pass
        logger.info("[CalendarReminder] Worker stopped")

    def start(self):
        if self._task and not self._task.done():
            return
        self._stopped.clear()
        self._task = asyncio.create_task(self._run_loop())

    async def stop(self):
        self._stopped.set()
        if self._task:
            await self._task
