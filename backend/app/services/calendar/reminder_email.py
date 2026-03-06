import html
import os
from datetime import datetime
from typing import Dict, List, Optional
from urllib.parse import quote_plus
from zoneinfo import ZoneInfo

from ..email.smtp_sender import SmtpEmailSender


class CalendarReminderEmailService:
    def __init__(self, sender: Optional[SmtpEmailSender] = None):
        self.sender = sender or SmtpEmailSender()

    def _default_start_meeting_url(self, meeting_title: str) -> str:
        base = os.getenv(
            "CALENDAR_EMAIL_START_MEETING_URL",
            "https://meet.quexio.com/?autoStart=true&source=calendar_email",
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
            if dt.tzinfo is None:
                dt = dt.replace(tzinfo=ZoneInfo("UTC"))

            tz_name = os.getenv("CALENDAR_EMAIL_TIMEZONE", "Asia/Kolkata")
            local_dt = dt.astimezone(ZoneInfo(tz_name))
            return local_dt.strftime("%a, %d %b %Y at %I:%M %p IST")
        except Exception:
            return meeting_start_iso

    @staticmethod
    def _clean_emails(candidates: List[str]) -> List[str]:
        cleaned = []
        for value in candidates:
            if isinstance(value, dict):
                email = (value.get("email") or "").strip().lower()
            else:
                email = (str(value or "")).strip().lower()
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
            f'Your meeting "{meeting_title}" is {meeting_time_label}.',
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
            'style="color:#1d4ed8;text-decoration:none;font-weight:600;">Open meeting link</a></p>'
            if meeting_link
            else ""
        )

        html_body = f"""
        <div style="font-family:'Segoe UI',Arial,sans-serif;max-width:680px;margin:0 auto;padding:22px;color:#111827;background:#f8fafc;">
          <div style="background:#ffffff;border:1px solid #e2e8f0;border-radius:12px;padding:20px;">
            <p style="margin:0;color:#64748b;font-size:12px;letter-spacing:.06em;text-transform:uppercase;">Pnyx Meeting Reminder</p>
            <h2 style="margin:8px 0 12px;font-size:22px;line-height:1.3;">{safe_title}</h2>
            <p style="margin:0 0 16px;color:#334155;">Hi {safe_host_email}, your meeting is scheduled for <strong>{safe_time}</strong>.</p>

            <table role="presentation" style="width:100%;border-collapse:collapse;margin:0 0 16px;">
              <tr>
                <td style="padding:10px 12px;border:1px solid #e2e8f0;background:#f8fafc;width:160px;color:#475569;font-weight:600;">Meeting</td>
                <td style="padding:10px 12px;border:1px solid #e2e8f0;color:#0f172a;">{safe_title}</td>
              </tr>
              <tr>
                <td style="padding:10px 12px;border:1px solid #e2e8f0;background:#f8fafc;color:#475569;font-weight:600;">Time (IST)</td>
                <td style="padding:10px 12px;border:1px solid #e2e8f0;color:#0f172a;">{safe_time}</td>
              </tr>
            </table>

            <a href="{safe_start_url}"
               style="display:inline-block;background:#0f172a;color:#ffffff;text-decoration:none;padding:11px 18px;border-radius:8px;font-weight:600;">
              Start in Pnyx
            </a>
            {join_link_html}

            <div style="margin-top:18px;padding:12px;background:#f8fafc;border:1px solid #e2e8f0;border-radius:8px;">
              <p style="margin:0 0 8px;font-weight:600;color:#0f172a;">Before you start</p>
              <ul style="margin:0 0 0 18px;padding:0;color:#334155;">
                <li>Verify microphone permissions</li>
                <li>Confirm speaker/system audio</li>
                <li>Start recording once all participants join</li>
              </ul>
            </div>
          </div>
        </div>
        """

        return {"subject": subject, "text_body": text_body, "html_body": html_body}

    def _build_recap_email_content(
        self,
        host_email: str,
        meeting_title: str,
        tldr: str = "",
        action_items: Optional[List[str]] = None,
        app_notes_url: str = "",
    ) -> Dict[str, str]:
        safe_title = html.escape(meeting_title)
        safe_host_email = html.escape(host_email)
        safe_app_url = html.escape(app_notes_url or "", quote=True)
        safe_tldr = html.escape(tldr or "Your meeting notes are ready.")
        items = action_items or []

        subject = f"Recap: {meeting_title}"

        # --- Plain-text fallback ---
        text_lines = [
            f"Hi {host_email},",
            "",
            f'Your notes for "{meeting_title}" are ready.',
            "",
            f"TL;DR: {tldr or 'Your meeting notes are ready.'}",
        ]
        if items:
            text_lines.extend(["", "Top Action Items:"])
            for idx, item in enumerate(items[:3], 1):
                text_lines.append(f"  {idx}. {item}")
        if app_notes_url:
            text_lines.extend([
                "",
                f"View full meeting notes & transcript: {app_notes_url}",
            ])
        text_body = "\n".join(text_lines)

        # --- Action items HTML ---
        action_items_html = ""
        if items:
            li_items = "".join(
                f'<li style="margin:0 0 6px;color:#334155;">{html.escape(item)}</li>'
                for item in items[:3]
            )
            action_items_html = f"""
            <div style="margin:16px 0;padding:14px;background:#fffbeb;border:1px solid #fde68a;border-radius:8px;">
              <p style="margin:0 0 8px;font-weight:600;color:#92400e;font-size:13px;text-transform:uppercase;letter-spacing:.04em;">Action Items</p>
              <ol style="margin:0 0 0 18px;padding:0;font-size:14px;">
                {li_items}
              </ol>
            </div>
            """

        # --- CTA button HTML ---
        cta_html = ""
        if safe_app_url:
            cta_html = f"""
            <div style="text-align:center;margin:20px 0 8px;">
              <a href="{safe_app_url}"
                 style="display:inline-block;background:#0f172a;color:#ffffff;text-decoration:none;padding:13px 28px;border-radius:8px;font-weight:600;font-size:15px;">
                View Full Meeting Notes &amp; Transcript
              </a>
            </div>
            """

        html_body = f"""
        <div style="font-family:'Segoe UI',Arial,sans-serif;max-width:680px;margin:0 auto;padding:22px;color:#111827;background:#f8fafc;">
          <div style="background:#ffffff;border:1px solid #e2e8f0;border-radius:12px;padding:24px;">
            <p style="margin:0;color:#64748b;font-size:12px;letter-spacing:.06em;text-transform:uppercase;">Pnyx Meeting Recap</p>
            <h2 style="margin:8px 0 14px;font-size:22px;line-height:1.3;">{safe_title}</h2>
            <p style="margin:0 0 6px;color:#334155;">Hi {safe_host_email}, your notes are ready.</p>

            <div style="margin:14px 0;padding:12px;background:#f0f9ff;border-left:4px solid #3b82f6;border-radius:0 8px 8px 0;">
              <p style="margin:0;color:#1e40af;font-size:14px;line-height:1.5;"><strong>TL;DR:</strong> {safe_tldr}</p>
            </div>

            {action_items_html}
            {cta_html}

            <p style="margin:16px 0 0;color:#94a3b8;font-size:12px;text-align:center;">
              This email was sent by Pnyx Meeting Co-Pilot.
            </p>
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

    async def send_post_meeting_recap(
        self,
        host_email: str,
        meeting_title: str,
        tldr: str = "",
        action_items: Optional[List[str]] = None,
        app_notes_url: str = "",
        attendees: Optional[List[str]] = None,
        include_attendees: bool = False,
        accepted_only_emails: Optional[List[str]] = None,
    ) -> Dict:
        recipients = [host_email]
        if include_attendees and attendees:
            recipients.extend(attendees)
        recipients = self._clean_emails(recipients)

        # Filter to only accepted/tentative attendees if provided
        if accepted_only_emails is not None:
            accepted_set = {e.strip().lower() for e in accepted_only_emails if e}
            # Always keep the host
            recipients = [
                r for r in recipients
                if r == host_email.strip().lower() or r in accepted_set
            ]

        content = self._build_recap_email_content(
            host_email=host_email,
            meeting_title=meeting_title,
            tldr=tldr,
            action_items=action_items,
            app_notes_url=app_notes_url,
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
        }

