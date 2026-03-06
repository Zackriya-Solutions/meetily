"""
Speaker Diarization Service.

This module handles post-meeting speaker diarization using cloud APIs
(Deepgram or AssemblyAI). It processes recorded audio to identify
"who spoke when" and aligns the results with existing transcripts.

Features:
- Cloud API integration (Deepgram Nova-2, AssemblyAI)
- Audio chunk merging and conversion
- Transcript-speaker alignment
- Speaker segment generation
"""

import asyncio
import httpx
import logging
import os
import json
import aiofiles
import io
import re
import wave
import shutil
from pathlib import Path
from typing import Optional, Dict, List, Tuple
from datetime import datetime
from dataclasses import dataclass

try:
    from .recorder import AudioRecorder
    from .groq_client import GroqTranscriptionClient
    from .alignment import AlignmentEngine
except (ImportError, ValueError):
    from services.audio.recorder import AudioRecorder
    from services.audio.groq_client import GroqTranscriptionClient
    from services.audio.alignment import AlignmentEngine

logger = logging.getLogger(__name__)


@dataclass
class SpeakerSegment:
    """Represents a speaker segment with timing and text."""

    speaker: str
    start_time: float
    end_time: float
    text: str
    confidence: float = 1.0
    word_count: int = 0


@dataclass
class DiarizationResult:
    """Result of diarization processing."""

    status: str  # 'completed', 'failed', 'pending'
    meeting_id: str
    speaker_count: int
    segments: List[SpeakerSegment]
    processing_time_seconds: float
    provider: str
    error: Optional[str] = None


