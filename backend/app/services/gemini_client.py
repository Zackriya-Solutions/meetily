import logging
from typing import Any, AsyncIterator, Dict, Optional

from google import genai

logger = logging.getLogger(__name__)


async def _close_client(client: genai.Client):
    try:
        await client.aio.aclose()
    except Exception:
        pass
    try:
        client.close()
    except Exception:
        pass


async def generate_content_text_async(
    api_key: str,
    model: str,
    contents: Any,
    config: Optional[Dict[str, Any]] = None,
) -> str:
    client = genai.Client(api_key=api_key)
    try:
        response = await client.aio.models.generate_content(
            model=model,
            contents=contents,
            config=config or {},
        )
        return getattr(response, "text", "") or ""
    finally:
        await _close_client(client)


def generate_content_text_sync(
    api_key: str,
    model: str,
    contents: Any,
    config: Optional[Dict[str, Any]] = None,
) -> str:
    client = genai.Client(api_key=api_key)
    try:
        response = client.models.generate_content(
            model=model,
            contents=contents,
            config=config or {},
        )
        return getattr(response, "text", "") or ""
    finally:
        try:
            client.close()
        except Exception:
            pass


async def stream_content_text_async(
    api_key: str,
    model: str,
    contents: Any,
    config: Optional[Dict[str, Any]] = None,
) -> AsyncIterator[str]:
    client = genai.Client(api_key=api_key)
    try:
        stream = await client.aio.models.generate_content_stream(
            model=model,
            contents=contents,
            config=config or {},
        )
        async for chunk in stream:
            text = getattr(chunk, "text", None)
            if text:
                yield text
    finally:
        await _close_client(client)


def generate_content_with_file_sync(
    api_key: str,
    model: str,
    prompt: str,
    file_path: str,
    mime_type: str,
    config: Optional[Dict[str, Any]] = None,
) -> str:
    client = genai.Client(api_key=api_key)
    uploaded = None
    try:
        uploaded = client.files.upload(file=file_path, config={"mime_type": mime_type})
        response = client.models.generate_content(
            model=model,
            contents=[prompt, uploaded],
            config=config or {},
        )
        return getattr(response, "text", "") or ""
    finally:
        if uploaded and getattr(uploaded, "name", None):
            try:
                client.files.delete(name=uploaded.name)
            except Exception as cleanup_err:
                logger.warning("Failed to delete Gemini uploaded file: %s", cleanup_err)
        try:
            client.close()
        except Exception:
            pass

