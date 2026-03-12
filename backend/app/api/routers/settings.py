from fastapi import APIRouter, Depends, HTTPException
import logging
from typing import List, Optional

try:
    from ..deps import get_current_user
    from ...schemas.user import User
    from ...schemas.settings import (
        SaveModelConfigRequest,
        SaveTranscriptConfigRequest,
        GetApiKeyRequest,
        UserApiKeySaveRequest,
        UserAIHostSkillRequest,
        UserAIHostSkillResponse,
        AIHostStyleItem,
        AIHostStylesListResponse,
        UserAIHostStyleCreateRequest,
        UserAIHostStyleUpdateRequest,
        UserAIHostStyleDefaultRequest,
    )
    from ...db import DatabaseManager
    from ...services.ai_participant import SYSTEM_HOST_SKILLS
    from ...services.ai_participant_skills import parse_skill_markdown
except (ImportError, ValueError):
    from api.deps import get_current_user
    from schemas.user import User
    from schemas.settings import (
        SaveModelConfigRequest,
        SaveTranscriptConfigRequest,
        GetApiKeyRequest,
        UserApiKeySaveRequest,
        UserAIHostSkillRequest,
        UserAIHostSkillResponse,
        AIHostStyleItem,
        AIHostStylesListResponse,
        UserAIHostStyleCreateRequest,
        UserAIHostStyleUpdateRequest,
        UserAIHostStyleDefaultRequest,
    )
    from db import DatabaseManager
    from services.ai_participant import SYSTEM_HOST_SKILLS
    from services.ai_participant_skills import parse_skill_markdown

# Initialize services
db = DatabaseManager()

router = APIRouter()
logger = logging.getLogger(__name__)


def mask_key(key: Optional[str]) -> Optional[str]:
    """Mask an API key for safe display in UI"""
    if not key:
        return None
    if key.startswith("****"):
        return key
    return "****************"  # Fixed masked placeholder


@router.post("/save-model-config")
async def save_model_config(
    request: SaveModelConfigRequest, current_user: User = Depends(get_current_user)
):
    """Save the model configuration"""
    await db.save_model_config(request.provider, request.model, request.whisperModel)
    if request.apiKey is not None:
        # Don't save if it's just the masked placeholder
        if (
            request.apiKey == "****************"
            or request.apiKey == "****"
            or (request.apiKey and "..." in request.apiKey)
        ):
            logger.info(
                f"Skipping save for masked API key (provider: {request.provider})"
            )
        else:
            # Save as personal key for isolation
            await db.save_user_api_key(
                current_user.email, request.provider, request.apiKey
            )
    return {"status": "success", "message": "Model configuration saved successfully"}


@router.get("/get-model-config")
async def get_model_config(current_user: User = Depends(get_current_user)):
    """Get the model configuration"""
    config = await db.get_model_config()
    if config:
        # HOTFIX: Migrate users away from retired models
        retired_models = ["gemini-1.5-pro", "gemini-1.5-flash", "gemini-2.0-flash", "gemini-1.5-pro-latest"]
        if config.get("model", "") in retired_models:
            logger.info(
                f"Migrating retired model {config['model']} to gemini-3-pro-preview"
            )
            config["model"] = "gemini-3-pro-preview"
            await db.save_model_config(
                config["provider"],
                "gemini-3-pro-preview",
                config.get("whisperModel", "large-v3"),
            )

        # Check if user has a personal API key for the provider
        user_key = await db.get_user_api_key(current_user.email, config["provider"])
        if user_key:
            config["apiKey"] = mask_key(user_key)
        else:
            # Fallback to system key
            system_key = await db.get_api_key(config["provider"])
            if system_key:
                config["apiKey"] = mask_key(system_key)
            else:
                # Fallback to Env Var check to satisfy frontend validation
                import os

                provider = config["provider"]
                env_key = None
                if provider == "gemini":
                    env_key = os.getenv("GEMINI_API_KEY")
                elif provider == "openai":
                    env_key = os.getenv("OPENAI_API_KEY")
                elif provider == "groq":
                    env_key = os.getenv("GROQ_API_KEY")
                elif provider == "claude":
                    env_key = os.getenv("ANTHROPIC_API_KEY")

                if env_key:
                    config["apiKey"] = mask_key("EXISTS")

    return config


