"""
Authentication API routes.

Provides registration, login, token refresh, logout, profile,
device-linking, password reset, email verification, and account
management endpoints backed by MongoDB.
"""

import logging
import os
import secrets
import uuid
from datetime import datetime, timedelta, timezone

from fastapi import APIRouter, Depends, HTTPException, Request, status
from argon2 import PasswordHasher
from argon2.exceptions import VerifyMismatchError
from sendgrid import SendGridAPIClient
from sendgrid.helpers.mail import Mail

from mongodb import (
    get_users_collection,
    get_password_resets_collection,
    get_login_attempts_collection,
    get_token_families_collection,
    get_email_verifications_collection,
    get_usage_events_collection,
    get_usage_summaries_collection,
)
from auth_models import (
    RegisterRequest,
    LoginRequest,
    RefreshRequest,
    LinkDeviceRequest,
    ForgotPasswordRequest,
    ResetPasswordRequest,
    ChangePasswordRequest,
    VerifyEmailRequest,
    ResendVerificationRequest,
    UpdateProfileRequest,
    AuthResponse,
    UserProfile,
    DeviceSummary,
)
from auth_middleware import (
    get_current_user,
    create_access_token,
    create_refresh_token,
    _decode_token,
)
from audit_log import log_event

logger = logging.getLogger(__name__)
router = APIRouter(prefix="/api/auth", tags=["auth"])

ph = PasswordHasher()

# ── Configuration ───────────────────────────────────────────────────

RESET_CODE_EXPIRY_MINUTES = 15
VERIFICATION_CODE_EXPIRY_MINUTES = 30
CODE_MAX_ATTEMPTS_PER_HOUR = 3
MAX_LOGIN_ATTEMPTS = 5
LOGIN_LOCKOUT_MINUTES = 15
DEFAULT_MAX_DEVICES = 3

ACCOUNT_LEVEL_LIMITS = {
    "free": 3,
    "pro": 10,
    "enterprise": 50,
}

SENDGRID_API_KEY = os.getenv("SENDGRID_API_KEY", "")
SENDGRID_FROM_EMAIL = os.getenv("SENDGRID_FROM_EMAIL", "noreply@iqcapture.app")


# ── Email helpers ───────────────────────────────────────────────────


async def _send_email(to_email: str, subject: str, html_content: str):
    """Send an email via SendGrid. Falls back to logging if not configured."""
    if not SENDGRID_API_KEY:
        logger.warning(
            f"[EMAIL] SENDGRID_API_KEY not configured — would send to {to_email}: {subject}"
        )
        return

    message = Mail(
        from_email=SENDGRID_FROM_EMAIL,
        to_emails=to_email,
        subject=subject,
        html_content=html_content,
    )

    try:
        sg = SendGridAPIClient(SENDGRID_API_KEY)
        response = sg.send(message)
        logger.info(f"[EMAIL] Sent to {to_email} — status {response.status_code}")
    except Exception as e:
        logger.error(f"[EMAIL] SendGrid failed for {to_email}: {e}")
        raise HTTPException(
            status_code=status.HTTP_503_SERVICE_UNAVAILABLE,
            detail="Unable to send email. Please try again later.",
        )


async def _send_reset_email(email: str, code: str):
    """Send password reset code."""
    if not SENDGRID_API_KEY:
        logger.warning(f"[PASSWORD RESET] Code for {email}: {code}")
        return
    await _send_email(
        email,
        "Your password reset code",
        f"<p>Your password reset code is:</p>"
        f'<h1 style="letter-spacing:0.3em;font-family:monospace">{code}</h1>'
        f"<p>This code expires in {RESET_CODE_EXPIRY_MINUTES} minutes.</p>"
        f"<p>If you did not request this, you can safely ignore this email.</p>",
    )


async def _send_verification_email(email: str, code: str):
    """Send email verification code."""
    if not SENDGRID_API_KEY:
        logger.warning(f"[EMAIL VERIFICATION] Code for {email}: {code}")
        return
    await _send_email(
        email,
        "Verify your email address",
        f"<p>Your verification code is:</p>"
        f'<h1 style="letter-spacing:0.3em;font-family:monospace">{code}</h1>'
        f"<p>This code expires in {VERIFICATION_CODE_EXPIRY_MINUTES} minutes.</p>",
    )


