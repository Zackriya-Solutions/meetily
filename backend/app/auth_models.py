"""
Pydantic models for authentication endpoints.
"""

from pydantic import BaseModel, EmailStr, field_validator
from typing import Optional, List
import re


def _check_password_complexity(v: str) -> str:
    """Shared password validation: min 8 chars, 1 uppercase, 1 lowercase, 1 digit."""
    if len(v) < 8:
        raise ValueError("Password must be at least 8 characters")
    if not re.search(r"[A-Z]", v):
        raise ValueError("Password must contain at least one uppercase letter")
    if not re.search(r"[a-z]", v):
        raise ValueError("Password must contain at least one lowercase letter")
    if not re.search(r"\d", v):
        raise ValueError("Password must contain at least one digit")
    return v


class RegisterRequest(BaseModel):
    email: str
    password: str
    display_name: Optional[str] = None
    device_id: str  # Current device's user_id from analytics.json
    invite_code: str  # Required — users must be invited to an organisation

    @field_validator("email")
    @classmethod
    def validate_email(cls, v: str) -> str:
        v = v.strip().lower()
        if not re.match(r"^[^@\s]+@[^@\s]+\.[^@\s]+$", v):
            raise ValueError("Invalid email address")
        return v

    @field_validator("password")
    @classmethod
    def validate_password(cls, v: str) -> str:
        return _check_password_complexity(v)


class LoginRequest(BaseModel):
    email: str
    password: str
    device_id: str

    @field_validator("email")
    @classmethod
    def validate_email(cls, v: str) -> str:
        return v.strip().lower()


class RefreshRequest(BaseModel):
    refresh_token: str


class LinkDeviceRequest(BaseModel):
    device_id: str
    platform: Optional[str] = None
    architecture: Optional[str] = None


class DeviceSummary(BaseModel):
    device_id: str
    linked_at: str
    platform: Optional[str] = None
    last_seen: Optional[str] = None


class UserProfile(BaseModel):
    user_id: str
    email: str
    display_name: Optional[str] = None
    account_level: Optional[str] = None
    email_verified: Optional[bool] = None
    org_id: Optional[str] = None
    org_role: Optional[str] = None
    org_name: Optional[str] = None
    devices: List[DeviceSummary] = []


class ForgotPasswordRequest(BaseModel):
    email: str

    @field_validator("email")
    @classmethod
    def validate_email(cls, v: str) -> str:
        return v.strip().lower()


class ResetPasswordRequest(BaseModel):
    email: str
    code: str
    new_password: str

    @field_validator("email")
    @classmethod
    def validate_email(cls, v: str) -> str:
        return v.strip().lower()

    @field_validator("new_password")
    @classmethod
    def validate_password(cls, v: str) -> str:
        return _check_password_complexity(v)


class ChangePasswordRequest(BaseModel):
    current_password: str
    new_password: str

    @field_validator("new_password")
    @classmethod
    def validate_password(cls, v: str) -> str:
        return _check_password_complexity(v)


class VerifyEmailRequest(BaseModel):
    email: str
    code: str

    @field_validator("email")
    @classmethod
    def validate_email(cls, v: str) -> str:
        return v.strip().lower()


class ResendVerificationRequest(BaseModel):
    email: str

    @field_validator("email")
    @classmethod
    def validate_email(cls, v: str) -> str:
        return v.strip().lower()


class UpdateProfileRequest(BaseModel):
    display_name: str


class AuthResponse(BaseModel):
    access_token: str
    refresh_token: str
    token_type: str = "bearer"
    user: UserProfile
