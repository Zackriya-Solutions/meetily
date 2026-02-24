"""
Groq API client for streaming Whisper transcription.
Supports Hindi + English with low latency.
"""

from groq import Groq, RateLimitError
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
    """

    def __init__(self, api_key: str = None):
        self.api_key = api_key or os.getenv("GROQ_API_KEY")
        if not self.api_key:
            raise ValueError("GROQ_API_KEY not found in environment")

        self.client = Groq(api_key=self.api_key)
        logger.info("✅ Groq client initialized")

    async def transcribe_audio(
        self,
        audio_data: bytes,
        language: str = "hi",
        prompt: str = None
    ) -> dict:
        """
        Transcribe audio using Groq Whisper Large v3.

        Args:
            audio_data: Raw PCM audio (16kHz, mono, 16-bit)
            language: Language code (hi, en, or auto)
            prompt: Context prompt for better accuracy

        Returns:
            {
                "text": "transcribed text",
                "confidence": 0.95,
                "language": "hi",
                "duration": 2.5
            }
        """
        try:
            # Convert PCM to WAV format for Groq API
            wav_buffer = io.BytesIO()
            with wave.open(wav_buffer, 'wb') as wav_file:
                wav_file.setnchannels(1)  # Mono
                wav_file.setsampwidth(2)  # 16-bit
                wav_file.setframerate(16000)  # 16kHz
                wav_file.writeframes(audio_data)

            wav_buffer.seek(0)

            # Call Groq API (synchronous, but fast ~0.5s)
            transcription = self.client.audio.transcriptions.create(
                file=("audio.wav", wav_buffer.read()),
                model="whisper-large-v3",
                language=language if language != "auto" else None,
                prompt=prompt or "This is a business meeting in Hindi and English.",
                response_format="verbose_json",  # Get confidence scores
                temperature=0.0  # Deterministic output
            )

            return {
                "text": transcription.text.strip(),
                "confidence": 1.0,  # Groq doesn't return confidence in current API
                "language": getattr(transcription, 'language', language),
                "duration": getattr(transcription, 'duration', 0.0)
            }

        except Exception as e:
            logger.error(f"❌ Groq transcription error: {e}")
            return {
                "text": "",
                "confidence": 0.0,
                "error": str(e)
            }

    def transcribe_audio_sync(
        self,
        audio_data: bytes,
        language: str = "hi",
        prompt: str = None,
        translate_to_english: bool = True
    ) -> dict:
        """
        Synchronous version of transcribe_audio.
        Use this in async contexts with run_in_executor if needed.

        Args:
            audio_data: Raw PCM audio (16kHz, mono, 16-bit)
            language: Language code (hi, en, or auto)
            prompt: Context prompt for better accuracy
            translate_to_english: If True, uses direct translation (better for code-switching)
        """
        try:
            # Convert PCM to WAV format
            wav_buffer = io.BytesIO()
            with wave.open(wav_buffer, 'wb') as wav_file:
                wav_file.setnchannels(1)
                wav_file.setsampwidth(2)
                wav_file.setframerate(16000)
                wav_file.writeframes(audio_data)

            wav_buffer.seek(0)

            # TRANSLATION MODE: Convert any language directly to English
            # This avoids Urdu script and provides clean English output
            if translate_to_english:
                logger.debug(f"🔄 Using Whisper TRANSLATION mode (direct to English)")

                # Use the translations endpoint instead of transcriptions
                # This translates Hindi/Urdu/any language directly to English
                translation = self.client.audio.translations.create(
                    file=("audio.wav", wav_buffer.read()),
                    model="whisper-large-v3",
                    response_format="verbose_json",
                    temperature=0.0
                    # No language param - auto-detect source, output is always English
                )

                text = translation.text.strip()
                detected_lang = getattr(translation, 'language', 'auto')

                logger.info(
                    "✅ Translated to English (chars=%s, source_language=%s)",
                    len(text),
                    detected_lang,
                )

                return {
                    "text": text,
                    "confidence": 1.0,
                    "language": "en",  # Output is always English
                    "translated": True,
                    "source_language": detected_lang
                }

            # TRANSCRIPTION-ONLY MODE: If translation disabled
            else:
                wav_buffer.seek(0)
                transcription = self.client.audio.transcriptions.create(
                    file=("audio.wav", wav_buffer.read()),
                    model="whisper-large-v3",
                    language=None,  # Auto-detect
                    prompt=prompt or "This is a business meeting.",
                    response_format="verbose_json",
                    temperature=0.0
                )

                text = transcription.text.strip()
                detected_language = getattr(transcription, 'language', 'unknown')

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
                    "original_text": None
                }

        except RateLimitError as e:
            logger.error(f"❌ Groq Rate Limit Reached: {e}")
            return {
                "text": "",
                "confidence": 0.0,
                "error": "rate_limit_exceeded"
            }
        except Exception as e:
            logger.error(f"❌ Groq transcription error: {e}")
            return {
                "text": "",
                "confidence": 0.0,
                "error": str(e)
            }
    async def transcribe_full_audio(
        self,
        audio_data: bytes,
        language: str = "en",
        prompt: str = None
    ) -> dict:
        """
        Transcribe a large audio file and return detailed segments.
        Used for post-meeting 'Gold Standard' recovery.
        """
        try:
            # If input is already a container audio file (wav/mp3/ogg/mp4/m4a),
            # send as-is. Otherwise treat bytes as raw PCM and wrap into WAV.
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
                wav_buffer = io.BytesIO()
                with wave.open(wav_buffer, 'wb') as wav_file:
                    wav_file.setnchannels(1)
                    wav_file.setsampwidth(2)
                    wav_file.setframerate(16000)
                    wav_file.writeframes(audio_data)
                wav_buffer.seek(0)
                upload_bytes = wav_buffer.read()
                filename = "audio.wav"
            elif audio_data.startswith(b"OggS"):
                filename = "audio.ogg"
            elif audio_data.startswith(b"ID3") or audio_data.startswith(b"\xff\xfb"):
                filename = "audio.mp3"
            elif audio_data[4:8] == b"ftyp":
                filename = "audio.m4a"

            # Groq has upload size limits. Guard early for large container uploads.
            # For WAV we can chunk locally and stitch transcript segments with offsets.
            max_upload_bytes = int(
                os.getenv("GROQ_MAX_UPLOAD_BYTES", str(24 * 1024 * 1024))
            )
            if len(upload_bytes) > max_upload_bytes:
                if looks_like_container and filename != "audio.wav":
                    raise ValueError(
                        f"groq_request_too_large_container: size={len(upload_bytes)} bytes exceeds "
                        f"GROQ_MAX_UPLOAD_BYTES={max_upload_bytes}. Use smaller audio, or provide WAV/PCM for chunked fallback."
                    )
                if filename == "audio.wav":
                    return self._transcribe_large_wav_in_chunks(
                        wav_bytes=upload_bytes,
                        max_upload_bytes=max_upload_bytes,
                        prompt=prompt,
                    )

            # Use translation/transcription based on requirements
            # For gold-standard, we prioritize English output for consistency
            result = self.client.audio.translations.create(
                file=(filename, upload_bytes),
                model="whisper-large-v3",
                response_format="verbose_json",
                temperature=0.0,
                prompt=prompt or "This is a business meeting transcript."
            )

            # Extract segments for precise alignment
            segments = []
            if hasattr(result, 'segments'):
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
                        {
                            "text": seg_text,
                            "start": seg_start,
                            "end": seg_end,
                            "confidence": seg_conf,  # Using logprob as confidence proxy
                        }
                    )
            
            return {
                "text": result.text.strip(),
                "segments": segments,
                "language": "en",
                "duration": getattr(result, 'duration', 0.0)
            }

        except Exception as e:
            logger.error(f"❌ Groq full transcription error: {e}")
            return {"text": "", "segments": [], "error": str(e)}

    def _transcribe_large_wav_in_chunks(
        self,
        wav_bytes: bytes,
        max_upload_bytes: int,
        prompt: str = None,
    ) -> Dict:
        """
        Chunk a large WAV into <= max_upload_bytes pieces and stitch transcript segments.
        """
        with wave.open(io.BytesIO(wav_bytes), "rb") as wav_reader:
            nchannels = wav_reader.getnchannels()
            sampwidth = wav_reader.getsampwidth()
            framerate = wav_reader.getframerate()
            total_frames = wav_reader.getnframes()
            pcm_frames = wav_reader.readframes(total_frames)

        bytes_per_frame = max(1, nchannels * sampwidth)
        # Reserve space for WAV headers and request overhead.
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

            chunk_io = io.BytesIO()
            with wave.open(chunk_io, "wb") as wav_writer:
                wav_writer.setnchannels(nchannels)
                wav_writer.setsampwidth(sampwidth)
                wav_writer.setframerate(framerate)
                wav_writer.writeframes(chunk_pcm)

            chunk_bytes = chunk_io.getvalue()
            chunk_offset_sec = chunk_start_frame / float(framerate)
            logger.info(
                "📦 Groq chunked transcription: chunk=%s size=%s offset=%.2fs",
                chunk_index,
                len(chunk_bytes),
                chunk_offset_sec,
            )

            result = self.client.audio.translations.create(
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
                        {
                            "text": seg_text,
                            "start": seg_start,
                            "end": seg_end,
                            "confidence": seg_conf,
                        }
                    )

            chunk_start_frame = chunk_end_frame
            chunk_index += 1

        return {
            "text": " ".join(texts).strip(),
            "segments": segments,
            "language": "en",
            "duration": duration_seconds,
        }