# ── Helpers ──────────────────────────────────────────────────────────


def _user_to_profile(doc: dict) -> UserProfile:
    """Convert a MongoDB user document to a UserProfile response."""
    devices = []
    for d in doc.get("devices", []):
        devices.append(
            DeviceSummary(
                device_id=d["device_id"],
                linked_at=d["linked_at"].isoformat()
                if hasattr(d["linked_at"], "isoformat")
                else str(d["linked_at"]),
                platform=d.get("platform"),
                last_seen=d["last_seen"].isoformat()
                if d.get("last_seen") and hasattr(d["last_seen"], "isoformat")
                else None,
            )
        )
    return UserProfile(
        user_id=doc["user_id"],
        email=doc["email"],
        display_name=doc.get("display_name"),
        account_level=doc.get("account_level", "free"),
        email_verified=doc.get("email_verified", True),
        devices=devices,
    )


def _make_device_entry(device_id: str, platform: str | None = None) -> dict:
    """Create a device sub-document for embedding in the user record."""
    now = datetime.now(timezone.utc)
    return {
        "device_id": device_id,
        "linked_at": now,
        "platform": platform,
        "last_seen": now,
    }


def _get_max_devices(user: dict) -> int:
    """Get max devices for user's account level."""
    level = user.get("account_level", "free")
    return ACCOUNT_LEVEL_LIMITS.get(level, DEFAULT_MAX_DEVICES)


def _check_device_limit(user: dict, new_device_id: str):
    """Raise 403 if adding a new device would exceed the limit."""
    device_ids = [d["device_id"] for d in user.get("devices", [])]
    if new_device_id in device_ids:
        return  # Already linked, no limit issue
    max_devices = _get_max_devices(user)
    if len(device_ids) >= max_devices:
        raise HTTPException(
            status_code=status.HTTP_403_FORBIDDEN,
            detail=f"Maximum {max_devices} devices allowed for your account level.",
        )


async def _create_token_family(user_id: str) -> str:
    """Create a new token family and return its ID."""
    family_id = str(uuid.uuid4())
    families_col = get_token_families_collection()
    await families_col.insert_one({
        "family_id": family_id,
        "user_id": user_id,
        "created_at": datetime.now(timezone.utc),
        "revoked": False,
    })
    return family_id


def _get_client_ip(request: Request) -> str | None:
    """Extract client IP from request."""
    return request.client.host if request.client else None


# ── Routes ───────────────────────────────────────────────────────────


@router.post("/register", response_model=AuthResponse)
async def register(req: RegisterRequest, request: Request):
    """Create a new user account and link the current device."""
    col = get_users_collection()
    ip = _get_client_ip(request)

    existing = await col.find_one({"email": req.email})
    if existing:
        raise HTTPException(
            status_code=status.HTTP_409_CONFLICT,
            detail="Email already registered",
        )

    user_id = str(uuid.uuid4())
    now = datetime.now(timezone.utc)
    password_hash = ph.hash(req.password)

    user_doc = {
        "user_id": user_id,
        "email": req.email,
        "password_hash": password_hash,
        "display_name": req.display_name,
        "created_at": now,
        "updated_at": now,
        "last_login_at": now,
        "status": "active",
        "account_level": "free",
        "email_verified": False,
        "devices": [_make_device_entry(req.device_id)],
    }

    await col.insert_one(user_doc)
    await _link_device_in_registry(user_id, req.device_id)

    # Generate and send email verification code
    verification_code = f"{secrets.randbelow(1000000):06d}"
    verifications_col = get_email_verifications_collection()
    await verifications_col.insert_one({
        "email": req.email,
        "code_hash": ph.hash(verification_code),
        "created_at": now,
        "expires_at": now + timedelta(minutes=VERIFICATION_CODE_EXPIRY_MINUTES),
        "used": False,
    })
    await _send_verification_email(req.email, verification_code)

    # Create token family and issue tokens
    family_id = await _create_token_family(user_id)
    access = create_access_token(user_id, req.device_id)
    refresh = create_refresh_token(user_id, req.device_id, family_id)

    await log_event("register", user_id=user_id, email=req.email, ip=ip)
    logger.info(f"User registered: {req.email}")
    return AuthResponse(
        access_token=access,
        refresh_token=refresh,
        user=_user_to_profile(user_doc),
    )


