import os
import logging
import asyncio
import subprocess
import uuid
from pathlib import Path
from datetime import datetime
from typing import Optional, Dict, List

import aiofiles

try:
    from ..db import DatabaseManager
    from .audio.groq_client import GroqTranscriptionClient
    from .audio.elevenlabs_client import ElevenLabsTranscriptionClient
    from .audio.diarization import get_diarization_service
except (ImportError, ValueError):
    from db import DatabaseManager
    from services.audio.groq_client import GroqTranscriptionClient
    from services.audio.elevenlabs_client import ElevenLabsTranscriptionClient
    from services.audio.diarization import get_diarization_service

logger = logging.getLogger(__name__)

# Configure storage paths
UPLOAD_DIR = Path("./data/uploads")
RECORDING_DIR = Path("./data/recordings")


class FileProcessor:
    """
    Handles processing of uploaded audio/video files.
    - Converts to standard WAV format
    - Transcribes using Groq Whisper
    - Runs Speaker Diarization
    - Generates AI Summary
    """

    def __init__(self, db_manager: DatabaseManager):
        self.db = db_manager
        self.groq_client = GroqTranscriptionClient()
        self.elevenlabs_client: Optional[ElevenLabsTranscriptionClient] = None
        self.diarization_service = get_diarization_service()

        # Ensure directories exist
        UPLOAD_DIR.mkdir(parents=True, exist_ok=True)
        RECORDING_DIR.mkdir(parents=True, exist_ok=True)

    async def process_file(
        self, meeting_id: str, file_path: Path, title: str, file_ext: str = ""
    ):
        """
        Background task to process an uploaded file.
        file_path: Local temporary path where the file is currently stored.
                   It is expected to be a temp file and will be cleaned up after processing.
        """
        try:
            logger.info(
                f"🚀 Starting processing for meeting {meeting_id} (File: {file_path})"
            )

            # Update status to processing (Need to implement in DB or assume implicit)

            # 1. Convert to standardized PCM (16kHz, Mono, s16le)
            # This generates a local temp file
            pcm_path = await self._convert_to_pcm(file_path, meeting_id)

            # 2. Upload PCM to Storage (GCP/Local)
            if pcm_path:
                try:
                    # Upload "merged_recording.pcm"
                    from .storage import StorageService  # Import inside to avoid circle

                    await StorageService.upload_file(
                        str(pcm_path), f"{meeting_id}/merged_recording.pcm"
                    )
                except Exception as e:
                    logger.error(f"Failed to upload PCM to storage: {e}")

            if not pcm_path:
                logger.error(f"❌ Audio conversion failed for {meeting_id}")
                return

            # 2.5 Build compressed canonical artifact for imported meetings
            # Keep WAV for diarization compatibility, but prefer compressed for retrieval/use.
            compressed_path = await self._create_compressed_from_pcm(pcm_path, meeting_id)
            if compressed_path and compressed_path.exists():
                try:
                    from .storage import StorageService

                    await StorageService.upload_file(
                        str(compressed_path), f"{meeting_id}/recording.opus"
                    )
                    await StorageService.upload_file(
                        str(compressed_path), f"{meeting_id}/merged_recording.opus"
                    )
                    logger.info(f"✅ Uploaded compressed recording for {meeting_id}")
                except Exception as e:
                    logger.error(f"Failed to upload compressed recording: {e}")

            # 2. Transcribe
            logger.info(f"📝 Transcribing {meeting_id}...")
            # Read the raw PCM data
            async with aiofiles.open(pcm_path, "rb") as f:
                pcm_data = await f.read()

            transcription_provider = (
                os.getenv("TRANSCRIPTION_PROVIDER", "groq").lower().strip()
            )
            model_name = "whisper-large-v3"
            model_family = "groq-whisper"
            if transcription_provider == "elevenlabs":
                if self.elevenlabs_client is None:
                    self.elevenlabs_client = ElevenLabsTranscriptionClient(mode="batch")
                transcription_result = await self.elevenlabs_client.transcribe_full_audio(
                    pcm_data
                )
                model_name = "scribe_v2"
                model_family = "elevenlabs-scribe"
            else:
                transcription_result = await self.groq_client.transcribe_full_audio(
                    pcm_data
                )

            if "error" in transcription_result:
                logger.error(
                    f"❌ Transcription failed: {transcription_result['error']}"
                )
                return

            segments = transcription_result.get("segments", [])
            full_text = transcription_result.get("text", "")

            logger.info(f"✅ Transcription complete: {len(segments)} segments")

            # 3. Save Initial Transcript
            db_segments = []
            for i, seg in enumerate(segments):
                db_segments.append(
                    {
                        "id": str(uuid.uuid4()),
                        "meeting_id": meeting_id,
                        "transcript": seg[
                            "text"
                        ],  # Changed from 'text' to match DB schema usually
                        "timestamp": datetime.now().isoformat(),  # Placeholder
                        "audio_start_time": seg["start"],
                        "audio_end_time": seg["end"],
                        "duration": seg["end"] - seg["start"],
                        "speaker": str(seg.get("speaker") or "Speaker 0"),
                        "speaker_confidence": 1.0,
                        "source": "upload",
                        "alignment_state": "confident",
                    }
                )

            # Save segments to main transcript table first?
            # Usually we save to transcript_segments table.
            # We can iterate and save.
            for seg in db_segments:
                await self.db.save_meeting_transcript(
                    meeting_id=meeting_id,
                    transcript=seg["transcript"],
                    timestamp=seg["timestamp"],
                    audio_start_time=seg["audio_start_time"],
                    audio_end_time=seg["audio_end_time"],
                    duration=seg["duration"],
                    source="upload",
                )

            # Save full transcript text as well
            if full_text:
                await self.db.save_transcript(
                    meeting_id=meeting_id,
                    transcript_text=full_text,
                    model=model_family,
                    model_name=model_name,
                    chunk_size=0,
                    overlap=0,
                )

            # 4. Diarization (Optional but recommended)
            if self.diarization_service.enabled:
                logger.info(f"👥 Starting diarization for {meeting_id}...")

                # Diarization service usually expects a WAV file for upload to providers
                # We need to create a temporary WAV from our PCM for the provider
                # Or reuse the original file if supported?
                # Deepgram supports WAV. Let's create a WAV for it.
                wav_path = await self._create_wav_from_pcm(pcm_path, meeting_id)

                # Upload WAV to storage as well
                if wav_path:
                    try:
                        from .storage import StorageService

                        await StorageService.upload_file(
                            str(wav_path), f"{meeting_id}/recording.wav"
                        )
                        await StorageService.upload_file(
                            str(wav_path), f"{meeting_id}/merged_recording.wav"
                        )
                    except Exception as e:
                        logger.error(f"Failed to upload WAV to storage: {e}")

                # The service expects a path where "merged_recording.wav" might exist or we pass data
                # Actually diarize_meeting merges chunks. We should bypass that and call _diarize_with_provider directly?
                # No, diarize_meeting does helpful logic.
                # But it looks for chunks in storage_path/meeting_id.
                # We should put our WAV file there as "merged_recording.wav".

                diarization_result = await self.diarization_service.diarize_meeting(
                    meeting_id=meeting_id,
                    storage_path=str(RECORDING_DIR),
                    provider="deepgram",
                )

                if diarization_result.status == "completed":
                    # Align and update transcripts
                    # Note: align_with_transcripts expects input transcripts to have 'text' key usually
                    # db_segments has 'transcript' key. Let's map it.
                    input_segments = [
                        {"text": s["transcript"], **s} for s in db_segments
                    ]

                    (
                        aligned_segments,
                        metrics,
                    ) = await self.diarization_service.align_with_transcripts(
                        meeting_id, diarization_result, input_segments
                    )

                    # Save VERSION 1 (Diarized)
                    await self.db.save_transcript_version(
                        meeting_id=meeting_id,
                        source="diarization",
                        content=aligned_segments,
                        is_authoritative=True,
                        created_by="system",
                        alignment_config=metrics,
                    )

                    logger.info(f"✅ Diarization saved for {meeting_id}")
                else:
                    logger.warning(
                        f"⚠️ Diarization failed or skipped: {diarization_result.error}"
                    )

                # Cleanup WAV path
                if wav_path and wav_path.exists():
                    try:
                        os.unlink(wav_path)
                    except:
                        pass

            # 5. Generate Summary
            logger.info(f"🧠 Generating summary for {meeting_id}...")
            # Trigger summary generation logic here if needed

            logger.info(f"🎉 Processing complete for {meeting_id}")

        except Exception as e:
            logger.error(
                f"❌ Fatal error processing file for {meeting_id}: {e}", exc_info=True
            )
        finally:
            # Cleanup source file (it was a temp file passed from upload)
            try:
                if file_path.exists():
                    os.unlink(file_path)
                    logger.debug(f"Cleaned up temp file: {file_path}")
            except Exception as e:
                logger.warning(f"Failed to cleanup temp file {file_path}: {e}")

            # Cleanup PCM path
            try:
                # pcm_path is defined in local scope if success
                if "pcm_path" in locals() and pcm_path and pcm_path.exists():
                    os.unlink(pcm_path)
            except Exception as e:
                pass

            # Cleanup compressed temp path
            try:
                if (
                    "compressed_path" in locals()
                    and compressed_path
                    and compressed_path.exists()
                ):
                    os.unlink(compressed_path)
            except Exception:
                pass

            # Remove the recording dir if empty (since we uploaded everything)
            try:
                rec_dir = RECORDING_DIR / meeting_id
                if rec_dir.exists() and not any(rec_dir.iterdir()):
                    rec_dir.rmdir()
            except:
                pass

    async def _convert_to_pcm(
        self, input_path: Path, meeting_id: str
    ) -> Optional[Path]:
        """Convert any audio/video to 16kHz mono PCM (s16le) using ffmpeg."""
        output_dir = RECORDING_DIR / meeting_id
        output_dir.mkdir(parents=True, exist_ok=True)
        output_path = output_dir / "merged_recording.pcm"  # Store as PCM

        cmd = [
            "ffmpeg",
            "-y",
            "-i",
            str(input_path),
            "-ar",
            "16000",
            "-ac",
            "1",
            "-f",
            "s16le",
            "-c:a",
            "pcm_s16le",
            str(output_path),
        ]

        try:
            process = await asyncio.create_subprocess_exec(
                *cmd, stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE
            )
            stdout, stderr = await process.communicate()

            if process.returncode != 0:
                logger.error(f"FFmpeg conversion failed: {stderr.decode()}")
                return None

            logger.info(f"✅ Converted {input_path} to {output_path}")
            return output_path
        except Exception as e:
            logger.error(f"FFmpeg execution error: {e}")
            return None

    async def _create_wav_from_pcm(
        self, pcm_path: Path, meeting_id: str
    ) -> Optional[Path]:
        """Wrap PCM in WAV container for Diarization service."""
        output_dir = RECORDING_DIR / meeting_id
        wav_path = output_dir / "merged_recording.wav"

        cmd = [
            "ffmpeg",
            "-y",
            "-f",
            "s16le",
            "-ar",
            "16000",
            "-ac",
            "1",
            "-i",
            str(pcm_path),
            str(wav_path),
        ]

        try:
            process = await asyncio.create_subprocess_exec(
                *cmd, stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE
            )
            await process.communicate()
            return wav_path
        except Exception:
            return None

    async def _create_compressed_from_pcm(
        self, pcm_path: Path, meeting_id: str
    ) -> Optional[Path]:
        """
        Convert PCM to a compressed Opus artifact for faster retrieval and lower storage.
        """
        output_dir = RECORDING_DIR / meeting_id
        opus_path = output_dir / "recording.opus"

        cmd = [
            "ffmpeg",
            "-y",
            "-f",
            "s16le",
            "-ar",
            "16000",
            "-ac",
            "1",
            "-i",
            str(pcm_path),
            "-c:a",
            "libopus",
            "-b:a",
            "24k",
            str(opus_path),
        ]

        try:
            process = await asyncio.create_subprocess_exec(
                *cmd, stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE
            )
            _, stderr = await process.communicate()
            if process.returncode != 0:
                logger.error(f"FFmpeg Opus conversion failed: {stderr.decode()}")
                return None
            return opus_path
        except Exception as e:
            logger.error(f"FFmpeg Opus conversion error: {e}")
            return None


# Singleton instance
_file_processor = None


def get_file_processor(db_manager: DatabaseManager) -> FileProcessor:
    global _file_processor
    if not _file_processor:
        _file_processor = FileProcessor(db_manager)
    return _file_processor