@router.get("/get-transcript-config")
async def get_transcript_config(current_user: User = Depends(get_current_user)):
    """Get the current transcript configuration"""
    transcript_config = await db.get_transcript_config()
    if transcript_config:
        transcript_api_key = await db.get_transcript_api_key(
            transcript_config["provider"], user_email=current_user.email
        )
        if transcript_api_key:
            transcript_config["apiKey"] = mask_key(transcript_api_key)
    return transcript_config


@router.post("/save-transcript-config")
async def save_transcript_config(
    request: SaveTranscriptConfigRequest, current_user: User = Depends(get_current_user)
):
    """Save the transcript configuration"""
    await db.save_transcript_config(request.provider, request.model)
    if request.apiKey is not None:
        if (
            request.apiKey == "****************"
            or request.apiKey == "****"
            or (request.apiKey and "..." in request.apiKey)
        ):
            logger.info(
                f"Skipping save for masked transcript API key (provider: {request.provider})"
            )
        else:
            await db.save_user_api_key(
                current_user.email, request.provider, request.apiKey
            )
    return {
        "status": "success",
        "message": "Transcript configuration saved successfully",
    }


@router.post("/get-api-key")
async def get_api_key_api(
    request: GetApiKeyRequest, current_user: User = Depends(get_current_user)
):
    try:
        return await db.get_api_key(request.provider, user_email=current_user.email)
    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))


@router.post("/get-transcript-api-key")
async def get_transcript_api_key_api(
    request: GetApiKeyRequest, current_user: User = Depends(get_current_user)
):
    try:
        return await db.get_transcript_api_key(
            request.provider, user_email=current_user.email
        )
    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))


# --- User Personal API Keys Endpoints ---


@router.get("/api/user/keys")
async def get_user_keys(current_user: User = Depends(get_current_user)):
    """Get masked API keys for the current user"""
    try:
        return await db.get_user_api_keys(current_user.email)
    except Exception as e:
        logger.error(f"Error fetching user keys: {e}")
        raise HTTPException(status_code=500, detail="Failed to fetch keys")


@router.post("/api/user/keys")
async def save_user_key(
    request: UserApiKeySaveRequest, current_user: User = Depends(get_current_user)
):
    """Save/Update an encrypted API key for the current user"""
    try:
        await db.save_user_api_key(
            current_user.email, request.provider, request.api_key
        )
        return {"status": "success", "message": f"API key for {request.provider} saved"}
    except Exception as e:
        logger.error(f"Error saving user key: {e}")
        raise HTTPException(status_code=500, detail="Failed to save key")


@router.delete("/api/user/keys/{provider}")
async def delete_user_key(
    provider: str, current_user: User = Depends(get_current_user)
):
    """Delete an API key for the current user"""
    try:
        await db.delete_user_api_key(current_user.email, provider)
        return {"status": "success", "message": f"API key for {provider} deleted"}
    except Exception as e:
        logger.error(f"Error deleting user key: {e}")
        raise HTTPException(status_code=500, detail="Failed to delete key")


@router.get("/api/user/ai-host-skill", response_model=UserAIHostSkillResponse)
async def get_user_ai_host_skill(current_user: User = Depends(get_current_user)):
    """Get persisted AI host skill profile for current user."""
    try:
        skill = await db.get_user_ai_host_skill(current_user.email)
        if not skill:
            return UserAIHostSkillResponse(
                user_email=current_user.email,
                skill_markdown="",
                is_active=True,
                source="user",
            )
        return UserAIHostSkillResponse(
            user_email=skill["user_email"],
            skill_markdown=skill.get("skill_markdown") or "",
            is_active=bool(skill.get("is_active", True)),
            source="user",
        )
    except Exception as e:
        logger.error(f"Error fetching user ai host skill: {e}")
        raise HTTPException(status_code=500, detail="Failed to fetch AI host skill")