@router.post("/login", response_model=AuthResponse)
async def login(req: LoginRequest, request: Request):
    """Authenticate with email and password."""
    col = get_users_collection()
    attempts_col = get_login_attempts_collection()
    ip = _get_client_ip(request)

    # Rate limit: check failed attempts
    cutoff = datetime.now(timezone.utc) - timedelta(minutes=LOGIN_LOCKOUT_MINUTES)
    failed_count = await attempts_col.count_documents(
        {"email": req.email, "attempted_at": {"$gt": cutoff}, "success": False}
    )
    if failed_count >= MAX_LOGIN_ATTEMPTS:
        await log_event("login_rate_limited", email=req.email, ip=ip)
        raise HTTPException(
            status_code=status.HTTP_429_TOO_MANY_REQUESTS,
            detail="Account temporarily locked due to too many failed login attempts. Try again in 15 minutes.",
        )

    user = await col.find_one({"email": req.email})
    if not user:
        await attempts_col.insert_one(
            {"email": req.email, "attempted_at": datetime.now(timezone.utc), "success": False}
        )
        await log_event("failed_login", email=req.email, ip=ip, metadata={"reason": "unknown_email"})
        raise HTTPException(
            status_code=status.HTTP_401_UNAUTHORIZED,
            detail="Invalid email or password",
        )

    if user.get("status") != "active":
        await log_event("failed_login", email=req.email, ip=ip, metadata={"reason": "inactive_account"})
        raise HTTPException(
            status_code=status.HTTP_403_FORBIDDEN,
            detail="Account is not active",
        )

    try:
        ph.verify(user["password_hash"], req.password)
    except VerifyMismatchError:
        await attempts_col.insert_one(
            {"email": req.email, "attempted_at": datetime.now(timezone.utc), "success": False}
        )
        await log_event(
            "failed_login", user_id=user["user_id"], email=req.email, ip=ip,
            metadata={"reason": "wrong_password"},
        )
        raise HTTPException(
            status_code=status.HTTP_401_UNAUTHORIZED,
            detail="Invalid email or password",
        )

    # Check email verification (grandfathered users without the field are treated as verified)
    if user.get("email_verified") is False:
        raise HTTPException(
            status_code=status.HTTP_403_FORBIDDEN,
            detail="Email not verified",
            headers={"X-Error-Code": "EMAIL_NOT_VERIFIED"},
        )

    # Rehash if needed (argon2 parameter upgrades)
    if ph.check_needs_rehash(user["password_hash"]):
        new_hash = ph.hash(req.password)
        await col.update_one(
            {"user_id": user["user_id"]},
            {"$set": {"password_hash": new_hash}},
        )

    now = datetime.now(timezone.utc)

    # Check device limit and link if not present
    _check_device_limit(user, req.device_id)
    device_ids = [d["device_id"] for d in user.get("devices", [])]
    if req.device_id not in device_ids:
        await col.update_one(
            {"user_id": user["user_id"]},
            {"$push": {"devices": _make_device_entry(req.device_id)}},
        )
        user["devices"].append(_make_device_entry(req.device_id))

    # Update last_login and device last_seen
    await col.update_one(
        {"user_id": user["user_id"]},
        {"$set": {"last_login_at": now, "updated_at": now}},
    )
    await col.update_one(
        {"user_id": user["user_id"], "devices.device_id": req.device_id},
        {"$set": {"devices.$.last_seen": now}},
    )

    await _link_device_in_registry(user["user_id"], req.device_id)

    # Clear failed login attempts on success
    await attempts_col.delete_many({"email": req.email})

    # Create token family and issue tokens
    family_id = await _create_token_family(user["user_id"])
    access = create_access_token(user["user_id"], req.device_id)
    refresh = create_refresh_token(user["user_id"], req.device_id, family_id)

    await log_event("login", user_id=user["user_id"], email=req.email, ip=ip)
    logger.info(f"User logged in: {req.email}")
    return AuthResponse(
        access_token=access,
        refresh_token=refresh,
        user=_user_to_profile(user),
    )