class DiarizationService:
    """
    Service for speaker diarization using cloud APIs.

    Supported providers:
    - Deepgram (Nova-2): Fast, good accuracy, $0.25/hour
    - AssemblyAI: Best in noisy conditions, $0.37/hour
    """

    def __init__(self, provider: str = "deepgram", groq_api_key: str = None):
        """
        Initialize the diarization service.

        Args:
            provider: 'deepgram' or 'assemblyai'
        """
        self.provider = provider.lower()

        # Load API keys from environment
        self.deepgram_api_key = os.getenv("DEEPGRAM_API_KEY")
        self.assemblyai_api_key = os.getenv("ASSEMBLYAI_API_KEY")

        # Groq client for high-fidelity transcription
        self.groq_api_key = groq_api_key or os.getenv("GROQ_API_KEY")
        self.groq = (
            GroqTranscriptionClient(self.groq_api_key) if self.groq_api_key else None
        )

        # API endpoints
        self.deepgram_url = "https://api.deepgram.com/v1/listen"
        self.assemblyai_url = "https://api.assemblyai.com/v2"

        # Feature flag
        self.enabled = os.getenv("ENABLE_DIARIZATION", "true").lower() == "true"

        # Alignment engine (3-tier logic)
        self.alignment_engine = AlignmentEngine()
        self.deepgram_parallel_enabled = (
            os.getenv("DEEPGRAM_PARALLEL_DIARIZATION_ENABLED", "true").lower() == "true"
        )
        self.deepgram_parallel_chunk_minutes = float(
            os.getenv("DEEPGRAM_PARALLEL_CHUNK_MINUTES", "10")
        )
        self.deepgram_parallel_overlap_seconds = float(
            os.getenv("DEEPGRAM_PARALLEL_OVERLAP_SECONDS", "30")
        )
        self.deepgram_parallel_concurrency = int(
            os.getenv("DEEPGRAM_PARALLEL_CONCURRENCY", "8")
        )
        self.deepgram_parallel_similarity_threshold = float(
            os.getenv("DEEPGRAM_PARALLEL_SPEAKER_SIMILARITY_THRESHOLD", "0.08")
        )

        logger.info(
            f"DiarizationService initialized (provider={provider}, enabled={self.enabled})"
        )

    @staticmethod
    def _text_similarity(text_a: str, text_b: str) -> float:
        """
        Calculate text similarity using Levenshtein ratio.
        Fallbacks to Jaccard token similarity if Levenshtein is unavailable.
        """
        if not text_a or not text_b:
            return 0.0

        try:
            from Levenshtein import ratio

            return ratio(text_a.lower().strip(), text_b.lower().strip())
        except ImportError:
            # Fallback to Jaccard
            def _tokenize(t):
                cleaned = re.sub(r"[^a-zA-Z0-9\s]", " ", (t or "").lower())
                return {w for w in cleaned.split() if len(w) > 2}

            tokens_a = _tokenize(text_a)
            tokens_b = _tokenize(text_b)
            if not tokens_a or not tokens_b:
                return 0.0
            union = len(tokens_a | tokens_b)
            return len(tokens_a & tokens_b) / union if union > 0 else 0.0

    @staticmethod
    def _build_overlap_text(
        segment_dicts: List[Dict],
        overlap_start: float,
        overlap_end: float,
        speaker: str,
    ) -> str:
        parts = []
        for seg in segment_dicts:
            if seg["speaker"] != speaker:
                continue
            if seg["end_time"] <= overlap_start or seg["start_time"] >= overlap_end:
                continue
            parts.append(seg.get("text", ""))
        return " ".join(parts).strip()

    def _split_wav_for_parallel(
        self, wav_data: bytes, chunk_minutes: float, overlap_seconds: float
    ) -> List[Tuple[int, float, float, bytes]]:
        """
        Split WAV bytes into overlapping WAV chunks for parallel diarization.
        Returns tuples: (chunk_index, start_sec, end_sec, chunk_wav_bytes).
        """
        with wave.open(io.BytesIO(wav_data), "rb") as wav_reader:
            nchannels = wav_reader.getnchannels()
            sampwidth = wav_reader.getsampwidth()
            framerate = wav_reader.getframerate()
            # Trust the actual data bytes, not the header nframes (often placeholder 0x7FFFFFFF from pipe)
            pcm_frames = wav_reader.readframes(2**31 - 1)

        total_bytes = len(pcm_frames)
        bytes_per_frame = nchannels * sampwidth
        total_frames = total_bytes // bytes_per_frame

        chunk_seconds = max(60.0, chunk_minutes * 60.0)
        step_seconds = max(1.0, chunk_seconds - max(0.0, overlap_seconds))
        total_seconds = total_frames / float(framerate) if framerate else 0.0

        logger.info(
            "📏 Parallel split: total_duration=%.2fs total_frames=%d bytes=%d chunk_sec=%.1f",
            total_seconds,
            total_frames,
            total_bytes,
            chunk_seconds,
        )

        chunks: List[Tuple[int, float, float, bytes]] = []
        chunk_index = 0
        start_sec = 0.0
        while start_sec < total_seconds:
            end_sec = min(total_seconds, start_sec + chunk_seconds)
            start_frame = int(start_sec * framerate)
            end_frame = int(end_sec * framerate)
            if end_frame <= start_frame:
                break

            start_byte = start_frame * bytes_per_frame
            end_byte = end_frame * bytes_per_frame
            chunk_pcm = pcm_frames[start_byte:end_byte]

            chunk_io = io.BytesIO()
            with wave.open(chunk_io, "wb") as chunk_writer:
                chunk_writer.setnchannels(nchannels)
                chunk_writer.setsampwidth(sampwidth)
                chunk_writer.setframerate(framerate)
                chunk_writer.writeframes(chunk_pcm)

            chunks.append((chunk_index, start_sec, end_sec, chunk_io.getvalue()))
            chunk_index += 1

            # Avoid infinite loop if step is 0 or end reached
            if end_sec >= total_seconds:
                break
            start_sec += step_seconds

        return chunks

    @staticmethod
    def _looks_like_audio_container(audio_data: bytes) -> bool:
        if not audio_data or len(audio_data) < 12:
            return False
        return (
            audio_data.startswith(b"RIFF")
            or audio_data.startswith(b"OggS")
            or audio_data.startswith(b"ID3")
            or audio_data.startswith(b"\xff\xfb")
            or audio_data[4:8] == b"ftyp"
        )

    async def _decode_container_to_wav_with_ffmpeg(
        self, audio_data: bytes, meeting_id: str
    ) -> bytes:
        if not shutil.which("ffmpeg"):
            raise RuntimeError(
                "ffmpeg not found in PATH; cannot decode compressed audio container"
            )

        cmd = [
            "ffmpeg",
            "-hide_banner",
            "-loglevel",
            "error",
            "-i",
            "pipe:0",
            "-ac",
            "1",
            "-ar",
            "16000",
            "-f",
            "wav",
            "pipe:1",
        ]
        process = await asyncio.create_subprocess_exec(
            *cmd,
            stdin=asyncio.subprocess.PIPE,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )
        stdout, stderr = await process.communicate(input=audio_data)
        if process.returncode != 0:
            err_text = (stderr or b"").decode("utf-8", errors="ignore")[:400]
            raise RuntimeError(f"ffmpeg decode failed: {err_text}")
        if not stdout or not stdout.startswith(b"RIFF"):
            raise RuntimeError("ffmpeg decode produced invalid WAV output")

        logger.info(
            "📦 Decoded compressed container to WAV for %s (%0.2f MB -> %0.2f MB)",
            meeting_id,
            len(audio_data) / (1024 * 1024),
            len(stdout) / (1024 * 1024),
        )
        return stdout

    async def ensure_wav_audio(self, audio_data: bytes, meeting_id: str) -> bytes:
        """
        Normalize arbitrary audio bytes into 16k mono WAV for stable diarization.
        - WAV container: pass through
        - Other known containers (opus/ogg/mp3/m4a): decode via ffmpeg
        - Raw PCM fallback: wrap as WAV
        """
        if not audio_data:
            raise ValueError("No audio bytes provided for WAV normalization")

        if audio_data.startswith(b"RIFF"):
            logger.info("📦 Audio is already WAV format")
            return audio_data

        if self._looks_like_audio_container(audio_data):
            return await self._decode_container_to_wav_with_ffmpeg(
                audio_data, meeting_id
            )

        logger.info("📦 Treating audio as raw PCM and converting to WAV")
        return AudioRecorder.convert_pcm_to_wav(audio_data)

    def _stitch_parallel_segments(
        self,
        chunk_results: List[Tuple[int, float, float, List[SpeakerSegment]]],
    ) -> List[Dict]:
        stitched: List[Dict] = []
        for chunk_index, chunk_start, _, segments in sorted(
            chunk_results, key=lambda x: x[0]
        ):
            for seg in segments:
                stitched.append(
                    {
                        "speaker": seg.speaker,
                        "local_speaker": seg.speaker,
                        "chunk_index": chunk_index,
                        "start_time": seg.start_time + chunk_start,
                        "end_time": seg.end_time + chunk_start,
                        "text": seg.text,
                        "confidence": seg.confidence,
                        "word_count": seg.word_count,
                    }
                )

        stitched.sort(key=lambda s: s["start_time"])
        deduped: List[Dict] = []
        last_end = -1.0
        for seg in stitched:
            start = seg["start_time"]
            end = seg["end_time"]
            if end <= last_end:
                continue
            if start < last_end:
                seg["start_time"] = last_end
            if seg["end_time"] - seg["start_time"] < 0.05:
                continue
            deduped.append(seg)
            last_end = seg["end_time"]

        return deduped

    def _reconcile_parallel_chunk_speakers(
        self,
        stitched_segments: List[Dict],
        overlap_seconds: float,
        similarity_threshold: float,
    ) -> List[Dict]:
        by_chunk: Dict[int, List[Dict]] = {}
        for seg in stitched_segments:
            by_chunk.setdefault(seg["chunk_index"], []).append(seg)

        chunk_indices = sorted(by_chunk.keys())
        if not chunk_indices:
            return stitched_segments

        chunk_local_to_global: Dict[Tuple[int, str], str] = {}
        next_global_speaker_id = 0

        first_chunk = chunk_indices[0]
        for local_speaker in sorted(
            {s["local_speaker"] for s in by_chunk.get(first_chunk, [])}
        ):
            chunk_local_to_global[(first_chunk, local_speaker)] = (
                f"Speaker {next_global_speaker_id}"
            )
            next_global_speaker_id += 1

        for index in range(1, len(chunk_indices)):
            current_chunk = chunk_indices[index]
            previous_chunk = chunk_indices[index - 1]
            current_segments = by_chunk.get(current_chunk, [])
            previous_segments = by_chunk.get(previous_chunk, [])

            if not current_segments:
                continue

            current_chunk_start = min(s["start_time"] for s in current_segments)
            overlap_start = max(0.0, current_chunk_start - overlap_seconds)
            overlap_end = current_chunk_start + overlap_seconds
            used_globals = set()

            for current_local in sorted({s["local_speaker"] for s in current_segments}):
                current_text = self._build_overlap_text(
                    current_segments, overlap_start, overlap_end, current_local
                )

                best_global = None
                best_score = 0.0
                for previous_local in sorted(
                    {s["local_speaker"] for s in previous_segments}
                ):
                    previous_global = chunk_local_to_global.get(
                        (previous_chunk, previous_local)
                    )
                    if not previous_global or previous_global in used_globals:
                        continue

                    previous_text = self._build_overlap_text(
                        previous_segments, overlap_start, overlap_end, previous_local
                    )
                    score = self._text_similarity(current_text, previous_text)
                    if score > best_score:
                        best_score = score
                        best_global = previous_global

                if best_global and best_score >= similarity_threshold:
                    global_speaker = best_global
                else:
                    global_speaker = f"Speaker {next_global_speaker_id}"
                    next_global_speaker_id += 1

                used_globals.add(global_speaker)
                chunk_local_to_global[(current_chunk, current_local)] = global_speaker

        reconciled: List[Dict] = []
        for seg in stitched_segments:
            global_speaker = chunk_local_to_global.get(
                (seg["chunk_index"], seg["local_speaker"]), seg["speaker"]
            )
            reconciled.append({**seg, "speaker": global_speaker})
        return reconciled

    async def _diarize_with_deepgram_parallel(
        self,
        wav_data: bytes,
        meeting_id: str,
        api_key: str,
    ) -> List[SpeakerSegment]:
        chunks = self._split_wav_for_parallel(
            wav_data=wav_data,
            chunk_minutes=self.deepgram_parallel_chunk_minutes,
            overlap_seconds=self.deepgram_parallel_overlap_seconds,
        )
        if len(chunks) <= 1:
            return await self._diarize_with_deepgram(
                audio_data=wav_data,
                meeting_id=meeting_id,
                api_key=api_key,
            )

        logger.info(
            "🚀 Parallel diarization: chunks=%s chunk_minutes=%.1f overlap=%.1fs concurrency=%s",
            len(chunks),
            self.deepgram_parallel_chunk_minutes,
            self.deepgram_parallel_overlap_seconds,
            self.deepgram_parallel_concurrency,
        )

        semaphore = asyncio.Semaphore(max(1, self.deepgram_parallel_concurrency))

        async def run_chunk(chunk: Tuple[int, float, float, bytes]):
            chunk_index, start_sec, end_sec, chunk_wav = chunk
            async with semaphore:
                segments = await self._diarize_with_deepgram(
                    audio_data=chunk_wav,
                    meeting_id=f"{meeting_id}-chunk-{chunk_index}",
                    api_key=api_key,
                )
                return chunk_index, start_sec, end_sec, segments

        chunk_results = await asyncio.gather(*(run_chunk(chunk) for chunk in chunks))
        stitched = self._stitch_parallel_segments(chunk_results)
        reconciled = self._reconcile_parallel_chunk_speakers(
            stitched_segments=stitched,
            overlap_seconds=self.deepgram_parallel_overlap_seconds,
            similarity_threshold=self.deepgram_parallel_similarity_threshold,
        )

        return [
            SpeakerSegment(
                speaker=seg["speaker"],
                start_time=seg["start_time"],
                end_time=seg["end_time"],
                text=seg.get("text", ""),
                confidence=seg.get("confidence", 1.0),
                word_count=seg.get("word_count", 0),
            )
            for seg in reconciled
        ]

    async def _get_groq_api_key(self, user_email: str = None) -> Optional[str]:
        """
        Resolve Groq API key for diarization transcription.
        Priority: 1) Environment, 2) User-specific key from DB, 3) cached instance key.
        """
        env_key = os.getenv("GROQ_API_KEY")
        if env_key:
            return env_key

        if user_email:
            try:
                try:
                    from ...db import DatabaseManager
                except (ImportError, ValueError):
                    from db import DatabaseManager

                db = DatabaseManager()
                db_key = await db.get_api_key("groq", user_email=user_email)
                if db_key:
                    return db_key
            except Exception as e:
                logger.warning(f"Failed to get Groq key from database: {e}")

        return self.groq_api_key

    async def transcribe_with_whisper(
        self, audio_data: bytes, user_email: str = None
    ) -> List[Dict]:
        """
        Run high-fidelity Whisper transcription on the full meeting audio.
        Returns segments for alignment.
        """
        resolved_groq_key = await self._get_groq_api_key(user_email)
        if not resolved_groq_key:
            logger.error(
                "No Groq API key available for high-fidelity transcription (user=%s)",
                user_email,
            )
            return []

        if self.groq is None or self.groq_api_key != resolved_groq_key:
            self.groq_api_key = resolved_groq_key
            self.groq = GroqTranscriptionClient(self.groq_api_key)

        logger.info("💎 Running Gold Standard Whisper transcription...")
        result = await self.groq.transcribe_full_audio(
            audio_data,
            prompt="This is a business meeting in Hindi and English. Speakers may code-switch.",
        )

        if result.get("error"):
            logger.error(f"Gold transcription failed: {result['error']}")
            return []

        logger.info(
            f"✅ Gold transcription complete: {len(result.get('segments', []))} segments"
        )
        return result.get("segments", [])

    async def _get_api_key(
        self, provider: str = None, user_email: str = None
    ) -> Optional[str]:
        """
        Get API key for the specified provider.
        Priority: 1) User-specific key from DB, 2) Environment variable
        """
        provider = provider or self.provider

        # Try environment variable first (fastest)
        env_key = None
        if provider == "deepgram":
            env_key = os.getenv("DEEPGRAM_API_KEY")
        elif provider == "assemblyai":
            env_key = os.getenv("ASSEMBLYAI_API_KEY")

        if env_key:
            return env_key

        # Fall back to database lookup if user_email provided
        if user_email:
            try:
                try:
                    from ...db import DatabaseManager
                except (ImportError, ValueError):
                    from db import DatabaseManager

                db = DatabaseManager()
                db_key = await db.get_api_key(provider, user_email=user_email)
                if db_key:
                    return db_key
            except Exception as e:
                logger.warning(f"Failed to get API key from database: {e}")

        # Return cached instance variable as final fallback
        if provider == "deepgram":
            return self.deepgram_api_key
        elif provider == "assemblyai":
            return self.assemblyai_api_key

        logger.error(f"No API key found for provider: {provider}")
        return None

    async def diarize_meeting(
        self,
        meeting_id: str,
        storage_path: str = "./data/recordings",
        provider: str = None,
        audio_data: bytes = None,
        user_email: str = None,
    ) -> DiarizationResult:
        """
        Run speaker diarization on a meeting's recorded audio.

        This is the main entry point for diarization. It:
        1. Merges audio chunks (or uses provided audio_data)
        2. Sends to cloud API
        3. Processes results
        4. Returns speaker segments

        Args:
            meeting_id: Meeting ID to diarize
            storage_path: Base path for recordings
            provider: Override default provider
            audio_data: Optional pre-loaded audio bytes (PCM or WAV)
            user_email: Optional user email for fetching API keys

        Returns:
            DiarizationResult with speaker segments
        """
        start_time = datetime.utcnow()
        provider = provider or self.provider

        if not self.enabled:
            return DiarizationResult(
                status="disabled",
                meeting_id=meeting_id,
                speaker_count=0,
                segments=[],
                processing_time_seconds=0,
                provider=provider,
                error="Diarization is disabled",
            )

        api_key = await self._get_api_key(provider, user_email)
        if not api_key:
            return DiarizationResult(
                status="failed",
                meeting_id=meeting_id,
                speaker_count=0,
                segments=[],
                processing_time_seconds=0,
                provider=provider,
                error=f"No API key configured for {provider}. Set {provider.upper()}_API_KEY environment variable.",
            )

        try:
            logger.info(
                f"🎯 Starting diarization for meeting {meeting_id} with {provider}"
            )

            # Step 1: Get Audio Data
            if audio_data is None:
                # Try to find existing merged files first (Imported)
                recording_dir = Path(storage_path) / meeting_id
                merged_pcm = recording_dir / "merged_recording.pcm"
                merged_wav = recording_dir / "merged_recording.wav"

                if merged_pcm.exists():
                    logger.info(f"📂 Found existing merged PCM file for {meeting_id}")
                    async with aiofiles.open(merged_pcm, "rb") as f:
                        audio_data = await f.read()
                elif merged_wav.exists():
                    logger.info(f"📂 Found existing merged WAV file for {meeting_id}")
                    async with aiofiles.open(merged_wav, "rb") as f:
                        audio_data = await f.read()
                else:
                    # Fallback to merging chunks
                    logger.info(
                        f"⚠️ Merged audio missing for {meeting_id}, attempting to merge chunks locally..."
                    )
                    audio_data = await AudioRecorder.merge_chunks(
                        meeting_id, storage_path
                    )

            # If still no audio, check for any chunks and try harder
            if not audio_data:
                recording_dir = Path(storage_path) / meeting_id
                if recording_dir.exists():
                    chunks = list(recording_dir.glob("chunk_*.pcm"))
                    if chunks:
                        logger.info(
                            f"⚠️ explicit chunk merge triggered for {len(chunks)} chunks"
                        )
                        audio_data = await AudioRecorder.merge_chunks(
                            meeting_id, storage_path
                        )

            if not audio_data:
                return DiarizationResult(
                    status="failed",
                    meeting_id=meeting_id,
                    speaker_count=0,
                    segments=[],
                    processing_time_seconds=0,
                    provider=provider,
                    error="No audio data found for this meeting. Ensure recording was enabled.",
                )

            # Step 2: Normalize audio to WAV (bytes path only)
            wav_data = None
            if audio_data:
                wav_data = await self.ensure_wav_audio(audio_data, meeting_id)
                logger.info(f"📦 Audio prepared: {len(wav_data)} bytes")

            # Step 3: Send to diarization API
            if provider == "deepgram":
                # Prefer sending decoded WAV bytes directly over a signed URL.
                # The URL often points to a compressed container (m4a/opus) which
                # can cause Deepgram to timeout, whereas we already have decoded WAV.
                if self.deepgram_parallel_enabled and wav_data:
                    try:
                        segments = await self._diarize_with_deepgram_parallel(
                            wav_data=wav_data,
                            meeting_id=meeting_id,
                            api_key=api_key,
                        )
                    except Exception as parallel_error:
                        logger.warning(
                            "Parallel Deepgram diarization failed for %s, falling back to single pass: %s",
                            meeting_id,
                            parallel_error,
                        )
                        segments = await self._diarize_with_deepgram(
                            wav_data, meeting_id, api_key
                        )
                else:
                    segments = await self._diarize_with_deepgram(
                        wav_data, meeting_id, api_key
                    )
            elif provider == "assemblyai":
                segments = await self._diarize_with_assemblyai(
                    wav_data, meeting_id, api_key, audio_url=audio_url
                )
            else:
                raise ValueError(f"Unknown provider: {provider}")

            # Calculate processing time
            processing_time = (datetime.utcnow() - start_time).total_seconds()

            # Count unique speakers
            unique_speakers = set(seg.speaker for seg in segments)

            logger.info(
                f"✅ Diarization complete: {len(segments)} segments, "
                f"{len(unique_speakers)} speakers, "
                f"{processing_time:.1f}s processing time"
            )

            return DiarizationResult(
                status="completed",
                meeting_id=meeting_id,
                speaker_count=len(unique_speakers),
                segments=segments,
                processing_time_seconds=processing_time,
                provider=provider,
            )

        except Exception as e:
            logger.error(f"Diarization failed for meeting {meeting_id}: {e}")
            return DiarizationResult(
                status="failed",
                meeting_id=meeting_id,
                speaker_count=0,
                segments=[],
                processing_time_seconds=(
                    datetime.utcnow() - start_time
                ).total_seconds(),
                provider=provider,
                error=str(e),
            )

    async def _diarize_with_deepgram(
        self,
        audio_data: bytes,
        meeting_id: str,
        api_key: str,
    ) -> List[SpeakerSegment]:
        """
        Send audio bytes to Deepgram for diarization.

        Uses Deepgram Nova-2 model with diarization enabled.

        Args:
            audio_data: WAV/MP3/OGG audio bytes
            meeting_id: For logging
            api_key: Deepgram API key

        Returns:
            List of SpeakerSegment objects
        """
        max_retries = 3
        retry_delay = 1
        last_error = None

        # Determine content type based on header
        content_type = "audio/wav"
        if audio_data.startswith(b"ID3") or audio_data.startswith(b"\xff\xfb"):
            content_type = "audio/mp3"
        elif audio_data.startswith(b"OggS"):
            content_type = "audio/ogg"

        for attempt in range(max_retries):
            try:
                # Use standard client
                async with httpx.AsyncClient(timeout=300.0) as client:
                    response = await client.post(
                        self.deepgram_url,
                        headers={
                            "Authorization": f"Token {api_key}",
                            "Content-Type": content_type,
                        },
                        params={
                            "model": "nova-2",
                            "diarize": "true",
                            "punctuate": "true",
                            "utterances": "true",
                            "smart_format": "false",
                        },
                        content=audio_data,
                    )

                    if response.status_code != 200:
                        error_text = response.text
                        logger.error(
                            f"Deepgram API error (Attempt {attempt + 1}/{max_retries}): {response.status_code} - {error_text}"
                        )
                        # If 4xx error (client error), do not retry
                        if 400 <= response.status_code < 500:
                            raise Exception(
                                f"Deepgram API error: {response.status_code}"
                            )

                        response.raise_for_status()

                    result = response.json()

                    # Parse response into segments
                    segments = []

                    # QUALITY TRANSCRIPTION: Prefer 'utterances' for natural punctuation,
                    # fallback to 'words' reconstruction for 100% completeness.
                    utterances = result.get("results", {}).get("utterances", [])

                    channels = result.get("results", {}).get("channels", [])
                    alternatives = (
                        channels[0].get("alternatives", []) if channels else []
                    )
                    words = alternatives[0].get("words", []) if alternatives else []

                    if not words and not utterances:
                        logger.warning(
                            f"No results returned by Deepgram for meeting {meeting_id}"
                        )
                        return []

                    raw_segments = []

                    if utterances:
                        # Use punctuated utterances
                        for u in utterances:
                            raw_segments.append(
                                SpeakerSegment(
                                    speaker=f"Speaker {u.get('speaker', 0)}",
                                    start_time=u.get("start", 0),
                                    end_time=u.get("end", 0),
                                    text=u.get("transcript", ""),
                                    confidence=u.get("confidence", 1.0),
                                    word_count=len(u.get("words", [])),
                                )
                            )
                    elif words:
                        # Fallback: Reconstruct from words (raw, but complete)
                        current_speaker = None
                        current_segment = None

                        for w in words:
                            speaker = f"Speaker {w.get('speaker', 0)}"
                            if speaker != current_speaker:
                                if current_segment:
                                    raw_segments.append(current_segment)
                                current_speaker = speaker
                                current_segment = SpeakerSegment(
                                    speaker=speaker,
                                    start_time=w.get("start", 0),
                                    end_time=w.get("end", 0),
                                    text=w.get("word", ""),
                                    confidence=w.get("speaker_confidence", 1.0),
                                    word_count=1,
                                )
                            else:
                                if current_segment:
                                    current_segment.end_time = w.get(
                                        "end", current_segment.end_time
                                    )
                                    current_segment.text += " " + w.get("word", "")
                                    current_segment.word_count += 1
                        if current_segment:
                            raw_segments.append(current_segment)

                    if not raw_segments:
                        logger.warning(f"No usable segments for {meeting_id}")
                        return []

                    # NATURAL GROUPING: Merge consecutive segments from same speaker
                    segments = []
                    current = raw_segments[0]
                    MAX_GAP = 5.0  # seconds

                    for next_seg in raw_segments[1:]:
                        gap = next_seg.start_time - current.end_time
                        if next_seg.speaker == current.speaker and gap < MAX_GAP:
                            # Merge
                            current.text += " " + next_seg.text
                            current.end_time = next_seg.end_time
                            current.word_count += next_seg.word_count
                        else:
                            segments.append(current)
                            current = next_seg

                    segments.append(current)
                    logger.info(
                        f"Reconstructed {len(segments)} natural segments for {meeting_id}"
                    )
                    return segments

            except httpx.NetworkError as e:
                last_error = e
                logger.warning(
                    f"Deepgram network error (Attempt {attempt + 1}/{max_retries}): {e}"
                )
                if attempt < max_retries - 1:
                    await asyncio.sleep(retry_delay * (2**attempt))
            except Exception as e:
                # Non-network errors (or 4xx from above) - re-raise immediately
                raise e

        # If we get here, all retries failed
        raise Exception(
            f"Deepgram API failed after {max_retries} attempts: {last_error}"
        )

    async def _diarize_with_assemblyai(
        self,
        audio_data: Optional[bytes],
        meeting_id: str,
        api_key: str,
        audio_url: Optional[str] = None,
    ) -> List[SpeakerSegment]:
        """
        Send audio to AssemblyAI for diarization.

        Uses AssemblyAI's transcription with speaker_labels enabled.
        Note: AssemblyAI uses a two-step process (upload then transcribe).

        Args:
            audio_data: WAV audio bytes

        Returns:
            List of SpeakerSegment objects
        """
        async with httpx.AsyncClient(timeout=600.0) as client:
            if not audio_url:
                # Step 1: Upload audio file
                upload_response = await client.post(
                    f"{self.assemblyai_url}/upload",
                    headers={
                        "authorization": api_key,
                        "content-type": "application/octet-stream",
                    },
                    content=audio_data,
                )

                if upload_response.status_code != 200:
                    raise Exception(
                        f"AssemblyAI upload failed: {upload_response.status_code}"
                    )

                audio_url = upload_response.json().get("upload_url")
                logger.info(f"Audio uploaded to AssemblyAI")

            # Step 2: Request transcription with diarization
            transcript_response = await client.post(
                f"{self.assemblyai_url}/transcript",
                headers={
                    "authorization": api_key,
                    "content-type": "application/json",
                },
                json={
                    "audio_url": audio_url,
                    "speaker_labels": True,
                    "punctuate": True,
                    "format_text": True,
                },
            )

            if transcript_response.status_code != 200:
                raise Exception(
                    f"AssemblyAI transcription request failed: {transcript_response.status_code}"
                )

            transcript_id = transcript_response.json().get("id")
            logger.info(f"Transcription started: {transcript_id}")

            # Step 3: Poll for completion
            while True:
                status_response = await client.get(
                    f"{self.assemblyai_url}/transcript/{transcript_id}",
                    headers={"authorization": api_key},
                )

                status_data = status_response.json()
                status = status_data.get("status")

                if status == "completed":
                    break
                elif status == "error":
                    raise Exception(
                        f"AssemblyAI transcription failed: {status_data.get('error')}"
                    )

                logger.debug(f"AssemblyAI status: {status}")
                await asyncio.sleep(3)  # Poll every 3 seconds

            # Step 4: Parse results
            segments = []
            utterances = status_data.get("utterances", [])

            for utterance in utterances:
                segment = SpeakerSegment(
                    speaker=f"Speaker {utterance.get('speaker', 'A')}",
                    start_time=utterance.get("start", 0)
                    / 1000,  # AssemblyAI uses milliseconds
                    end_time=utterance.get("end", 0) / 1000,
                    text=utterance.get("text", ""),
                    confidence=utterance.get("confidence", 1.0),
                    word_count=len(utterance.get("words", [])),
                )
                segments.append(segment)

            logger.info(f"AssemblyAI returned {len(segments)} speaker segments")
            return segments

    def _clean_undefined(self, text: str) -> str:
        """Robustly remove 'undefined' artifacts from transcript text."""
        if not text:
            return ""

        # Remove 'undefined' prefix (case-insensitive, with or without space)
        import re

        text = re.sub(r"^(undefined\s*)+", "", text, flags=re.IGNORECASE)

        # Remove other common artifacts
        text = text.replace("undefined", "").replace("  ", " ").strip()

        return text

    async def align_with_transcripts(
        self,
        meeting_id: str,
        diarization_result: DiarizationResult,
        transcripts: List[Dict],
    ) -> Tuple[List[Dict], Dict]:
        """
        Align diarization results with transcript segments using 3-tier alignment.

        Returns:
            Tuple of (aligned_transcripts, metrics)
        """
        # CLEANUP: Pre-clean transcripts text to remove artifacts like 'undefined'
        for t in transcripts:
            if "text" in t:
                t["text"] = self._clean_undefined(t["text"])
            elif "transcript" in t:
                t["transcript"] = self._clean_undefined(t["transcript"])

        if diarization_result.status != "completed":
            logger.warning(
                f"Cannot align - diarization status: {diarization_result.status}"
            )
            return transcripts, {
                "error": f"Diarization status: {diarization_result.status}"
            }

        # Convert SpeakerSegment dataclasses to dicts for alignment engine
        speaker_segments = [
            {
                "speaker": seg.speaker,
                "start_time": seg.start_time,
                "end_time": seg.end_time,
                "text": seg.text,
                "confidence": seg.confidence,
            }
            for seg in diarization_result.segments
        ]

        if not speaker_segments:
            logger.warning("No diarization segments found to align with.")
            # Map everything to Unknown if no detection happened
            unknown_transcripts = [
                {
                    **t,
                    "speaker": "Unknown",
                    "text": t.get("text", t.get("transcript", "")),
                    "alignment_state": "UNKNOWN_SPEAKER",
                    "speaker_confidence": 0.0,
                }
                for t in transcripts
            ]
            return unknown_transcripts, {
                "total_segments": len(transcripts),
                "unknown_count": len(transcripts),
                "avg_confidence": 0.0,
            }

        # Use the new AlignmentEngine
        aligned_transcripts, metrics = self.alignment_engine.align_batch(
            transcripts, speaker_segments
        )

        # Assign UUIDs to segments if missing (crucial for React keys and streaming matching)
        import uuid

        for seg in aligned_transcripts:
            if "id" not in seg:
                seg["id"] = str(uuid.uuid4())

        logger.info(
            f"✅ Aligned {meeting_id}: {metrics['confident_count']}/{metrics['total_segments']} confident, "
            f"{metrics['uncertain_count']} uncertain, {metrics['overlap_count']} overlap, "
            f"avg_conf={metrics['avg_confidence']:.2f}"
        )

        return aligned_transcripts, metrics

    async def translate_aligned_transcript(
        self,
        meeting_id: str,
        aligned_transcripts: List[Dict],
        user_email: str,
    ) -> List[Dict]:
        """
        Translate the text of aligned transcript segments into English using an LLM.
        Preserves timestamps, speaker labels, and alignment confidence.
        """
        if not aligned_transcripts:
            return aligned_transcripts

        logger.info(
            f"🌐 Translating {len(aligned_transcripts)} aligned segments for {meeting_id}"
        )

        # 1. Prepare segments for the LLM
        # We need a format that is easy to parse back. JSON is best.
        import json

        payload_segments = []
        for index, seg in enumerate(aligned_transcripts):
            # Clean input text before sending to LLM to avoid garbage in/garbage out
            clean_input = self._clean_undefined(seg.get("text", ""))
            payload_segments.append({"index": index, "text": clean_input})

        prompt = f"""
        You are a professional meeting translator. Your task is to translate the following meeting transcript segments into clear, natural English.
        The input is a JSON array of segment objects. Each object has an `index` and `text`.
        
        Rules:
        1. Translate the `text` field of each segment into English.
        2. If a segment is already in English, simply return it as is or improve its grammatical clarity slightly.
        3. Do NOT merge or split segments. You must return exactly the same number of segments, with the exact same `index` values.
        4. Return ONLY a valid JSON array of objects, where each object has `index` and the new translated `text`.

        Input Segments:
        {json.dumps(payload_segments, ensure_ascii=False, indent=2)}
        """

        try:
            # Prioritize Environment Variables over Database (Consistency)
            api_key = os.getenv("GEMINI_API_KEY") or os.getenv("GOOGLE_API_KEY")
            if not api_key:
                logger.warning("GEMINI_API_KEY or GOOGLE_API_KEY not found, skipping translation")
                return aligned_transcripts

            try:
                from ..gemini_client import generate_content_text_async
            except (ImportError, ValueError):
                try:
                    from services.gemini_client import generate_content_text_async
                except ImportError:
                    from app.services.gemini_client import generate_content_text_async

            # Using Gemini as the default fast/cheap translation engine
            response_text = await generate_content_text_async(
                api_key=api_key,
                model="gemini-3-flash-preview",  # Reliable json output
                contents=prompt,
                config={"temperature": 0.1},
            )

            # Clean potential markdown wrapping
            if response_text.startswith("```json"):
                response_text = response_text[7:-3].strip()
            elif response_text.startswith("```"):
                response_text = response_text[3:-3].strip()

            translated_payload = json.loads(response_text)

            # Verify translation count matches
            if len(translated_payload) != len(aligned_transcripts):
                logger.error(
                    f"❌ Translation count mismatch: Expected {len(aligned_transcripts)}, got {len(translated_payload)}"
                )
                return aligned_transcripts  # Fallback to original

            # Map translations back to the original segments
            translated_dict = {
                item["index"]: item["text"] for item in translated_payload
            }

            translated_results = []
            for index, seg in enumerate(aligned_transcripts):
                new_seg = dict(seg)
                raw_text = seg.get("text", "")

                # Clean undefined before translation (redundant but safe)
                cleaned_text = self._clean_undefined(raw_text)

                new_seg["original_text"] = cleaned_text  # Keep clean original

                # Get translated text and Clean it too!
                translated_text = translated_dict.get(index, cleaned_text)
                new_seg["text"] = self._clean_undefined(translated_text)

                new_seg["translated"] = True
                translated_results.append(new_seg)

            logger.info(
                f"✅ Successfully translated {len(translated_results)} segments for {meeting_id}"
            )
            return translated_results

        except Exception as e:
            logger.error(f"❌ Post-alignment translation failed for {meeting_id}: {e}")
            return aligned_transcripts  # Fallback to original text on failure

    def format_transcript_with_speakers(self, transcripts: List[Dict]) -> str:
        """
        Format transcripts with speaker labels for LLM consumption.

        Args:
            transcripts: List of transcript dicts with 'speaker' and 'text' fields

        Returns:
            Formatted string with speaker labels
        """
        lines = []
        current_speaker = None

        for t in transcripts:
            speaker = t.get("speaker", "Unknown")
            text = t.get("text", "").strip()

            # CLEANUP: Remove 'undefined' artifacts
            text = self._clean_undefined(text)

            if not text:
                continue

            # Group consecutive segments from same speaker
            if speaker != current_speaker:
                lines.append(f"\n**{speaker}:** {text}")
                current_speaker = speaker
            else:
                lines.append(f" {text}")

        return "".join(lines).strip()


# Singleton instance
_diarization_service: Optional[DiarizationService] = None


def get_diarization_service() -> DiarizationService:
    """Get or create the diarization service singleton."""
    global _diarization_service

    if _diarization_service is None:
        provider = os.getenv("DIARIZATION_PROVIDER", "deepgram")
        _diarization_service = DiarizationService(provider)

    return _diarization_service
