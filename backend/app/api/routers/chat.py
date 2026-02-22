from fastapi import APIRouter, Depends, HTTPException
from fastapi.responses import JSONResponse, StreamingResponse
from typing import Optional
import logging
import os

try:
    from ..deps import get_current_user
    from ...schemas.user import User
    from ...schemas.chat import ChatRequest, CatchUpRequest, SearchContextRequest
    from ...db import DatabaseManager
    from ...core.rbac import RBAC
    from ...services.chat import ChatService
    from ...services.gemini_client import stream_content_text_async
except (ImportError, ValueError):
    from api.deps import get_current_user
    from schemas.user import User
    from schemas.chat import ChatRequest, CatchUpRequest, SearchContextRequest
    from db import DatabaseManager
    from core.rbac import RBAC
    from services.chat import ChatService
    from services.gemini_client import stream_content_text_async

# Initialize services
db = DatabaseManager()
rbac = RBAC(db)
chat_service = ChatService(db)

router = APIRouter()
logger = logging.getLogger(__name__)


@router.post("/chat-meeting")
async def chat_meeting(
    request: ChatRequest, current_user: User = Depends(get_current_user)
):
    """
    Chat with a specific meeting using AI.
    Streams the response back to the client.
    """
    if not await rbac.can(current_user, "ai_interact", request.meeting_id):
        raise HTTPException(
            status_code=403, detail="Permission denied to chat with this meeting"
        )

    try:
        logger.info(f"Received chat request for meeting {request.meeting_id}")

        full_text = ""
        if request.context_text is not None:
            full_text = request.context_text
            logger.info("Using provided context_text for chat")
        else:
            meeting_data = await db.get_meeting(request.meeting_id)
            if meeting_data:
                transcripts = meeting_data.get("transcripts", [])
                if not transcripts:
                    chunk_data = await db.get_transcript_data(request.meeting_id)
                    if chunk_data and chunk_data.get("transcript"):
                        full_text = chunk_data.get("transcript")
                    elif chunk_data and chunk_data.get("transcript_text"):
                        full_text = chunk_data.get("transcript_text")
                else:
                    full_text = "\n".join([t["text"] for t in transcripts])
            else:
                logger.warning(
                    f"Meeting {request.meeting_id} not found in DB and no context provided."
                )

        if not full_text and not request.allowed_meeting_ids and not request.history:
            logger.info(
                "No context, history, or linked meetings. Proceeding with empty context."
            )

        stream_generator = await chat_service.chat_about_meeting(
            context=full_text,
            question=request.question,
            model=request.model,
            model_name=request.model_name,
            allowed_meeting_ids=request.allowed_meeting_ids,
            history=request.history,
            user_email=current_user.email,
        )

        return StreamingResponse(stream_generator, media_type="text/plain")

    except HTTPException:
        raise
    except Exception as e:
        logger.error(f"Error in chat_meeting: {str(e)}", exc_info=True)
        raise HTTPException(status_code=500, detail=str(e))


