"""
Organisation management API routes.

Provides endpoints for viewing org details, inviting users,
managing members, and listing invites. Requires authentication.
Owner/admin roles enforced where appropriate.
"""

import logging
import secrets
from datetime import datetime, timedelta, timezone
from typing import Optional, List

from fastapi import APIRouter, Depends, HTTPException, status

from mongodb import get_organisations_collection, get_invites_collection, get_users_collection
from auth_middleware import get_current_user
from org_models import (
    CreateInviteRequest,
    InviteResponse,
    OrgDetailResponse,
    OrgMemberResponse,
    OrgBrandResponse,
    UpdateOrgBrandRequest,
    UpdateMemberRoleRequest,
)

logger = logging.getLogger(__name__)
router = APIRouter(prefix="/api/org", tags=["organisation"])

INVITE_EXPIRY_DAYS = 7


def _require_org(current_user: dict) -> str:
    """Extract org_id from JWT or raise 403."""
    org_id = current_user.get("org_id")
    if not org_id:
        raise HTTPException(
            status_code=status.HTTP_403_FORBIDDEN,
            detail="You are not a member of any organisation",
        )
    return org_id


def _require_admin(current_user: dict) -> str:
    """Extract org_id and verify admin/owner role, or raise 403."""
    org_id = _require_org(current_user)
    role = current_user.get("org_role", "member")
    if role not in ("admin", "owner"):
        raise HTTPException(
            status_code=status.HTTP_403_FORBIDDEN,
            detail="Admin or owner role required",
        )
    return org_id


@router.get("", response_model=OrgDetailResponse)
async def get_org_detail(current_user: dict = Depends(get_current_user)):
    """Get the current user's organisation details and member list."""
    org_id = _require_org(current_user)
    org = await get_organisations_collection().find_one({"org_id": org_id})
    if not org:
        raise HTTPException(status_code=404, detail="Organisation not found")

    users_col = get_users_collection()
    members = []
    async for user in users_col.find({"org_id": org_id}, {"password_hash": 0}):
        members.append(OrgMemberResponse(
            user_id=user["user_id"],
            email=user["email"],
            display_name=user.get("display_name"),
            org_role=user.get("org_role", "member"),
            status=user.get("status", "active"),
            last_login_at=user["last_login_at"].isoformat()
            if user.get("last_login_at") and hasattr(user["last_login_at"], "isoformat")
            else None,
        ))

    user_count = await users_col.count_documents({"org_id": org_id})

    return OrgDetailResponse(
        org_id=org["org_id"],
        name=org["name"],
        slug=org["slug"],
        status=org.get("status", "active"),
        plan=org.get("plan", "free"),
        max_users=org.get("max_users", 25),
        max_devices_per_user=org.get("max_devices_per_user", 3),
        current_user_count=user_count,
        members=members,
    )


@router.post("/invites", response_model=InviteResponse)
async def create_invite(
    req: CreateInviteRequest,
    current_user: dict = Depends(get_current_user),
):
    """Create an invite code for the current org. Requires admin/owner role."""
    org_id = _require_admin(current_user)
    org = await get_organisations_collection().find_one({"org_id": org_id})
    if not org:
        raise HTTPException(status_code=404, detail="Organisation not found")

    if org.get("max_users"):
        current_count = await get_users_collection().count_documents({"org_id": org_id})
        if current_count >= org["max_users"]:
            raise HTTPException(
                status_code=status.HTTP_403_FORBIDDEN,
                detail=f"Organisation has reached its maximum of {org['max_users']} users",
            )

    now = datetime.now(timezone.utc)
    code = secrets.token_urlsafe(16)
    invite_doc = {
        "code": code,
        "org_id": org_id,
        "email": req.email,
        "role": req.role,
        "created_by": current_user["sub"],
        "created_at": now,
        "expires_at": now + timedelta(days=INVITE_EXPIRY_DAYS),
        "used": False,
    }

    await get_invites_collection().insert_one(invite_doc)
    logger.info(f"Invite created for org {org['name']} by {current_user['sub']}")

    return InviteResponse(
        code=code,
        org_id=org_id,
        org_name=org["name"],
        email=req.email,
        role=req.role,
        expires_at=invite_doc["expires_at"].isoformat(),
    )


