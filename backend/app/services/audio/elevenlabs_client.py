"""
ElevenLabs Scribe v2 client for speech-to-text transcription.
Supports two modes:
  - batch: REST API using scribe_v2 model (highest accuracy, ~1-3s latency)
  - stream: WebSocket API using scribe_v2_realtime model (~150ms latency)

Interface is compatible with GroqTranscriptionClient for drop-in replacement.
"""

import asyncio
import base64
import io
import json
import logging
import os
import wave
from typing import Dict, List, Optional

import httpx

logger = logging.getLogger(__name__)

# ElevenLabs API base
ELEVENLABS_API_BASE = "https://api.elevenlabs.io"
ELEVENLABS_WS_BASE = "wss://api.elevenlabs.io"


class ElevenLabsTranscriptionClient:
    """
    ElevenLabs Scribe v2 client for speech-to-text.
    Supports batch (REST) and stream (WebSocket) modes.

    Interface matches GroqTranscriptionClient:
      - transcribe_audio_async(audio_data, language, prompt, translate_to_english) -> dict
    """

    def __init__(self, api_key: str = None, mode: str = "batch"):
        self.api_key = api_key or os.getenv("ELEVENLABS_API_KEY", "")
        if not self.api_key:
            raise ValueError(
                "ELEVENLABS_API_KEY not found. Set it in environment variables."
            )

        self.mode = mode.lower().strip()
        if self.mode not in ("batch", "stream"):
            raise ValueError(f"Invalid ElevenLabs mode: {self.mode}. Use 'batch' or 'stream'.")

        # Batch mode: reusable async HTTP client
        self._http_client: Optional[httpx.AsyncClient] = None

        # Stream mode: persistent WebSocket connection state
        self._ws = None
        self._ws_lock = asyncio.Lock()
        self._ws_connected = False

        logger.info(
            "✅ ElevenLabs Scribe v2 client initialized (mode=%s)",
            self.mode,
        )

    # ── helpers ──────────────────────────────────────────────────────────

    @staticmethod
    def _pcm_to_wav_bytes(audio_data: bytes, sample_rate: int = 16000) -> bytes:
        """Convert raw PCM audio to WAV format for the REST API."""
        buf = io.BytesIO()
        with wave.open(buf, "wb") as wf:
            wf.setnchannels(1)
            wf.setsampwidth(2)  # 16-bit
            wf.setframerate(sample_rate)
            wf.writeframes(audio_data)
        return buf.getvalue()

    async def _get_http_client(self) -> httpx.AsyncClient:
        if self._http_client is None or self._http_client.is_closed:
            self._http_client = httpx.AsyncClient(
                base_url=ELEVENLABS_API_BASE,
                headers={"xi-api-key": self.api_key},
                timeout=30.0,
            )
        return self._http_client

    # ── batch (REST) mode ────────────────────────────────────────────────

    async def _transcribe_batch(
        self,
        audio_data: bytes,
        language: str = "auto",
        translate_to_english: bool = True,
    ) -> dict:
        """
        Transcribe audio using ElevenLabs REST API (scribe_v2 model).
        Sends PCM audio as WAV file to POST /v1/speech-to-text.
        """
        client = await self._get_http_client()

        # ElevenLabs REST API accepts file upload
        wav_bytes = await asyncio.to_thread(self._pcm_to_wav_bytes, audio_data)

        # Build form data
        files = {"file": ("audio.wav", wav_bytes, "audio/wav")}
        data: Dict[str, str] = {
            "model_id": "scribe_v2",
            "timestamps_granularity": "word",
            "tag_audio_events": "false",
        }

        # Language: ElevenLabs uses ISO 639-1/639-3 codes
        if language and language != "auto":
            data["language_code"] = language

        try:
            response = await client.post(
                "/v1/speech-to-text",
                files=files,
                data=data,
            )

            if response.status_code == 429:
                logger.warning("⚠️ ElevenLabs Rate Limit Reached")
                return {"text": "", "confidence": 0.0, "error": "rate_limit_exceeded"}

            if response.status_code == 401:
                logger.error("❌ ElevenLabs Invalid API Key")
                return {"text": "", "confidence": 0.0, "error": "invalid_api_key"}

            response.raise_for_status()
            result = response.json()

            text = (result.get("text") or "").strip()
            detected_language = result.get("language_code") or "unknown"
            language_probability = result.get("language_probability") or 0.0

            logger.info(
                "✅ ElevenLabs batch transcription (chars=%s, lang=%s, prob=%.2f)",
                len(text),
                detected_language,
                language_probability,
            )

            return {
                "text": text,
                "confidence": float(language_probability) if language_probability else 1.0,
                "language": detected_language,
                "translated": False,  # ElevenLabs STT doesn't translate
                "source_language": detected_language,
            }

        except httpx.HTTPStatusError as e:
            logger.error("❌ ElevenLabs HTTP error: %s %s", e.response.status_code, e.response.text[:200])
            return {"text": "", "confidence": 0.0, "error": str(e)}
        except Exception as e:
            logger.error("❌ ElevenLabs batch transcription error: %s", e)
            return {"text": "", "confidence": 0.0, "error": str(e)}

    # ── stream (WebSocket) mode ──────────────────────────────────────────

    async def _ensure_ws_connection(self, language: str = "auto") -> None:
        """Establish WebSocket connection if not already connected."""
        async with self._ws_lock:
            if self._ws_connected and self._ws is not None:
                return

            try:
                import websockets

                # Build WebSocket URL with query params
                # API key must be passed as query param for WebSocket auth
                params = [
                    f"model_id=scribe_v2_realtime",
                    f"xi-api-key={self.api_key}",
                    f"encoding=pcm_16000",
                ]
                if language and language != "auto":
                    params.append(f"language_code={language}")

                url = f"{ELEVENLABS_WS_BASE}/v1/speech-to-text?{'&'.join(params)}"

                self._ws = await websockets.connect(
                    url,
                    ping_interval=20,
                    ping_timeout=10,
                    close_timeout=5,
                )
                self._ws_connected = True
                logger.info("✅ ElevenLabs WebSocket connected (realtime mode)")
            except Exception as e:
                self._ws_connected = False
                self._ws = None
                logger.error("❌ ElevenLabs WebSocket connection failed: %s", e)
                raise

    async def _transcribe_stream(
        self,
        audio_data: bytes,
        language: str = "auto",
        translate_to_english: bool = True,
    ) -> dict:
        """
        Transcribe audio using ElevenLabs WebSocket API (scribe_v2_realtime).
        Sends raw PCM data as input_audio_chunk, collects committed_transcript.
        """
        try:
            await self._ensure_ws_connection(language)

            if self._ws is None:
                return {"text": "", "confidence": 0.0, "error": "ws_not_connected"}

            # Send audio chunk as base64-encoded PCM
            audio_b64 = base64.b64encode(audio_data).decode("utf-8")
            await self._ws.send(json.dumps({
                "type": "input_audio_chunk",
                "audio_chunk": audio_b64,
            }))

            # Send a commit message to force transcription of buffered audio
            await self._ws.send(json.dumps({
                "type": "commit",
            }))

            # Collect responses until we get a committed_transcript
            collected_text = ""
            timeout = 10.0  # max wait time
            start = asyncio.get_event_loop().time()

            while (asyncio.get_event_loop().time() - start) < timeout:
                try:
                    raw = await asyncio.wait_for(self._ws.recv(), timeout=5.0)
                    msg = json.loads(raw)
                    msg_type = msg.get("message_type") or msg.get("type", "")

                    if msg_type == "committed_transcript":
                        collected_text = (msg.get("text") or "").strip()
                        break
                    elif msg_type == "committed_transcript_with_timestamps":
                        collected_text = (msg.get("text") or "").strip()
                        break
                    elif msg_type == "partial_transcript":
                        # Keep collecting, wait for committed
                        continue
                    elif msg_type == "error":
                        error_msg = msg.get("message") or msg.get("error") or "unknown"
                        logger.error("❌ ElevenLabs WS error: %s", error_msg)
                        return {"text": "", "confidence": 0.0, "error": error_msg}
                    else:
                        # Other message types, keep waiting
                        continue
                except asyncio.TimeoutError:
                    logger.warning("⚠️ ElevenLabs WS timeout waiting for committed_transcript")
                    break

            detected_lang = "unknown"
            if isinstance(msg, dict):
                detected_lang = msg.get("language_code") or "unknown"

            logger.info(
                "✅ ElevenLabs stream transcription (chars=%s, lang=%s)",
                len(collected_text),
                detected_lang,
            )

            return {
                "text": collected_text,
                "confidence": 1.0,
                "language": detected_lang,
                "translated": False,
                "source_language": detected_lang,
            }

        except Exception as e:
            logger.error("❌ ElevenLabs stream transcription error: %s", e)
            # Reset connection on error
            await self._close_ws()
            return {"text": "", "confidence": 0.0, "error": str(e)}

    async def _close_ws(self) -> None:
        """Close WebSocket connection."""
        async with self._ws_lock:
            if self._ws is not None:
                try:
                    await self._ws.close()
                except Exception:
                    pass
                self._ws = None
                self._ws_connected = False

    # ── public interface (matches GroqTranscriptionClient) ───────────────

    async def transcribe_audio_async(
        self,
        audio_data: bytes,
        language: str = "auto",
        prompt: Optional[str] = None,
        translate_to_english: bool = True,
    ) -> dict:
        """
        Transcribe audio using ElevenLabs Scribe v2.
        Interface matches GroqTranscriptionClient.transcribe_audio_async().

        Args:
            audio_data: Raw PCM audio (16kHz, mono, 16-bit)
            language: Language code (auto, en, hi, etc.)
            prompt: Context prompt (not used by ElevenLabs but kept for interface compat)
            translate_to_english: Not directly supported by ElevenLabs STT

        Returns:
            dict with keys: text, confidence, language, translated, source_language
            On error: dict with text="", confidence=0.0, error=<message>
        """
        if self.mode == "stream":
            return await self._transcribe_stream(audio_data, language, translate_to_english)
        else:
            return await self._transcribe_batch(audio_data, language, translate_to_english)

    def cleanup(self) -> None:
        """Clean up resources. For stream mode, schedule WS close."""
        if self._ws is not None:
            try:
                loop = asyncio.get_event_loop()
                if loop.is_running():
                    loop.create_task(self._close_ws())
                else:
                    loop.run_until_complete(self._close_ws())
            except Exception:
                pass

        if self._http_client is not None:
            try:
                loop = asyncio.get_event_loop()
                if loop.is_running():
                    loop.create_task(self._http_client.aclose())
                else:
                    loop.run_until_complete(self._http_client.aclose())
            except Exception:
                pass

        logger.info("ElevenLabs client cleanup complete (mode=%s)", self.mode)