@router.post("/catch-up")
async def catch_up(
    request: CatchUpRequest, current_user: User = Depends(get_current_user)
):
    """
    Generate a quick bulleted summary of the meeting so far.
    For late joiners or participants who zoned out.
    Streams the response back for fast display.
    """
    try:
        logger.info(
            f"Catch-up request received with {len(request.transcripts)} transcripts"
        )

        normalized_lines = []
        for item in request.transcripts:
            if isinstance(item, str):
                text = item.strip()
                if text:
                    normalized_lines.append(text)
                continue

            if isinstance(item, dict):
                text = str(item.get("text", "")).strip()
                if not text:
                    continue
                timestamp = item.get("timestamp")
                if timestamp:
                    normalized_lines.append(f"[{timestamp}] {text}")
                else:
                    normalized_lines.append(text)

        full_text = "\n".join(normalized_lines)

        window_label = (
            f"last {request.window_minutes} minutes"
            if request.window_minutes
            else "entire meeting"
        )

        if not full_text or len(full_text.strip()) < 10:
            if request.window_minutes:
                return JSONResponse(
                    status_code=200,
                    content={
                        "summary": (
                            f"• In the {window_label}, there was little or no spoken discussion.\n"
                            f"• This window includes silence as requested."
                        )
                    },
                )
            return JSONResponse(
                status_code=400,
                content={"error": "Not enough transcript content to summarize yet."},
            )

        catch_up_prompt = f"""You are a meeting assistant. A participant just joined late or zoned out and needs a quick catch-up.

Requested catch-up window: {window_label}
Window start: {request.window_start_iso or "unknown"}
Window end: {request.window_end_iso or "unknown"}
Meeting elapsed time (seconds): {request.meeting_elapsed_seconds if request.meeting_elapsed_seconds is not None else "unknown"}

Important:
• The requested time window is based on real elapsed meeting time (wall-clock), including silence.
• If the transcript content in this window is sparse due to silence, explicitly say that.

Based on the meeting transcript content inside this window, provide a BRIEF bulleted summary of:
• Key topics discussed
• Important decisions made
• Action items mentioned
• Any deadlines or dates mentioned

Keep it SHORT (max 5-7 bullets). Start each bullet with "•".
Be conversational: "The team discussed..." not "Discussion of..."

Meeting Transcript:
---
{full_text}
---

Quick Catch-Up Summary:"""

        # Reuse chat_service logic or direct calls
        # Using a simple wrapper to stream response

        async def generate_catch_up():
            # For simplicity, reuse the same streaming logic pattern as chat_meeting
            # but with a fixed prompt as the question.
            # However, chat_about_meeting adds its own system prompt.
            # We should probably expose a generic generate method on ChatService.

            # Temporary implementation mimicking original main.py
            try:
                if request.model == "groq":
                    api_key = await db.get_api_key(
                        "groq", user_email=current_user.email
                    )
                    if not api_key:
                        api_key = os.getenv("GROQ_API_KEY")
                    if not api_key:
                        yield "Error: Groq API key not configured"
                        return

                    from groq import AsyncGroq

                    client = AsyncGroq(api_key=api_key)
                    stream = await client.chat.completions.create(
                        messages=[{"role": "user", "content": catch_up_prompt}],
                        model=request.model_name,
                        stream=True,
                        max_tokens=500,
                        temperature=0.3,
                    )
                    async for chunk in stream:
                        content = chunk.choices[0].delta.content or ""
                        if content:
                            yield content

                elif request.model == "gemini":
                    api_key = await db.get_api_key(
                        "gemini", user_email=current_user.email
                    )
                    if not api_key:
                        api_key = os.getenv("GOOGLE_API_KEY") or os.getenv(
                            "GEMINI_API_KEY"
                        )
                    if not api_key:
                        yield "Error: Gemini API key not configured"
                        return

                    model_name = request.model_name
                    if not model_name.startswith("gemini-"):
                        model_name = (
                            f"gemini-{model_name}"
                            if "gemini" not in model_name
                            else model_name
                        )

                    async for chunk_text in stream_content_text_async(
                        api_key=api_key,
                        model=model_name,
                        contents=catch_up_prompt,
                    ):
                        yield chunk_text
            except Exception as e:
                logger.error(f"Error generating catch-up: {e}")
                yield f"Error: {str(e)}"

        return StreamingResponse(generate_catch_up(), media_type="text/plain")

    except Exception as e:
        logger.error(f"Error in catch_up: {str(e)}", exc_info=True)
        raise HTTPException(status_code=500, detail=str(e))


@router.post("/search-context")
async def search_context_endpoint(request: SearchContextRequest):
    """
    Search across past meetings for relevant context.
    Returns matching chunks with source citations.
    """
    try:
        # Fallback to empty results for now as VectorDB logic is not fully migrated
        results = []
        return {
            "status": "success",
            "query": request.query,
            "results": results,
            "total_indexed": 0,
        }

    except Exception as e:
        logger.error(f"Error in search_context: {str(e)}", exc_info=True)
        raise HTTPException(status_code=500, detail=str(e))