@router.post("/api/user/ai-host-skill", response_model=UserAIHostSkillResponse)
async def save_user_ai_host_skill(
    request: UserAIHostSkillRequest, current_user: User = Depends(get_current_user)
):
    """Save/update persisted AI host skill profile for current user."""
    skill_text = (request.skill_markdown or "").strip()
    if len(skill_text) > 20000:
        raise HTTPException(
            status_code=400, detail="AI host skill markdown exceeds max length (20000)"
        )
    try:
        saved = await db.upsert_user_ai_host_skill(
            current_user.email,
            skill_markdown=skill_text,
            is_active=bool(request.is_active),
        )
        return UserAIHostSkillResponse(
            user_email=saved["user_email"],
            skill_markdown=saved.get("skill_markdown") or "",
            is_active=bool(saved.get("is_active", True)),
            source="user",
        )
    except Exception as e:
        logger.error(f"Error saving user ai host skill: {e}")
        raise HTTPException(status_code=500, detail="Failed to save AI host skill")


@router.delete("/api/user/ai-host-skill")
async def delete_user_ai_host_skill(current_user: User = Depends(get_current_user)):
    """Delete persisted AI host skill profile for current user."""
    try:
        await db.delete_user_ai_host_skill(current_user.email)
        return {"status": "success", "message": "AI host skill deleted"}
    except Exception as e:
        logger.error(f"Error deleting user ai host skill: {e}")
        raise HTTPException(status_code=500, detail="Failed to delete AI host skill")


def _system_style_items(default_style_id: str) -> List[AIHostStyleItem]:
    items: List[AIHostStyleItem] = []
    for key, markdown in SYSTEM_HOST_SKILLS.items():
        style_id = f"system:{key}"
        parsed = parse_skill_markdown(markdown)
        items.append(
            AIHostStyleItem(
                id=style_id,
                name=str(parsed.get("name") or key.title()),
                source="system",
                read_only=True,
                is_default=(style_id == default_style_id),
                is_active=True,
                skill_markdown=markdown,
            )
        )
    return items


@router.get("/api/user/ai-host-styles", response_model=AIHostStylesListResponse)
async def list_user_ai_host_styles(current_user: User = Depends(get_current_user)):
    """List system read-only styles + user custom styles with default marker."""
    try:
        default_style_id = await db.get_user_ai_host_default_style_id(current_user.email)
        if not default_style_id:
            default_style_id = "system:facilitator"

        system_items = _system_style_items(default_style_id=default_style_id)
        custom_rows = await db.list_user_ai_host_styles(current_user.email)
        custom_items = [
            AIHostStyleItem(
                id=f"user:{row['id']}",
                name=row.get("name") or "Custom Style",
                source="user",
                read_only=False,
                is_default=(f"user:{row['id']}" == default_style_id),
                is_active=bool(row.get("is_active", True)),
                skill_markdown=row.get("skill_markdown") or "",
            )
            for row in custom_rows
        ]
        return AIHostStylesListResponse(
            styles=[*system_items, *custom_items],
            default_style_id=default_style_id,
        )
    except Exception as e:
        logger.error(f"Error listing ai host styles: {e}")
        raise HTTPException(status_code=500, detail="Failed to list AI host styles")


@router.post("/api/user/ai-host-styles", response_model=AIHostStyleItem)
async def create_user_ai_host_style(
    request: UserAIHostStyleCreateRequest,
    current_user: User = Depends(get_current_user),
):
    try:
        name = (request.name or "").strip()
        if not name:
            raise HTTPException(status_code=400, detail="Style name is required")
        markdown = (request.skill_markdown or "").strip()
        if len(markdown) > 20000:
            raise HTTPException(status_code=400, detail="Skill markdown exceeds max length (20000)")
        created = await db.create_user_ai_host_style(
            user_email=current_user.email,
            name=name,
            skill_markdown=markdown,
            is_active=bool(request.is_active),
        )
        style_id = f"user:{created['id']}"
        if request.set_default:
            await db.set_user_ai_host_default_style_id(current_user.email, style_id)
        return AIHostStyleItem(
            id=style_id,
            name=created.get("name") or "Custom Style",
            source="user",
            read_only=False,
            is_default=bool(request.set_default),
            is_active=bool(created.get("is_active", True)),
            skill_markdown=created.get("skill_markdown") or "",
        )
    except HTTPException:
        raise
    except Exception as e:
        logger.error(f"Error creating ai host style: {e}")
        raise HTTPException(status_code=500, detail="Failed to create AI host style")


