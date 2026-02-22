"""
Groq API client for streaming Whisper transcription.
Supports Hindi + English with low latency.
"""

from groq import Groq, RateLimitError
import os
import logging
import io
import wave

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
            # Convert PCM to WAV format
            wav_buffer = io.BytesIO()
            with wave.open(wav_buffer, 'wb') as wav_file:
                wav_file.setnchannels(1)
                wav_file.setsampwidth(2)
                wav_file.setframerate(16000)
                wav_file.writeframes(audio_data)

            wav_buffer.seek(0)

            # Use translation/transcription based on requirements
            # For gold-standard, we prioritize English output for consistency
            result = self.client.audio.translations.create(
                file=("audio.wav", wav_buffer.read()),
                model="whisper-large-v3",
                response_format="verbose_json",
                temperature=0.0,
                prompt=prompt or "This is a business meeting transcript."
            )

            # Extract segments for precise alignment
            segments = []
            if hasattr(result, 'segments'):
                for s in result.segments:
                    segments.append({
                        "text": s.get("text", "").strip(),
                        "start": s.get("start", 0.0),
                        "end": s.get("end", 0.0),
                        "confidence": s.get("avg_logprob", 1.0) # Using logprob as confidence proxy
                    })
            
            return {
                "text": result.text.strip(),
                "segments": segments,
                "language": "en",
                "duration": getattr(result, 'duration', 0.0)
            }

        except Exception as e:
            logger.error(f"❌ Groq full transcription error: {e}")
            return {"text": "", "segments": [], "error": str(e)}