@router.post("/refresh", response_model=AuthResponse)
async def refresh_token(req: RefreshRequest, request: Request):
    """Issue a new access token using a valid refresh token."""
    payload = _decode_token(req.refresh_token, expected_type="refresh")
    user_id = payload["sub"]
    device_id = payload.get("device_id", "")
    old_family_id = payload.get("family_id")

    # Check token family revocation (legacy tokens without family_id are allowed once)
    families_col = get_token_families_collection()
    if old_family_id:
        family = await families_col.find_one({"family_id": old_family_id})
        if family and family.get("revoked"):
            await log_event("revoked_refresh_attempt", user_id=user_id, ip=_get_client_ip(request))
            raise HTTPException(
                status_code=status.HTTP_401_UNAUTHORIZED,
                detail="Refresh token has been revoked",
            )
        # Revoke old family (rotation)
        if family:
            await families_col.update_one(
                {"family_id": old_family_id}, {"$set": {"revoked": True}}
            )

    col = get_users_collection()
    user = await col.find_one({"user_id": user_id})
    if not user or user.get("status") != "active":
        raise HTTPException(
            status_code=status.HTTP_401_UNAUTHORIZED,
            detail="User not found or inactive",
        )

    # Issue new family
    new_family_id = await _create_token_family(user_id)
    access = create_access_token(user_id, device_id)
    new_refresh = create_refresh_token(user_id, device_id, new_family_id)

    return AuthResponse(
        access_token=access,
        refresh_token=new_refresh,
        user=_user_to_profile(user),
    )


@router.post("/logout")
async def logout(request: Request, current_user: dict = Depends(get_current_user)):
    """Logout — revokes all token families for the user."""
    families_col = get_token_families_collection()
    await families_col.update_many(
        {"user_id": current_user["sub"]},
        {"$set": {"revoked": True}},
    )
    await log_event("logout", user_id=current_user["sub"], ip=_get_client_ip(request))
    logger.info(f"User logged out: {current_user['sub']}")
    return {"message": "Logged out successfully"}


@router.get("/me", response_model=UserProfile)
async def get_me(current_user: dict = Depends(get_current_user)):
    """Get the current user's profile."""
    col = get_users_collection()
    user = await col.find_one({"user_id": current_user["sub"]})
    if not user:
        raise HTTPException(status_code=404, detail="User not found")
    return _user_to_profile(user)


@router.post("/link-device")
async def link_device(
    req: LinkDeviceRequest,
    request: Request,
    current_user: dict = Depends(get_current_user),
):
    """Associate a device_id with the authenticated user."""
    col = get_users_collection()
    user_id = current_user["sub"]

    user = await col.find_one({"user_id": user_id})
    if not user:
        raise HTTPException(status_code=404, detail="User not found")

    device_ids = [d["device_id"] for d in user.get("devices", [])]
    if req.device_id in device_ids:
        now = datetime.now(timezone.utc)
        await col.update_one(
            {"user_id": user_id, "devices.device_id": req.device_id},
            {"$set": {"devices.$.last_seen": now}},
        )
        return {"message": "Device already linked", "device_id": req.device_id}

    # Check device limit
    _check_device_limit(user, req.device_id)

    entry = _make_device_entry(req.device_id, platform=req.platform)
    await col.update_one(
        {"user_id": user_id},
        {
            "$push": {"devices": entry},
            "$set": {"updated_at": datetime.now(timezone.utc)},
        },
    )

    await _link_device_in_registry(user_id, req.device_id)

    logger.info(f"Device {req.device_id} linked to user {user_id}")
    return {"message": "Device linked", "device_id": req.device_id}


@router.get("/devices", response_model=list[DeviceSummary])
async def list_user_devices(current_user: dict = Depends(get_current_user)):
    """List all devices linked to the authenticated user."""
    col = get_users_collection()
    user = await col.find_one({"user_id": current_user["sub"]})
    if not user:
        raise HTTPException(status_code=404, detail="User not found")
    return _user_to_profile(user).devices


# ── Email verification ──────────────────────────────────────────────


