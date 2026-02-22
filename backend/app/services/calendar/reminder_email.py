import html
import os
from datetime import datetime
from typing import Dict, List, Optional
from urllib.parse import quote_plus

from ..email.smtp_sender import SmtpEmailSender


class CalendarReminderEmailService:
    def __init__(self, sender: Optional[SmtpEmailSender] = None):
        self.sender = sender or SmtpEmailSender()

    def _default_start_meeting_url(self, meeting_title: str) -> str:
        base = os.getenv(
            "CALENDAR_EMAIL_START_MEETING_URL",
            "http://localhost:3118/?autoStart=true&source=calendar_email",
        )
        separator = "&" if "?" in base else "?"
        return f"{base}{separator}meetingTitle={quote_plus(meeting_title)}"

    @staticmethod
    def _format_time_label(meeting_start_iso: Optional[str]) -> str:
        if not meeting_start_iso:
            return "Starting soon"

        try:
            normalized = meeting_start_iso.replace("Z", "+00:00")
            dt = datetime.fromisoformat(normalized)
            return dt.strftime("%a, %b %d at %I:%M %p")
        except Exception:
            return meeting_start_iso

    @staticmethod
    def _clean_emails(candidates: List[str]) -> List[str]:
        cleaned = []
        for value in candidates:
            email = (value or "").strip().lower()
            if "@" not in email:
                continue
            if email not in cleaned:
                cleaned.append(email)
        return cleaned

    def _build_email_content(
        self,
        host_email: str,
        meeting_title: str,
        meeting_time_label: str,
        start_meeting_url: str,
        meeting_link: Optional[str] = None,
    ) -> Dict[str, str]:
        safe_title = html.escape(meeting_title)
        safe_host_email = html.escape(host_email)
        safe_time = html.escape(meeting_time_label)
        safe_start_url = html.escape(start_meeting_url, quote=True)
        safe_meeting_link = html.escape(meeting_link or "", quote=True)

        subject = f"Reminder: {meeting_title}"

        text_lines = [
            f"Hi {host_email},",
            "",
            f"Your meeting \"{meeting_title}\" is {meeting_time_label}.",
            f"Start Pnyx: {start_meeting_url}",
        ]
        if meeting_link:
            text_lines.append(f"Join Link: {meeting_link}")
        text_lines.extend(
            [
                "",
                "Quick checklist:",
                "- Check microphone access",
                "- Confirm room/device audio",
                "- Start recording when everyone joins",
            ]
        )
        text_body = "\n".join(text_lines)

        join_link_html = (
            f'<p style="margin: 12px 0 0;"><a href="{safe_meeting_link}" '
            'style="color:#2563eb;text-decoration:none;">Open meeting link</a></p>'
            if meeting_link
            else ""
        )

        html_body = f"""
        <div style="font-family:Arial,sans-serif;max-width:640px;margin:0 auto;padding:20px;color:#111827;">
          <h2 style="margin:0 0 8px;">Upcoming meeting reminder</h2>
          <p style="margin:0 0 16px;">Hi {safe_host_email},</p>
          <p style="margin:0 0 12px;">
            Your meeting <strong>{safe_title}</strong> is <strong>{safe_time}</strong>.
          </p>
          <a href="{safe_start_url}"
             style="display:inline-block;background:#dc2626;color:#ffffff;text-decoration:none;padding:12px 18px;border-radius:8px;font-weight:600;">
            Start Meeting in Pnyx
          </a>
          {join_link_html}
          <div style="margin-top:20px;padding:14px;background:#f9fafb;border:1px solid #e5e7eb;border-radius:8px;">
            <p style="margin:0 0 8px;font-weight:600;">Quick setup checklist</p>
            <ul style="margin:0 0 0 18px;padding:0;">
              <li>Check microphone access</li>
              <li>Confirm room/device audio</li>
              <li>Start recording when everyone joins</li>
            </ul>
          </div>
        </div>
        """

        return {"subject": subject, "text_body": text_body, "html_body": html_body}

    async def send_pre_meeting_reminder(
        self,
        host_email: str,
        meeting_title: str,
        meeting_start_iso: Optional[str] = None,
        meeting_link: Optional[str] = None,
        start_meeting_url: Optional[str] = None,
        attendees: Optional[List[str]] = None,
        include_attendees: bool = False,
    ) -> Dict:
        start_url = start_meeting_url or self._default_start_meeting_url(meeting_title)
        recipients = [host_email]
        if include_attendees and attendees:
            recipients.extend(attendees)

        recipients = self._clean_emails(recipients)
        meeting_time_label = self._format_time_label(meeting_start_iso)
        content = self._build_email_content(
            host_email=host_email,
            meeting_title=meeting_title,
            meeting_time_label=meeting_time_label,
            start_meeting_url=start_url,
            meeting_link=meeting_link,
        )

        await self.sender.send(
            recipients=recipients,
            subject=content["subject"],
            text_body=content["text_body"],
            html_body=content["html_body"],
        )

        return {
            "sent": True,
            "recipient_count": len(recipients),
            "recipients": recipients,
            "start_meeting_url": start_url,
        }