@router.get("/invites", response_model=List[InviteResponse])
async def list_invites(current_user: dict = Depends(get_current_user)):
    """List pending invites for the current org. Requires admin/owner role."""
    org_id = _require_admin(current_user)
    org = await get_organisations_collection().find_one({"org_id": org_id}, {"name": 1})
    org_name = org["name"] if org else "Unknown"

    invites_col = get_invites_collection()
    results = []
    async for invite in invites_col.find({"org_id": org_id}).sort("created_at", -1):
        results.append(InviteResponse(
            code=invite["code"],
            org_id=org_id,
            org_name=org_name,
            email=invite.get("email"),
            role=invite.get("role", "member"),
            expires_at=invite["expires_at"].isoformat()
            if hasattr(invite["expires_at"], "isoformat")
            else str(invite["expires_at"]),
            used=invite.get("used", False),
        ))
    return results


@router.delete("/invites/{code}")
async def revoke_invite(code: str, current_user: dict = Depends(get_current_user)):
    """Revoke a pending invite. Requires admin/owner role."""
    org_id = _require_admin(current_user)
    result = await get_invites_collection().delete_one({"code": code, "org_id": org_id})
    if result.deleted_count == 0:
        raise HTTPException(status_code=404, detail="Invite not found")
    return {"message": "Invite revoked"}


@router.patch("/members/{user_id}/role")
async def update_member_role(
    user_id: str,
    req: UpdateMemberRoleRequest,
    current_user: dict = Depends(get_current_user),
):
    """Change a member's role. Only owners can promote to admin/owner."""
    org_id = _require_org(current_user)
    caller_role = current_user.get("org_role", "member")

    if req.role in ("admin", "owner") and caller_role != "owner":
        raise HTTPException(
            status_code=status.HTTP_403_FORBIDDEN,
            detail="Only the organisation owner can promote to admin or owner",
        )
    if caller_role == "admin" and req.role != "member":
        raise HTTPException(
            status_code=status.HTTP_403_FORBIDDEN,
            detail="Admins can only set members to 'member' role",
        )
    if caller_role == "member":
        raise HTTPException(
            status_code=status.HTTP_403_FORBIDDEN,
            detail="Insufficient permissions",
        )

    users_col = get_users_collection()
    result = await users_col.update_one(
        {"user_id": user_id, "org_id": org_id},
        {"$set": {"org_role": req.role, "updated_at": datetime.now(timezone.utc)}},
    )
    if result.matched_count == 0:
        raise HTTPException(status_code=404, detail="User not found in this organisation")

    return {"message": f"Role updated to {req.role}"}


@router.delete("/members/{user_id}")
async def remove_member(user_id: str, current_user: dict = Depends(get_current_user)):
    """Remove a member from the org. Requires admin/owner role. Cannot remove owner."""
    org_id = _require_admin(current_user)

    users_col = get_users_collection()
    target = await users_col.find_one({"user_id": user_id, "org_id": org_id})
    if not target:
        raise HTTPException(status_code=404, detail="User not found in this organisation")

    if target.get("org_role") == "owner":
        raise HTTPException(
            status_code=status.HTTP_403_FORBIDDEN,
            detail="Cannot remove the organisation owner",
        )

    await users_col.update_one(
        {"user_id": user_id},
        {"$set": {"org_id": None, "org_role": None, "updated_at": datetime.now(timezone.utc)}},
    )
    return {"message": "Member removed from organisation"}


@router.get("/brand", response_model=OrgBrandResponse)
async def get_org_brand(current_user: dict = Depends(get_current_user)):
    """Get the brand template ID for the current user's organisation."""
    org_id = _require_org(current_user)
    org = await get_organisations_collection().find_one({"org_id": org_id})
    if not org:
        raise HTTPException(status_code=404, detail="Organisation not found")

    settings = org.get("settings", {})
    return OrgBrandResponse(
        org_id=org["org_id"],
        org_name=org["name"],
        brand_template_id=settings.get("brand_template_id"),
    )


@router.put("/brand")
async def set_org_brand(
    req: UpdateOrgBrandRequest,
    current_user: dict = Depends(get_current_user),
):
    """Set the brand template for the organisation. Requires admin/owner role."""
    org_id = _require_admin(current_user)
    result = await get_organisations_collection().update_one(
        {"org_id": org_id},
        {"$set": {
            "settings.brand_template_id": req.brand_template_id,
            "updated_at": datetime.now(timezone.utc),
        }},
    )
    if result.matched_count == 0:
        raise HTTPException(status_code=404, detail="Organisation not found")
    return {"message": f"Brand template set to {req.brand_template_id}"}
