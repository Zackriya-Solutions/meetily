"""
JWT authentication middleware for FastAPI.

Provides dependencies for validating access and refresh tokens.
"""

import os
import logging
import uuid
from typing import Optional

import jwt
from fastapi import Depends, HTTPException, status
from fastapi.security import HTTPBearer, HTTPAuthorizationCredentials

logger = logging.getLogger(__name__)

security = HTTPBearer(auto_error=False)

JWT_SECRET = os.getenv("JWT_SECRET", "")
JWT_ALGORITHM = "HS256"
ACCESS_TOKEN_EXPIRE_MINUTES = 15
REFRESH_TOKEN_EXPIRE_DAYS = 30


def _decode_token(token: str, expected_type: str = "access") -> dict:
    """Decode and validate a JWT token.

    Returns the payload dict or raises HTTPException.
    """
    if not JWT_SECRET:
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail="JWT_SECRET not configured",
        )
    try:
        payload = jwt.decode(token, JWT_SECRET, algorithms=[JWT_ALGORITHM])
    except jwt.ExpiredSignatureError:
        raise HTTPException(
            status_code=status.HTTP_401_UNAUTHORIZED,
            detail="Token has expired",
        )
    except jwt.InvalidTokenError:
        raise HTTPException(
            status_code=status.HTTP_401_UNAUTHORIZED,
            detail="Invalid token",
        )

    if payload.get("type") != expected_type:
        raise HTTPException(
            status_code=status.HTTP_401_UNAUTHORIZED,
            detail=f"Expected {expected_type} token",
        )

    return payload


async def get_current_user(
    credentials: Optional[HTTPAuthorizationCredentials] = Depends(security),
) -> dict:
    """Validate access token and return user payload. Raises 401 if invalid."""
    if credentials is None:
        raise HTTPException(
            status_code=status.HTTP_401_UNAUTHORIZED,
            detail="Authentication required",
        )
    payload = _decode_token(credentials.credentials, expected_type="access")
    # Alias: cloud routes use "user_id", JWT standard uses "sub"
    payload["user_id"] = payload["sub"]
    return payload


async def get_optional_user(
    credentials: Optional[HTTPAuthorizationCredentials] = Depends(security),
) -> Optional[dict]:
    """Like get_current_user but returns None for unauthenticated requests."""
    if credentials is None:
        return None
    try:
        return _decode_token(credentials.credentials, expected_type="access")
    except HTTPException:
        return None


def create_access_token(
    user_id: str,
    device_id: str,
    org_id: Optional[str] = None,
    org_role: Optional[str] = None,
) -> str:
    """Create a short-lived access token."""
    import datetime

    now = datetime.datetime.now(datetime.timezone.utc)
    payload = {
        "sub": user_id,
        "device_id": device_id,
        "type": "access",
        "iat": now,
        "exp": now + datetime.timedelta(minutes=ACCESS_TOKEN_EXPIRE_MINUTES),
    }
    if org_id:
        payload["org_id"] = org_id
    if org_role:
        payload["org_role"] = org_role
    return jwt.encode(payload, JWT_SECRET, algorithm=JWT_ALGORITHM)


def create_refresh_token(
    user_id: str,
    device_id: str,
    family_id: Optional[str] = None,
    org_id: Optional[str] = None,
    org_role: Optional[str] = None,
) -> str:
    """Create a long-lived refresh token with token family tracking."""
    import datetime

    now = datetime.datetime.now(datetime.timezone.utc)
    payload = {
        "sub": user_id,
        "device_id": device_id,
        "type": "refresh",
        "family_id": family_id or str(uuid.uuid4()),
        "iat": now,
        "exp": now + datetime.timedelta(days=REFRESH_TOKEN_EXPIRE_DAYS),
    }
    if org_id:
        payload["org_id"] = org_id
    if org_role:
        payload["org_role"] = org_role
    return jwt.encode(payload, JWT_SECRET, algorithm=JWT_ALGORITHM)
