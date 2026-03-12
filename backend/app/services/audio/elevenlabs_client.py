"""
ElevenLabs Scribe v2 client for speech-to-text transcription.
Supports:
  - batch REST (`scribe_v2`) with diarization
  - realtime WebSocket (`scribe_v2_realtime`) with timestamps
"""

import asyncio
import base64
import io
import json
import logging
import os
import wave
from typing import Any, Dict, List, Optional
from urllib.parse import urlencode

import httpx

logger = logging.getLogger(__name__)

ELEVENLABS_API_BASE = "https://api.elevenlabs.io"
ELEVENLABS_WS_BASE = "wss://api.elevenlabs.io"


class ElevenLabsTranscriptionClient:
    def __init__(self, api_key: str = None, mode: str = "batch"):
        self.api_key = api_key or os.getenv("ELEVENLABS_API_KEY", "")
        if not self.api_key:
            raise ValueError(
                "ELEVENLABS_API_KEY not found. Set it in environment variables."
            )

        self.mode = mode.lower().strip()
        if self.mode not in ("batch", "stream"):
            raise ValueError(
                f"Invalid ElevenLabs mode: {self.mode}. Use 'batch' or 'stream'."
            )

        self._http_client: Optional[httpx.AsyncClient] = None
        self._ws = None
        self._ws_lock = asyncio.Lock()
        self._ws_connected = False

        logger.info("ElevenLabs Scribe client initialized (mode=%s)", self.mode)

    @staticmethod
    def _pcm_to_wav_bytes(audio_data: bytes, sample_rate: int = 16000) -> bytes:
        buf = io.BytesIO()
        with wave.open(buf, "wb") as wf:
            wf.setnchannels(1)
            wf.setsampwidth(2)
            wf.setframerate(sample_rate)
            wf.writeframes(audio_data)
        return buf.getvalue()

    async def _get_http_client(self) -> httpx.AsyncClient:
        if self._http_client is None or self._http_client.is_closed:
            self._http_client = httpx.AsyncClient(
                base_url=ELEVENLABS_API_BASE,
                headers={"xi-api-key": self.api_key},
                timeout=60.0,
            )
        return self._http_client

    @staticmethod
    def _to_float(value: Any, default: float = 0.0) -> float:
        try:
            return float(value)
        except Exception:
            return default

    def _normalize_words(self, words: Any) -> List[Dict[str, Any]]:
        normalized: List[Dict[str, Any]] = []
        if not isinstance(words, list):
            return normalized

        for item in words:
            if not isinstance(item, dict):
                continue
            text = str(item.get("text") or item.get("word") or "").strip()
            if not text:
                continue
            speaker_id = item.get("speaker_id")
            start = self._to_float(item.get("start") or item.get("start_time"))
            end = self._to_float(item.get("end") or item.get("end_time"), start)
            normalized.append(
                {
                    "text": text,
                    "start": start,
                    "end": max(end, start),
                    "speaker_id": speaker_id,
                    "type": item.get("type"),
                    "confidence": item.get("confidence"),
                }
            )

        return normalized

    def _build_speaker_turns(self, words: List[Dict[str, Any]]) -> List[Dict[str, Any]]:
        turns: List[Dict[str, Any]] = []
        if not words:
            return turns

        current: Optional[Dict[str, Any]] = None
        for word in words:
            speaker = word.get("speaker_id")
            if speaker is None:
                speaker = "unknown"
            speaker_label = f"Speaker {speaker}"
            token = word.get("text", "").strip()
            if not token:
                continue

            if current is None or current["speaker"] != speaker_label:
                if current is not None:
                    current["text"] = " ".join(current.pop("tokens")).strip()
                    turns.append(current)
                current = {
                    "speaker": speaker_label,
                    "speaker_id": speaker,
                    "start": word.get("start", 0.0),
                    "end": word.get("end", 0.0),
                    "tokens": [token],
                }
            else:
                current["tokens"].append(token)
                current["end"] = word.get("end", current.get("end", 0.0))

        if current is not None:
            current["text"] = " ".join(current.pop("tokens")).strip()
            turns.append(current)

        return turns

    def _build_text_segments(
        self, words: List[Dict[str, Any]], text: str
    ) -> List[Dict[str, Any]]:
        turns = self._build_speaker_turns(words)
        if turns:
            return [
                {
                    "start": self._to_float(turn.get("start"), 0.0),
                    "end": self._to_float(turn.get("end"), 0.0),
                    "text": str(turn.get("text") or "").strip(),
                    "speaker": str(turn.get("speaker") or "Speaker 0"),
                }
                for turn in turns
                if str(turn.get("text") or "").strip()
            ]

        cleaned = str(text or "").strip()
        if not cleaned:
            return []
        return [{"start": 0.0, "end": 0.0, "text": cleaned, "speaker": "Speaker 0"}]

    async def _transcribe_batch(
        self,
        audio_data: bytes,
        language: str = "auto",
        translate_to_english: bool = True,
    ) -> dict:
        client = await self._get_http_client()
        wav_bytes = await asyncio.to_thread(self._pcm_to_wav_bytes, audio_data)

        diarize_enabled = (
            os.getenv("ELEVENLABS_DIARIZATION_ENABLED", "true").lower() == "true"
        )
        num_speakers_raw = str(os.getenv("ELEVENLABS_NUM_SPEAKERS", "")).strip()
        diarization_threshold_raw = str(
            os.getenv("ELEVENLABS_DIARIZATION_THRESHOLD", "")
        ).strip()

        files = {"file": ("audio.wav", wav_bytes, "audio/wav")}
        data: Dict[str, str] = {
            "model_id": "scribe_v2",
            "timestamps_granularity": "word",
            "tag_audio_events": "false",
            "diarize": "true" if diarize_enabled else "false",
        }
        if language and language != "auto":
            data["language_code"] = language
        if num_speakers_raw:
            data["num_speakers"] = num_speakers_raw
        if diarization_threshold_raw:
            data["diarization_threshold"] = diarization_threshold_raw

        try:
            response = await client.post("/v1/speech-to-text", files=files, data=data)
            if response.status_code == 429:
                return {"text": "", "confidence": 0.0, "error": "rate_limit_exceeded"}
            if response.status_code == 401:
                return {"text": "", "confidence": 0.0, "error": "invalid_api_key"}

            response.raise_for_status()
            result = response.json()

            text = str(result.get("text") or "").strip()
            detected_language = str(result.get("language_code") or "unknown")
            language_probability = self._to_float(
                result.get("language_probability"), 0.0
            )

            words = self._normalize_words(result.get("words"))
            speaker_turns = self._build_speaker_turns(words)
            segments = self._build_text_segments(words, text)
            diarized = any(w.get("speaker_id") not in (None, "", "unknown") for w in words)

            logger.info(
                "ElevenLabs batch transcription complete (chars=%s, words=%s, diarized=%s)",
                len(text),
                len(words),
                diarized,
            )

            return {
                "text": text,
                "confidence": language_probability if language_probability else 1.0,
                "language": detected_language,
                "translated": False,
                "source_language": detected_language,
                "words": words,
                "speaker_turns": speaker_turns,
                "segments": segments,
                "diarized": diarized,
                "model": "scribe_v2",
            }
        except httpx.HTTPStatusError as e:
            logger.error(
                "ElevenLabs batch HTTP error: %s %s",
                e.response.status_code,
                (e.response.text or "")[:240],
            )
            return {"text": "", "confidence": 0.0, "error": str(e)}
        except Exception as e:
            logger.error("ElevenLabs batch transcription error: %s", e)
            return {"text": "", "confidence": 0.0, "error": str(e)}

    async def _ensure_ws_connection(self, language: str = "auto") -> None:
        async with self._ws_lock:
            if self._ws_connected and self._ws is not None:
                return

            try:
                import websockets

                params = {
                    "model_id": "scribe_v2_realtime",
                    "xi-api-key": self.api_key,
                    "include_timestamps": "true",
                }
                if language and language != "auto":
                    params["language_code"] = language
                url = f"{ELEVENLABS_WS_BASE}/v1/speech-to-text/realtime?{urlencode(params)}"

                self._ws = await websockets.connect(
                    url,
                    ping_interval=20,
                    ping_timeout=10,
                    close_timeout=5,
                )
                self._ws_connected = True
                logger.info("ElevenLabs realtime WebSocket connected")
            except Exception as e:
                self._ws_connected = False
                self._ws = None
                logger.error("ElevenLabs WebSocket connection failed: %s", e)
                raise

    async def _transcribe_stream(
        self,
        audio_data: bytes,
        language: str = "auto",
        translate_to_english: bool = True,
    ) -> dict:
        try:
            await self._ensure_ws_connection(language)
            if self._ws is None:
                return {"text": "", "confidence": 0.0, "error": "ws_not_connected"}

            audio_b64 = base64.b64encode(audio_data).decode("utf-8")
            await self._ws.send(
                json.dumps(
                    {
                        "message_type": "input_audio_chunk",
                        "audio_base_64": audio_b64,
                        "commit": True,
                    }
                )
            )

            collected_text = ""
            collected_words: List[Dict[str, Any]] = []
            detected_lang = "unknown"
            last_msg: Dict[str, Any] = {}
            timeout = 8.0
            start = asyncio.get_event_loop().time()

            while (asyncio.get_event_loop().time() - start) < timeout:
                try:
                    raw = await asyncio.wait_for(self._ws.recv(), timeout=4.0)
                    msg = json.loads(raw)
                    if not isinstance(msg, dict):
                        continue
                    last_msg = msg
                    msg_type = str(msg.get("message_type") or msg.get("type") or "")

                    if msg_type in (
                        "committed_transcript",
                        "committed_transcript_with_timestamps",
                    ):
                        collected_text = str(msg.get("text") or "").strip()
                        collected_words = self._normalize_words(msg.get("words"))
                        detected_lang = str(msg.get("language_code") or detected_lang)
                        break
                    if msg_type == "error":
                        error_msg = (
                            msg.get("message")
                            or msg.get("error")
                            or "elevenlabs_ws_error"
                        )
                        return {"text": "", "confidence": 0.0, "error": str(error_msg)}
                except asyncio.TimeoutError:
                    break

            if not collected_text:
                detected_lang = str(last_msg.get("language_code") or detected_lang)

            speaker_turns = self._build_speaker_turns(collected_words)
            diarized = any(
                w.get("speaker_id") not in (None, "", "unknown")
                for w in collected_words
            )

            return {
                "text": collected_text,
                "confidence": 1.0,
                "language": detected_lang,
                "translated": False,
                "source_language": detected_lang,
                "words": collected_words,
                "speaker_turns": speaker_turns,
                "segments": self._build_text_segments(collected_words, collected_text),
                "diarized": diarized,
                "model": "scribe_v2_realtime",
            }
        except Exception as e:
            logger.error("ElevenLabs stream transcription error: %s", e)
            await self._close_ws()
            return {"text": "", "confidence": 0.0, "error": str(e)}

    async def _close_ws(self) -> None:
        async with self._ws_lock:
            if self._ws is not None:
                try:
                    await self._ws.close()
                except Exception:
                    pass
                self._ws = None
                self._ws_connected = False

    async def transcribe_audio_async(
        self,
        audio_data: bytes,
        language: str = "auto",
        prompt: Optional[str] = None,
        translate_to_english: bool = True,
    ) -> dict:
        if self.mode == "stream":
            return await self._transcribe_stream(audio_data, language, translate_to_english)
        return await self._transcribe_batch(audio_data, language, translate_to_english)

    async def transcribe_full_audio(
        self,
        audio_data: bytes,
        language: str = "auto",
    ) -> dict:
        """
        Full-audio transcription contract used by uploaded-file processing.
        """
        result = await self._transcribe_batch(
            audio_data=audio_data,
            language=language,
            translate_to_english=False,
        )
        if result.get("error"):
            return result
        return {
            "text": result.get("text") or "",
            "segments": result.get("segments") or [],
            "words": result.get("words") or [],
            "language": result.get("language"),
            "diarized": bool(result.get("diarized")),
            "model": result.get("model") or "scribe_v2",
        }

    def cleanup(self) -> None:
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