@router.put("/api/user/ai-host-styles/{style_id}", response_model=AIHostStyleItem)
async def update_user_ai_host_style(
    style_id: str,
    request: UserAIHostStyleUpdateRequest,
    current_user: User = Depends(get_current_user),
):
    clean_id = (style_id or "").strip()
    if not clean_id.startswith("user:"):
        raise HTTPException(status_code=400, detail="Only user styles can be updated")
    row_id = clean_id.split("user:", 1)[1]
    markdown = request.skill_markdown
    if markdown is not None and len((markdown or "").strip()) > 20000:
        raise HTTPException(status_code=400, detail="Skill markdown exceeds max length (20000)")
    try:
        updated = await db.update_user_ai_host_style(
            user_email=current_user.email,
            style_id=row_id,
            name=request.name,
            skill_markdown=markdown,
            is_active=request.is_active,
        )
        if not updated:
            raise HTTPException(status_code=404, detail="Style not found")
        default_style_id = await db.get_user_ai_host_default_style_id(current_user.email)
        return AIHostStyleItem(
            id=clean_id,
            name=updated.get("name") or "Custom Style",
            source="user",
            read_only=False,
            is_default=(default_style_id == clean_id),
            is_active=bool(updated.get("is_active", True)),
            skill_markdown=updated.get("skill_markdown") or "",
        )
    except HTTPException:
        raise
    except Exception as e:
        logger.error(f"Error updating ai host style: {e}")
        raise HTTPException(status_code=500, detail="Failed to update AI host style")


@router.delete("/api/user/ai-host-styles/{style_id}")
async def delete_user_ai_host_style(
    style_id: str, current_user: User = Depends(get_current_user)
):
    clean_id = (style_id or "").strip()
    if not clean_id.startswith("user:"):
        raise HTTPException(status_code=400, detail="Only user styles can be deleted")
    row_id = clean_id.split("user:", 1)[1]
    try:
        deleted = await db.delete_user_ai_host_style(current_user.email, row_id)
        if not deleted:
            raise HTTPException(status_code=404, detail="Style not found")
        default_style_id = await db.get_user_ai_host_default_style_id(current_user.email)
        if default_style_id == clean_id:
            await db.set_user_ai_host_default_style_id(current_user.email, "system:facilitator")
        return {"status": "success", "message": "AI host style deleted"}
    except HTTPException:
        raise
    except Exception as e:
        logger.error(f"Error deleting ai host style: {e}")
        raise HTTPException(status_code=500, detail="Failed to delete AI host style")


@router.post("/api/user/ai-host-styles/default")
async def set_user_ai_host_default_style(
    request: UserAIHostStyleDefaultRequest,
    current_user: User = Depends(get_current_user),
):
    style_id = (request.style_id or "").strip()
    if not style_id:
        raise HTTPException(status_code=400, detail="style_id is required")
    if style_id.startswith("system:"):
        sys_key = style_id.split("system:", 1)[1]
        if sys_key not in SYSTEM_HOST_SKILLS:
            raise HTTPException(status_code=400, detail="Invalid system style")
    elif style_id.startswith("user:"):
        row_id = style_id.split("user:", 1)[1]
        row = await db.get_user_ai_host_style_by_id(current_user.email, row_id)
        if not row:
            raise HTTPException(status_code=404, detail="User style not found")
    else:
        raise HTTPException(status_code=400, detail="Invalid style_id format")
    try:
        saved = await db.set_user_ai_host_default_style_id(current_user.email, style_id)
        return {"status": "success", "default_style_id": saved}
    except Exception as e:
        logger.error(f"Error setting default ai host style: {e}")
        raise HTTPException(status_code=500, detail="Failed to set default AI host style")
