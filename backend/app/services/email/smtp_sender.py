import asyncio
import os
import smtplib
from email.message import EmailMessage
from typing import List


class SmtpEmailSender:
    def __init__(self):
        self.smtp_host = os.getenv("SMTP_HOST", "")
        self.smtp_port = int(os.getenv("SMTP_PORT", "587"))
        self.smtp_username = os.getenv("SMTP_USERNAME", "")
        self.smtp_password = os.getenv("SMTP_PASSWORD", "")
        self.smtp_from_email = os.getenv("SMTP_FROM_EMAIL", "")
        self.smtp_use_tls = os.getenv("SMTP_USE_TLS", "true").lower() == "true"

    def _validate(self):
        missing = []
        if not self.smtp_host:
            missing.append("SMTP_HOST")
        if not self.smtp_from_email:
            missing.append("SMTP_FROM_EMAIL")
        if not self.smtp_username:
            missing.append("SMTP_USERNAME")
        if not self.smtp_password:
            missing.append("SMTP_PASSWORD")

        if missing:
            raise ValueError(
                f"Missing SMTP configuration: {', '.join(missing)}"
            )

    def _send_sync(
        self,
        recipients: List[str],
        subject: str,
        text_body: str,
        html_body: str,
    ):
        self._validate()
        if not recipients:
            raise ValueError("No recipients provided")

        message = EmailMessage()
        message["Subject"] = subject
        message["From"] = self.smtp_from_email
        message["To"] = ", ".join(recipients)
        message.set_content(text_body)
        message.add_alternative(html_body, subtype="html")

        with smtplib.SMTP(self.smtp_host, self.smtp_port, timeout=20) as smtp:
            if self.smtp_use_tls:
                smtp.starttls()
            smtp.login(self.smtp_username, self.smtp_password)
            smtp.send_message(message)

    async def send(
        self,
        recipients: List[str],
        subject: str,
        text_body: str,
        html_body: str,
    ):
        await asyncio.to_thread(
            self._send_sync,
            recipients,
            subject,
            text_body,
            html_body,
        )

