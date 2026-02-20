import base64
import hashlib
import os
import secrets
from datetime import datetime, timedelta
from typing import Dict
from urllib.parse import urlencode

import httpx


class GoogleCalendarOAuthService:
    AUTH_URL = "https://accounts.google.com/o/oauth2/v2/auth"
    TOKEN_URL = "https://oauth2.googleapis.com/token"
    USERINFO_URL = "https://www.googleapis.com/oauth2/v3/userinfo"

    BASE_SCOPES = [
        "openid",
        "email",
        "https://www.googleapis.com/auth/calendar.readonly",
    ]
    WRITE_SCOPE = "https://www.googleapis.com/auth/calendar.events"

    def __init__(self, db):
        self.db = db

    def _required_env(self, name: str) -> str:
        value = os.getenv(name)
        if not value:
            raise ValueError(f"Missing required environment variable: {name}")
        return value

    def _get_redirect_uri(self) -> str:
        return self._required_env("CALENDAR_OAUTH_REDIRECT_URI")

    def _get_frontend_settings_url(self) -> str:
        return os.getenv("CALENDAR_OAUTH_FRONTEND_SETTINGS_URL", "http://localhost:3000/settings")

    async def build_google_authorization_url(
        self, user_email: str, request_write_scope: bool = False
    ) -> Dict[str, str]:
        client_id = self._required_env("CALENDAR_GOOGLE_CLIENT_ID")
        redirect_uri = self._get_redirect_uri()

        state = secrets.token_urlsafe(32)
        code_verifier = secrets.token_urlsafe(64)
        challenge = base64.urlsafe_b64encode(
            hashlib.sha256(code_verifier.encode()).digest()
        ).decode().rstrip("=")

        scopes = list(self.BASE_SCOPES)
        if request_write_scope:
            scopes.append(self.WRITE_SCOPE)

        await self.db.save_calendar_oauth_state(
            state=state,
            user_email=user_email,
            code_verifier=code_verifier,
            expires_at=datetime.utcnow() + timedelta(minutes=10),
        )

        query = urlencode(
            {
                "client_id": client_id,
                "redirect_uri": redirect_uri,
                "response_type": "code",
                "scope": " ".join(scopes),
                "state": state,
                "access_type": "offline",
                "prompt": "consent",
                "include_granted_scopes": "true",
                "code_challenge": challenge,
                "code_challenge_method": "S256",
            }
        )

        return {"authorization_url": f"{self.AUTH_URL}?{query}"}

    async def complete_google_oauth(self, state: str, code: str) -> Dict[str, str]:
        client_id = self._required_env("CALENDAR_GOOGLE_CLIENT_ID")
        client_secret = self._required_env("CALENDAR_GOOGLE_CLIENT_SECRET")
        redirect_uri = self._get_redirect_uri()

        oauth_state = await self.db.consume_calendar_oauth_state(state)
        if not oauth_state:
            raise ValueError("Invalid or expired OAuth state")

        form_data = {
            "client_id": client_id,
            "client_secret": client_secret,
            "code": code,
            "grant_type": "authorization_code",
            "redirect_uri": redirect_uri,
            "code_verifier": oauth_state["code_verifier"],
        }

        async with httpx.AsyncClient(timeout=20.0) as client:
            token_response = await client.post(self.TOKEN_URL, data=form_data)
            token_response.raise_for_status()
            token_data = token_response.json()

            access_token = token_data.get("access_token", "")
            refresh_token = token_data.get("refresh_token", "")
            scope_string = token_data.get("scope", "")
            scopes = [scope for scope in scope_string.split(" ") if scope]

            account_email = oauth_state["user_email"]
            if access_token:
                userinfo_response = await client.get(
                    self.USERINFO_URL,
                    headers={"Authorization": f"Bearer {access_token}"},
                )
                if userinfo_response.status_code == 200:
                    userinfo_data = userinfo_response.json()
                    account_email = userinfo_data.get("email", account_email)

        expires_in = token_data.get("expires_in")
        token_expires_at = None
        if expires_in:
            token_expires_at = datetime.utcnow() + timedelta(seconds=int(expires_in))

        await self.db.upsert_calendar_integration(
            user_email=oauth_state["user_email"],
            provider="google",
            external_account_email=account_email,
            scopes=scopes,
            access_token=access_token,
            refresh_token=refresh_token,
            token_expires_at=token_expires_at,
        )

        return {
            "user_email": oauth_state["user_email"],
            "account_email": account_email,
            "redirect_url": self._get_frontend_settings_url(),
        }