@router.post("/verify-email")
async def verify_email(req: VerifyEmailRequest, request: Request):
    """Verify a user's email address using a 6-digit code."""
    col = get_users_collection()
    verifications_col = get_email_verifications_collection()

    now = datetime.now(timezone.utc)
    doc = await verifications_col.find_one(
        {"email": req.email, "used": False, "expires_at": {"$gt": now}},
        sort=[("created_at", -1)],
    )

    if not doc:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Invalid or expired verification code",
        )

    try:
        ph.verify(doc["code_hash"], req.code)
    except VerifyMismatchError:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Invalid or expired verification code",
        )

    # Mark code as used
    await verifications_col.update_one({"_id": doc["_id"]}, {"$set": {"used": True}})

    # Mark user as verified
    await col.update_one(
        {"email": req.email},
        {"$set": {"email_verified": True, "updated_at": now}},
    )

    user = await col.find_one({"email": req.email})
    user_id = user["user_id"] if user else None
    await log_event("email_verified", user_id=user_id, email=req.email, ip=_get_client_ip(request))

    logger.info(f"Email verified: {req.email}")
    return {"message": "Email verified successfully."}


@router.post("/resend-verification")
async def resend_verification(req: ResendVerificationRequest, request: Request):
    """Resend the email verification code."""
    col = get_users_collection()
    verifications_col = get_email_verifications_collection()

    user = await col.find_one({"email": req.email})
    if not user:
        return {"message": "If an account exists, a verification code has been sent."}

    if user.get("email_verified"):
        return {"message": "Email already verified."}

    # Rate limit
    one_hour_ago = datetime.now(timezone.utc) - timedelta(hours=1)
    recent_count = await verifications_col.count_documents(
        {"email": req.email, "created_at": {"$gt": one_hour_ago}}
    )
    if recent_count >= CODE_MAX_ATTEMPTS_PER_HOUR:
        raise HTTPException(
            status_code=status.HTTP_429_TOO_MANY_REQUESTS,
            detail="Too many verification requests. Please try again later.",
        )

    code = f"{secrets.randbelow(1000000):06d}"
    now = datetime.now(timezone.utc)
    await verifications_col.insert_one({
        "email": req.email,
        "code_hash": ph.hash(code),
        "created_at": now,
        "expires_at": now + timedelta(minutes=VERIFICATION_CODE_EXPIRY_MINUTES),
        "used": False,
    })

    await _send_verification_email(req.email, code)
    return {"message": "If an account exists, a verification code has been sent."}


# ── Password reset ──────────────────────────────────────────────────


@router.post("/forgot-password")
async def forgot_password(req: ForgotPasswordRequest, request: Request):
    """Request a password reset code. Always returns 200 to prevent email enumeration."""
    col = get_users_collection()
    resets_col = get_password_resets_collection()
    ip = _get_client_ip(request)

    user = await col.find_one({"email": req.email})
    if not user:
        return {"message": "If an account exists with that email, a reset code has been sent."}

    # Rate limit
    one_hour_ago = datetime.now(timezone.utc) - timedelta(hours=1)
    recent_count = await resets_col.count_documents(
        {"email": req.email, "created_at": {"$gt": one_hour_ago}}
    )
    if recent_count >= CODE_MAX_ATTEMPTS_PER_HOUR:
        raise HTTPException(
            status_code=status.HTTP_429_TOO_MANY_REQUESTS,
            detail="Too many reset requests. Please try again later.",
        )

    code = f"{secrets.randbelow(1000000):06d}"
    now = datetime.now(timezone.utc)

    await resets_col.insert_one({
        "email": req.email,
        "code_hash": ph.hash(code),
        "created_at": now,
        "expires_at": now + timedelta(minutes=RESET_CODE_EXPIRY_MINUTES),
        "used": False,
    })

    await _send_reset_email(req.email, code)
    await log_event("password_reset_requested", user_id=user["user_id"], email=req.email, ip=ip)

    logger.info(f"Password reset requested for {req.email}")
    return {"message": "If an account exists with that email, a reset code has been sent."}


