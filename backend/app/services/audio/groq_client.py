"""
Groq API client for streaming Whisper transcription.
Supports Hindi + English with low latency.

Uses AsyncGroq for non-blocking I/O — no ThreadPoolExecutor required.
"""

from groq import AsyncGroq, RateLimitError
import asyncio
import os
import logging
import io
import wave
from typing import Dict, List

logger = logging.getLogger(__name__)


class GroqTranscriptionClient:
    """
    Groq API client for streaming Whisper transcription.
    Supports Hindi + English with low latency (~0.5-1s).

    All methods are async — safe to await directly on the event loop.
    """

    def __init__(self, api_key: str = None):
        self.api_key = api_key or os.getenv("GROQ_API_KEY")
        if not self.api_key:
            raise ValueError("GROQ_API_KEY not found in environment")

        self.client = AsyncGroq(api_key=self.api_key)
        logger.info("✅ Groq async client initialized")

    # ------------------------------------------------------------------
    # Internal helper: PCM → WAV bytes (CPU-bound, run in thread)
    # ------------------------------------------------------------------
    @staticmethod
    def _pcm_to_wav_bytes(audio_data: bytes, sample_rate: int = 16000) -> bytes:
        buf = io.BytesIO()
        with wave.open(buf, "wb") as wf:
            wf.setnchannels(1)
            wf.setsampwidth(2)
            wf.setframerate(sample_rate)
            wf.writeframes(audio_data)
        return buf.getvalue()

    # ------------------------------------------------------------------
    # Primary streaming transcription (replaces transcribe_audio_sync)
    # ------------------------------------------------------------------
    async def transcribe_audio_async(
        self,
        audio_data: bytes,
        language: str = "hi",
        prompt: str = None,
        translate_to_english: bool = True,
    ) -> dict:
        """
        Async transcription for the real-time streaming pipeline.

        Args:
            audio_data: Raw PCM audio (16kHz, mono, 16-bit)
            language: Language code (hi, en, or auto)
            prompt: Context prompt for better accuracy
            translate_to_english: If True, uses Whisper's direct translation mode
        """
        try:
            # Build WAV bytes on a thread so we don't block the event loop.
            wav_bytes = await asyncio.to_thread(self._pcm_to_wav_bytes, audio_data)

            streaming_model = os.getenv("GROQ_STREAMING_MODEL", "whisper-large-v3-turbo")
            translation_model = os.getenv("GROQ_TRANSLATION_MODEL", "whisper-large-v3")

            if translate_to_english:
                logger.debug("🔄 Using Whisper TRANSLATION mode (direct to English)")

                selected_translation_model = translation_model
                # Turbo does not support the translations endpoint.
                if selected_translation_model == "whisper-large-v3-turbo":
                    selected_translation_model = "whisper-large-v3"

                try:
                    translation = await self.client.audio.translations.create(
                        file=("audio.wav", wav_bytes),
                        model=selected_translation_model,
                        response_format="verbose_json",
                        temperature=0.0,
                    )
                except Exception as translate_err:
                    err_msg = str(translate_err)
                    if "does not support `translate`" not in err_msg:
                        raise

                    # Graceful fallback to plain transcription.
                    logger.warning(
                        "Model '%s' does not support translate; falling back to '%s'.",
                        selected_translation_model,
                        streaming_model,
                    )
                    transcription = await self.client.audio.transcriptions.create(
                        file=("audio.wav", wav_bytes),
                        model=streaming_model,
                        language=None,
                        prompt=prompt or "This is a business meeting.",
                        response_format="verbose_json",
                        temperature=0.0,
                    )
                    text = transcription.text.strip()
                    detected_language = getattr(transcription, "language", "unknown")
                    return {
                        "text": text,
                        "confidence": 1.0,
                        "language": detected_language,
                        "translated": False,
                        "source_language": detected_language,
                    }

                text = translation.text.strip()
                detected_lang = getattr(translation, "language", "auto")
                logger.info(
                    "✅ Translated to English (chars=%s, source_language=%s)",
                    len(text),
                    detected_lang,
                )
                return {
                    "text": text,
                    "confidence": 1.0,
                    "language": "en",
                    "translated": True,
                    "source_language": detected_lang,
                }

            else:
                # Transcription-only mode (no translation).
                transcription = await self.client.audio.transcriptions.create(
                    file=("audio.wav", wav_bytes),
                    model=streaming_model,
                    language=None,  # Auto-detect
                    prompt=prompt or "This is a business meeting.",
                    response_format="verbose_json",
                    temperature=0.0,
                )
                text = transcription.text.strip()
                detected_language = getattr(transcription, "language", "unknown")
                logger.info(
                    "🔍 Transcribed audio (detected_language=%s, chars=%s)",
                    detected_language,
                    len(text),
                )
                return {
                    "text": text,
                    "confidence": 1.0,
                    "language": detected_language,
                    "translated": False,
                    "original_text": None,
                }

        except RateLimitError as e:
            logger.error(f"❌ Groq Rate Limit Reached: {e}")
            return {"text": "", "confidence": 0.0, "error": "rate_limit_exceeded"}
        except Exception as e:
            logger.error(f"❌ Groq transcription error: {e}")
            return {"text": "", "confidence": 0.0, "error": str(e)}

    # ------------------------------------------------------------------
    # Legacy async method (simple, no translation) — kept for compat
    # ------------------------------------------------------------------
    async def transcribe_audio(
        self,
        audio_data: bytes,
        language: str = "hi",
        prompt: str = None,
    ) -> dict:
        """Transcribe audio using Groq Whisper Large v3 (async, no translation)."""
        try:
            wav_bytes = await asyncio.to_thread(self._pcm_to_wav_bytes, audio_data)
            transcription = await self.client.audio.transcriptions.create(
                file=("audio.wav", wav_bytes),
                model="whisper-large-v3",
                language=language if language != "auto" else None,
                prompt=prompt or "This is a business meeting in Hindi and English.",
                response_format="verbose_json",
                temperature=0.0,
            )
            return {
                "text": transcription.text.strip(),
                "confidence": 1.0,
                "language": getattr(transcription, "language", language),
                "duration": getattr(transcription, "duration", 0.0),
            }
        except Exception as e:
            logger.error(f"❌ Groq transcription error: {e}")
            return {"text": "", "confidence": 0.0, "error": str(e)}

    # ------------------------------------------------------------------
    # Full-audio transcription (post-meeting gold standard)
    # ------------------------------------------------------------------
    async def transcribe_full_audio(
        self,
        audio_data: bytes,
        language: str = "en",
        prompt: str = None,
    ) -> dict:
        """
        Transcribe a large audio file and return detailed segments.
        Used for post-meeting 'Gold Standard' recovery.
        """
        try:
            filename = "audio.wav"
            upload_bytes = audio_data

            looks_like_container = (
                audio_data.startswith(b"RIFF")
                or audio_data.startswith(b"OggS")
                or audio_data.startswith(b"ID3")
                or audio_data.startswith(b"\xff\xfb")
                or audio_data[4:8] == b"ftyp"
            )

            if not looks_like_container:
                upload_bytes = await asyncio.to_thread(self._pcm_to_wav_bytes, audio_data)
                filename = "audio.wav"
            elif audio_data.startswith(b"OggS"):
                filename = "audio.ogg"
            elif audio_data.startswith(b"ID3") or audio_data.startswith(b"\xff\xfb"):
                filename = "audio.mp3"
            elif audio_data[4:8] == b"ftyp":
                filename = "audio.m4a"

            max_upload_bytes = int(os.getenv("GROQ_MAX_UPLOAD_BYTES", str(24 * 1024 * 1024)))
            if len(upload_bytes) > max_upload_bytes:
                if looks_like_container and filename != "audio.wav":
                    raise ValueError(
                        f"groq_request_too_large_container: size={len(upload_bytes)} bytes exceeds "
                        f"GROQ_MAX_UPLOAD_BYTES={max_upload_bytes}. Use smaller audio, or provide WAV/PCM for chunked fallback."
                    )
                if filename == "audio.wav":
                    return await self._transcribe_large_wav_in_chunks(
                        wav_bytes=upload_bytes,
                        max_upload_bytes=max_upload_bytes,
                        prompt=prompt,
                    )

            result = await self.client.audio.translations.create(
                file=(filename, upload_bytes),
                model="whisper-large-v3",
                response_format="verbose_json",
                temperature=0.0,
                prompt=prompt or "This is a business meeting transcript.",
            )

            segments = []
            if hasattr(result, "segments"):
                for s in result.segments:
                    if isinstance(s, dict):
                        seg_text = (s.get("text", "") or "").strip()
                        seg_start = s.get("start", 0.0)
                        seg_end = s.get("end", 0.0)
                        seg_conf = s.get("avg_logprob", 1.0)
                    else:
                        seg_text = (getattr(s, "text", "") or "").strip()
                        seg_start = getattr(s, "start", 0.0)
                        seg_end = getattr(s, "end", 0.0)
                        seg_conf = getattr(s, "avg_logprob", 1.0)
                    segments.append(
                        {"text": seg_text, "start": seg_start, "end": seg_end, "confidence": seg_conf}
                    )

            return {
                "text": result.text.strip(),
                "segments": segments,
                "language": "en",
                "duration": getattr(result, "duration", 0.0),
            }

        except Exception as e:
            logger.error(f"❌ Groq full transcription error: {e}")
            return {"text": "", "segments": [], "error": str(e)}

    async def _transcribe_large_wav_in_chunks(
        self,
        wav_bytes: bytes,
        max_upload_bytes: int,
        prompt: str = None,
    ) -> Dict:
        """
        Chunk a large WAV into <= max_upload_bytes pieces and stitch transcript segments.
        Now async — each chunk is uploaded with await.
        """
        # Parse WAV header in a thread (blocking I/O)
        def _parse_wav(data: bytes):
            with wave.open(io.BytesIO(data), "rb") as wr:
                nchannels = wr.getnchannels()
                sampwidth = wr.getsampwidth()
                framerate = wr.getframerate()
                total_frames = wr.getnframes()
                pcm_frames = wr.readframes(total_frames)
            return nchannels, sampwidth, framerate, total_frames, pcm_frames

        nchannels, sampwidth, framerate, total_frames, pcm_frames = await asyncio.to_thread(
            _parse_wav, wav_bytes
        )

        bytes_per_frame = max(1, nchannels * sampwidth)
        chunk_target_bytes = max(1_000_000, max_upload_bytes - 256_000)
        frames_per_chunk = max(1, chunk_target_bytes // bytes_per_frame)

        segments: List[Dict] = []
        texts: List[str] = []
        duration_seconds = total_frames / float(framerate) if framerate else 0.0
        chunk_start_frame = 0
        chunk_index = 0

        while chunk_start_frame < total_frames:
            chunk_end_frame = min(total_frames, chunk_start_frame + frames_per_chunk)
            start_byte = chunk_start_frame * bytes_per_frame
            end_byte = chunk_end_frame * bytes_per_frame
            chunk_pcm = pcm_frames[start_byte:end_byte]

            # Build chunk WAV in a thread
            def _build_chunk_wav(pcm: bytes) -> bytes:
                chunk_io = io.BytesIO()
                with wave.open(chunk_io, "wb") as ww:
                    ww.setnchannels(nchannels)
                    ww.setsampwidth(sampwidth)
                    ww.setframerate(framerate)
                    ww.writeframes(pcm)
                return chunk_io.getvalue()

            chunk_bytes = await asyncio.to_thread(_build_chunk_wav, chunk_pcm)
            chunk_offset_sec = chunk_start_frame / float(framerate)

            logger.info(
                "📦 Groq chunked transcription: chunk=%s size=%s offset=%.2fs",
                chunk_index,
                len(chunk_bytes),
                chunk_offset_sec,
            )

            result = await self.client.audio.translations.create(
                file=(f"audio_chunk_{chunk_index}.wav", chunk_bytes),
                model="whisper-large-v3",
                response_format="verbose_json",
                temperature=0.0,
                prompt=prompt or "This is a business meeting transcript.",
            )

            chunk_text = (getattr(result, "text", "") or "").strip()
            if chunk_text:
                texts.append(chunk_text)

            if hasattr(result, "segments") and result.segments:
                for s in result.segments:
                    if isinstance(s, dict):
                        seg_text = (s.get("text", "") or "").strip()
                        seg_start = float(s.get("start", 0.0)) + chunk_offset_sec
                        seg_end = float(s.get("end", 0.0)) + chunk_offset_sec
                        seg_conf = s.get("avg_logprob", 1.0)
                    else:
                        seg_text = (getattr(s, "text", "") or "").strip()
                        seg_start = float(getattr(s, "start", 0.0)) + chunk_offset_sec
                        seg_end = float(getattr(s, "end", 0.0)) + chunk_offset_sec
                        seg_conf = getattr(s, "avg_logprob", 1.0)
                    segments.append(
                        {"text": seg_text, "start": seg_start, "end": seg_end, "confidence": seg_conf}
                    )

            chunk_start_frame = chunk_end_frame
            chunk_index += 1

        return {
            "text": " ".join(texts).strip(),
            "segments": segments,
            "language": "en",
            "duration": duration_seconds,
        }
