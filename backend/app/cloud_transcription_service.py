"""
Cloud transcription provider abstraction.
Supports Deepgram (primary) and OpenAI Whisper API (fallback).
"""

import os
import logging
from abc import ABC, abstractmethod
from typing import Optional
from dataclasses import dataclass

logger = logging.getLogger(__name__)


@dataclass
class TranscriptSegment:
    text: str
    start: float
    end: float
    confidence: float


@dataclass
class TranscriptResult:
    text: str
    segments: list  # List[TranscriptSegment]
    duration_seconds: float
    language: str


class TranscriptionProvider(ABC):
    """Base class for cloud transcription providers."""

    @abstractmethod
    async def transcribe(
        self,
        audio_data: bytes,
        audio_format: str = "m4a",
        language: Optional[str] = None,
    ) -> TranscriptResult:
        pass


class DeepgramProvider(TranscriptionProvider):
    """Deepgram transcription provider — primary choice for accuracy + streaming."""

    def __init__(self):
        self.api_key = os.getenv("DEEPGRAM_API_KEY", "")

    async def transcribe(
        self,
        audio_data: bytes,
        audio_format: str = "m4a",
        language: Optional[str] = None,
    ) -> TranscriptResult:
        if not self.api_key:
            raise ValueError("DEEPGRAM_API_KEY not configured")

        try:
            from deepgram import DeepgramClient, PrerecordedOptions

            client = DeepgramClient(self.api_key)
            options = PrerecordedOptions(
                model="nova-2",
                language=language or "en",
                smart_format=True,
                punctuate=True,
                paragraphs=True,
                utterances=True,
                diarize=True,
            )

            source = {"buffer": audio_data, "mimetype": f"audio/{audio_format}"}
            response = await client.listen.asyncrest.v("1").transcribe_file(source, options)

            result = response.results
            transcript_text = result.channels[0].alternatives[0].transcript
            duration = float(response.metadata.duration or 0)

            segments = []
            if result.utterances:
                for utt in result.utterances:
                    segments.append({
                        "text": utt.transcript,
                        "start": float(utt.start),
                        "end": float(utt.end),
                        "confidence": float(utt.confidence),
                    })
            elif result.channels[0].alternatives[0].words:
                # Build segments from words if no utterances
                words = result.channels[0].alternatives[0].words
                current_segment = {"text": "", "start": 0.0, "end": 0.0, "confidence": 0.0}
                word_count = 0
                for word in words:
                    if word_count == 0:
                        current_segment["start"] = float(word.start)
                    current_segment["text"] += word.punctuated_word + " "
                    current_segment["end"] = float(word.end)
                    current_segment["confidence"] += float(word.confidence)
                    word_count += 1

                    if word_count >= 20 or word.punctuated_word.endswith((".", "?", "!")):
                        current_segment["text"] = current_segment["text"].strip()
                        current_segment["confidence"] /= word_count
                        segments.append(current_segment.copy())
                        current_segment = {"text": "", "start": 0.0, "end": 0.0, "confidence": 0.0}
                        word_count = 0

                if word_count > 0:
                    current_segment["text"] = current_segment["text"].strip()
                    current_segment["confidence"] /= word_count
                    segments.append(current_segment)

            return TranscriptResult(
                text=transcript_text,
                segments=segments,
                duration_seconds=duration,
                language=language or "en",
            )

        except ImportError:
            raise ValueError("deepgram-sdk not installed. Run: pip install deepgram-sdk")
        except Exception as e:
            logger.error(f"Deepgram transcription failed: {e}", exc_info=True)
            raise


class OpenAIWhisperProvider(TranscriptionProvider):
    """OpenAI Whisper API — fallback transcription provider."""

    def __init__(self):
        self.api_key = os.getenv("OPENAI_API_KEY", "")

    async def transcribe(
        self,
        audio_data: bytes,
        audio_format: str = "m4a",
        language: Optional[str] = None,
    ) -> TranscriptResult:
        if not self.api_key:
            raise ValueError("OPENAI_API_KEY not configured")

        try:
            import httpx

            headers = {"Authorization": f"Bearer {self.api_key}"}
            files = {"file": (f"audio.{audio_format}", audio_data, f"audio/{audio_format}")}
            data = {
                "model": "whisper-1",
                "response_format": "verbose_json",
                "timestamp_granularities[]": "segment",
            }
            if language:
                data["language"] = language

            async with httpx.AsyncClient(timeout=300) as client:
                resp = await client.post(
                    "https://api.openai.com/v1/audio/transcriptions",
                    headers=headers,
                    files=files,
                    data=data,
                )
                resp.raise_for_status()
                result = resp.json()

            segments = []
            for seg in result.get("segments", []):
                segments.append({
                    "text": seg.get("text", "").strip(),
                    "start": float(seg.get("start", 0)),
                    "end": float(seg.get("end", 0)),
                    "confidence": float(seg.get("avg_logprob", -1)),
                })

            return TranscriptResult(
                text=result.get("text", ""),
                segments=segments,
                duration_seconds=float(result.get("duration", 0)),
                language=result.get("language", language or "en"),
            )

        except Exception as e:
            logger.error(f"OpenAI Whisper transcription failed: {e}", exc_info=True)
            raise


def get_transcription_provider(provider_name: str = "deepgram") -> TranscriptionProvider:
    """Factory function to get a transcription provider."""
    providers = {
        "deepgram": DeepgramProvider,
        "openai_whisper": OpenAIWhisperProvider,
    }
    cls = providers.get(provider_name)
    if not cls:
        raise ValueError(f"Unknown transcription provider: {provider_name}")
    return cls()
