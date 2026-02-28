from fastapi import APIRouter, Depends, HTTPException
import logging
from typing import Optional

try:
    from ..deps import get_current_user
    from ...schemas.user import User
    from ...schemas.settings import (
        SaveModelConfigRequest,
        SaveTranscriptConfigRequest,
        GetApiKeyRequest,
        UserApiKeySaveRequest,
    )
    from ...db import DatabaseManager
except (ImportError, ValueError):
    from api.deps import get_current_user
    from schemas.user import User
    from schemas.settings import (
        SaveModelConfigRequest,
        SaveTranscriptConfigRequest,
        GetApiKeyRequest,
        UserApiKeySaveRequest,
    )
    from db import DatabaseManager

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
        # HOTFIX: Migrate users away from retired gemini-1.5 models
        if config.get("model", "") in ["gemini-1.5-flash", "gemini-1.5-pro"]:
            logger.info(
                f"Migrating retired model {config['model']} to gemini-2.5-flash"
            )
            config["model"] = "gemini-2.5-flash"
            await db.save_model_config(
                config["provider"],
                "gemini-2.5-flash",
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