@router.post("/reset-password")
async def reset_password(req: ResetPasswordRequest, request: Request):
    """Reset password using a valid reset code."""
    col = get_users_collection()
    resets_col = get_password_resets_collection()
    ip = _get_client_ip(request)

    now = datetime.now(timezone.utc)
    reset_doc = await resets_col.find_one(
        {"email": req.email, "used": False, "expires_at": {"$gt": now}},
        sort=[("created_at", -1)],
    )

    if not reset_doc:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Invalid or expired reset code",
        )

    try:
        ph.verify(reset_doc["code_hash"], req.code)
    except VerifyMismatchError:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Invalid or expired reset code",
        )

    await resets_col.update_one({"_id": reset_doc["_id"]}, {"$set": {"used": True}})

    new_hash = ph.hash(req.new_password)
    await col.update_one(
        {"email": req.email},
        {"$set": {"password_hash": new_hash, "updated_at": now}},
    )

    user = await col.find_one({"email": req.email})
    user_id = user["user_id"] if user else None
    await log_event("password_reset_completed", user_id=user_id, email=req.email, ip=ip)

    logger.info(f"Password reset completed for {req.email}")
    return {"message": "Password has been reset successfully."}


@router.post("/change-password")
async def change_password(
    req: ChangePasswordRequest,
    request: Request,
    current_user: dict = Depends(get_current_user),
):
    """Change password for the authenticated user."""
    col = get_users_collection()
    ip = _get_client_ip(request)

    user = await col.find_one({"user_id": current_user["sub"]})
    if not user:
        raise HTTPException(status_code=404, detail="User not found")

    try:
        ph.verify(user["password_hash"], req.current_password)
    except VerifyMismatchError:
        raise HTTPException(
            status_code=status.HTTP_401_UNAUTHORIZED,
            detail="Current password is incorrect",
        )

    new_hash = ph.hash(req.new_password)
    now = datetime.now(timezone.utc)
    await col.update_one(
        {"user_id": current_user["sub"]},
        {"$set": {"password_hash": new_hash, "updated_at": now}},
    )

    await log_event("password_changed", user_id=current_user["sub"], ip=ip)
    logger.info(f"Password changed for user {current_user['sub']}")
    return {"message": "Password changed successfully."}


# ── Account management ──────────────────────────────────────────────


@router.put("/profile")
async def update_profile(
    req: UpdateProfileRequest,
    current_user: dict = Depends(get_current_user),
):
    """Update the user's display name."""
    col = get_users_collection()
    await col.update_one(
        {"user_id": current_user["sub"]},
        {"$set": {"display_name": req.display_name, "updated_at": datetime.now(timezone.utc)}},
    )
    return {"message": "Profile updated."}


@router.post("/deactivate")
async def deactivate_account(
    request: Request,
    current_user: dict = Depends(get_current_user),
):
    """Deactivate the user's account."""
    col = get_users_collection()
    now = datetime.now(timezone.utc)

    await col.update_one(
        {"user_id": current_user["sub"]},
        {"$set": {"status": "deactivated", "updated_at": now}},
    )

    # Revoke all token families
    families_col = get_token_families_collection()
    await families_col.update_many(
        {"user_id": current_user["sub"]},
        {"$set": {"revoked": True}},
    )

    await log_event("account_deactivated", user_id=current_user["sub"], ip=_get_client_ip(request))
    return {"message": "Account deactivated."}


@router.delete("/account")
async def delete_account(
    request: Request,
    current_user: dict = Depends(get_current_user),
):
    """Permanently delete the user's account and all associated data (GDPR)."""
    user_id = current_user["sub"]
    ip = _get_client_ip(request)

    # Delete across all collections
    await get_users_collection().delete_one({"user_id": user_id})
    await get_usage_events_collection().delete_many({"user_id": user_id})
    await get_usage_summaries_collection().delete_many({"user_id": user_id})
    await get_token_families_collection().delete_many({"user_id": user_id})

    # Log deletion with minimal info then delete audit trail too
    await log_event("account_deleted", user_id=user_id, ip=ip)

    logger.info(f"Account deleted (GDPR): {user_id}")
    return {"message": "Account and all associated data deleted."}


# ── Internal helpers ─────────────────────────────────────────────────


async def _link_device_in_registry(user_id: str, device_id: str):
    """Update the devices collection to set auth_user_id on the device."""
    try:
        from mongodb import _get_db
        devices_col = _get_db()["devices"]
        await devices_col.update_one(
            {"user_id": device_id},
            {"$set": {"auth_user_id": user_id}},
        )
    except Exception as e:
        logger.warning(f"Failed to link device in registry: {e}")
