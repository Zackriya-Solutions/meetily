"""Pydantic models for organisation management endpoints."""

from pydantic import BaseModel, field_validator
from typing import Optional, List
import re


class CreateInviteRequest(BaseModel):
    email: Optional[str] = None
    role: str = "member"

    @field_validator("email")
    @classmethod
    def validate_email(cls, v: Optional[str]) -> Optional[str]:
        if v is None:
            return None
        v = v.strip().lower()
        if not re.match(r"^[^@\s]+@[^@\s]+\.[^@\s]+$", v):
            raise ValueError("Invalid email address")
        return v

    @field_validator("role")
    @classmethod
    def validate_role(cls, v: str) -> str:
        if v not in ("member", "admin"):
            raise ValueError("Role must be 'member' or 'admin'")
        return v


class InviteResponse(BaseModel):
    code: str
    org_id: str
    org_name: str
    email: Optional[str] = None
    role: str
    expires_at: str
    used: bool = False


class OrgMemberResponse(BaseModel):
    user_id: str
    email: str
    display_name: Optional[str] = None
    org_role: str
    status: str
    last_login_at: Optional[str] = None


class OrgDetailResponse(BaseModel):
    org_id: str
    name: str
    slug: str
    status: str
    plan: str
    max_users: int
    max_devices_per_user: int
    current_user_count: int
    members: List[OrgMemberResponse]


class OrgBrandResponse(BaseModel):
    org_id: str
    org_name: str
    brand_template_id: Optional[str] = None


class UpdateOrgBrandRequest(BaseModel):
    brand_template_id: str


class UpdateMemberRoleRequest(BaseModel):
    role: str

    @field_validator("role")
    @classmethod
    def validate_role(cls, v: str) -> str:
        if v not in ("member", "admin", "owner"):
            raise ValueError("Role must be 'member', 'admin', or 'owner'")
        return v
