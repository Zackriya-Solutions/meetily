"""
Post-Recording Service

Orchestrates post-meeting audio processing:
1. Merge PCM chunks into a single file
2. Convert to WAV format
3. Upload to GCP (if configured)
4. Clean up local PCM chunks
5. Optionally trigger diarization
"""

import asyncio
import logging
import os
import shutil
from pathlib import Path
from typing import Optional, Dict

try:
    from .recorder import AudioRecorder
    from ..storage import StorageService
except (ImportError, ValueError):
    from services.audio.recorder import AudioRecorder
    from services.storage import StorageService

logger = logging.getLogger(__name__)


class PostRecordingService:
    """
    Handles all post-recording processing tasks.

    This service is called after a meeting recording ends to:
    - Finalize and merge audio chunks
    - Upload to cloud storage (GCP)
    - Clean up local temporary files
    - Trigger downstream processing (diarization, summarization)
    """

    def __init__(self, storage_path: str = "./data/recordings"):
        self.storage_path = Path(storage_path)
        self.storage_type = os.getenv("STORAGE_TYPE", "local").lower()
        self.prefer_compressed_read = (
            os.getenv("AUDIO_PREFER_COMPRESSED_READ", "true").lower() == "true"
        )
        self.skip_wav_finalize_if_compressed = (
            os.getenv("AUDIO_SKIP_WAV_FINALIZE_IF_COMPRESSED", "true").lower() == "true"
        )
        self.delete_local_after_upload = (
            os.getenv("DELETE_LOCAL_AFTER_UPLOAD", "true").lower() == "true"
        )
        self.delete_pcm_after_merge = (
            os.getenv("DELETE_PCM_AFTER_MERGE", "true").lower() == "true"
        )
        self.chunk_prefix = os.getenv("AUDIO_CHUNK_PREFIX", "pcm_chunks")

    async def finalize_recording(
        self,
        meeting_id: str,
        trigger_diarization: bool = False,
        user_email: Optional[str] = None,
    ) -> Dict:
        """
        Complete post-recording processing pipeline.

        Args:
            meeting_id: The meeting ID to process
            trigger_diarization: Whether to auto-trigger diarization
            user_email: User email for API key lookup

        Returns:
            Dict with processing status and file paths
        """
        result = {
            "meeting_id": meeting_id,
            "status": "pending",
            "merged_locally": False,
            "uploaded_to_gcp": False,
            "local_cleaned": False,
            "gcp_path": None,
            "local_path": None,
            "error": None,
        }

        try:
            recording_dir = self.storage_path / meeting_id

            if self.prefer_compressed_read and self.skip_wav_finalize_if_compressed:
                compressed_path = await self._get_compressed_archive_path(meeting_id)
                # DANGER: Only use compressed path if we are absolutely sure no concurrent pcm chunks exist
                # If we have PCM chunks, we MUST merge them to ensure no data loss from session resumes.
                if compressed_path:
                    logger.info(f"💾 Found compressed archive: {compressed_path}")
                    
                    # Double check for PCM chunks
                    has_chunks = False
                    if self.storage_type == "gcp":
                        prefix = f"{meeting_id}/{self.chunk_prefix}/"
                        files = await StorageService.list_files(prefix)
                        has_chunks = any(f.endswith(".pcm") for f in files)
                    else:
                        local_dir = self.storage_path / meeting_id
                        has_chunks = local_dir.exists() and any(local_dir.glob("chunk_*.pcm"))

                    if not has_chunks:
                        if self.storage_type == "gcp" and self.delete_pcm_after_merge:
                            try:
                                await self._cleanup_gcp_chunks(meeting_id)
                                result["local_cleaned"] = True
                            except Exception as e:
                                logger.warning(
                                    f"Failed to delete PCM chunks in GCS for {meeting_id}: {e}"
                                )
                        result["status"] = "completed"
                        result["uploaded_to_gcp"] = self.storage_type == "gcp"
                        result["gcp_path"] = compressed_path
                        logger.info(
                            f"✅ Post-recording fast path for {meeting_id}: {compressed_path} already available"
                        )
                        if trigger_diarization:
                            asyncio.create_task(
                                self._trigger_diarization(meeting_id, user_email)
                            )
                        return result
                    else:
                        logger.info(f"📦 PCM chunks detected for {meeting_id}, ignoring compressed archive to ensure full merge.")

            if self.storage_type == "gcp":
                logger.info(f"☁️ GCP mode: merging PCM in backend for {meeting_id}")
                merged = await self._merge_gcp_chunks_to_wav(meeting_id)
                if not merged:
                    result["status"] = "merge_failed"
                    result["error"] = "Failed to merge PCM chunks in GCP"
                    return result

                result["uploaded_to_gcp"] = True
                result["gcp_path"] = f"{meeting_id}/recording.wav"

                if self.delete_pcm_after_merge:
                    try:
                        await self._cleanup_gcp_chunks(meeting_id)
                        result["local_cleaned"] = True
                    except Exception as e:
                        logger.warning(f"Failed to delete PCM chunks in GCS: {e}")

                result["status"] = "completed"
                logger.info(f"✅ Post-recording (GCP) complete for {meeting_id}")

                if trigger_diarization:
                    asyncio.create_task(self._trigger_diarization(meeting_id, user_email))

                return result

            # Local mode: Check if recording directory exists
            if not recording_dir.exists():
                result["status"] = "no_recording"
                result["error"] = f"No recording found for meeting {meeting_id}"
                logger.warning(f"No recording directory found: {recording_dir}")
                return result

            # Step 1: Merge PCM chunks
            logger.info(f"📼 Step 1: Merging PCM chunks for meeting {meeting_id}")
            merged_pcm = await self._merge_chunks(meeting_id)

            if not merged_pcm:
                # RECOVERY ATTEMPT: Check if we have chunks but merge failed silently
                logger.warning(
                    f"Merge returned None, attempting manual chunk scan for {meeting_id}"
                )
                chunk_dir = self.storage_path / meeting_id
                if chunk_dir.exists() and list(chunk_dir.glob("chunk_*.pcm")):
                    logger.info("Found orphan chunks, retrying merge...")
                    merged_pcm = await AudioRecorder.merge_chunks(
                        meeting_id, str(self.storage_path)
                    )

            if not merged_pcm:
                # Final check: Maybe it was already merged and converted?
                wav_path = self.storage_path / meeting_id / "recording.wav"
                if wav_path.exists():
                    logger.info("Found existing recording.wav, using that.")
                    result["merged_locally"] = True
                    result["local_path"] = str(wav_path)
                    # Jump to GCP upload
                else:
                    result["status"] = "merge_failed"
                    result["error"] = (
                        "Failed to merge audio chunks and no existing WAV found"
                    )
                    return result

            # Step 2: Convert to WAV (if we have new PCM)
            if merged_pcm:
                logger.info(f"🎵 Step 2: Converting to WAV format")
                wav_path = await self._convert_to_wav(meeting_id, merged_pcm)

                if not wav_path:
                    result["status"] = "conversion_failed"
                    result["error"] = "Failed to convert to WAV"
                    return result

                result["merged_locally"] = True
                result["local_path"] = str(wav_path)

            # Ensure we have a path before proceeding
            if not result.get("local_path"):
                result["status"] = "error"
                result["error"] = "Lost audio file path reference"
                return result

            wav_path = Path(result["local_path"])

            # Step 3: Upload to GCP (if configured)
            if self.storage_type == "gcp":
                logger.info(f"☁️ Step 3: Uploading to GCP")
                gcp_path = await self._upload_to_gcp(meeting_id, wav_path)

                if gcp_path:
                    result["uploaded_to_gcp"] = True
                    result["gcp_path"] = gcp_path

                    # Step 4: Clean up local files (if configured and upload succeeded)
                    if self.delete_local_after_upload:
                        logger.info(f"🗑️ Step 4: Cleaning up local files")
                        await self._cleanup_local(meeting_id, keep_wav=False)
                        result["local_cleaned"] = True
                else:
                    logger.warning(f"GCP upload failed, keeping local files")
            else:
                logger.info(f"📁 Step 3: Local storage mode - skipping GCP upload")

            result["status"] = "completed"
            logger.info(f"✅ Post-recording processing complete for {meeting_id}")

            # Step 5: Trigger diarization if requested
            if trigger_diarization:
                asyncio.create_task(self._trigger_diarization(meeting_id, user_email))

            return result

        except Exception as e:
            logger.error(f"Post-recording processing failed: {e}", exc_info=True)
            result["status"] = "error"
            result["error"] = str(e)
            return result

    async def _get_compressed_archive_path(self, meeting_id: str) -> Optional[str]:
        """
        Return compressed archive path if present.
        """
        candidates = [f"{meeting_id}/recording.opus", f"{meeting_id}/recording.m4a"]
        try:
            # If PCM chunks exist, force full merge path to ensure resumed sessions
            # are represented in the final recording artifact.
            if self.storage_type == "gcp":
                prefix = f"{meeting_id}/{self.chunk_prefix}/"
                files = await StorageService.list_files(prefix)
                if any(f.endswith(".pcm") for f in files):
                    return None
            else:
                local_dir = self.storage_path / meeting_id
                if local_dir.exists() and any(local_dir.glob("chunk_*.pcm")):
                    return None

            if self.storage_type == "gcp":
                for path in candidates:
                    if await StorageService.check_file_exists(path):
                        return path
                return None

            local_dir = self.storage_path / meeting_id
            for name, path in (
                ("recording.opus", f"{meeting_id}/recording.opus"),
                ("recording.m4a", f"{meeting_id}/recording.m4a"),
            ):
                if (local_dir / name).exists():
                    return path
            return None
        except Exception:
            return None

    async def _merge_gcp_chunks_to_wav(self, meeting_id: str) -> bool:
        """
        Merge PCM chunks stored in GCS into a WAV file, upload to GCS.
        No local disk usage; uses in-memory buffering.
        """
        try:
            try:
                from ..storage import StorageService
            except (ImportError, ValueError):
                from services.storage import StorageService

            prefix = f"{meeting_id}/{self.chunk_prefix}/"
            files = await StorageService.list_files(prefix)
            chunk_files = sorted([f for f in files if f.endswith(".pcm")])

            if not chunk_files:
                logger.error(f"No PCM chunks found in GCS for {meeting_id}")
                return False

            logger.info(f"📂 Merging {len(chunk_files)} PCM chunks from GCP for {meeting_id}")
            for i, f in enumerate(chunk_files[:5]):
                logger.debug(f"  [{i}] {f}")
            if len(chunk_files) > 5:
                logger.debug(f"  ... and {len(chunk_files) - 5} more")

            import io
            import wave

            wav_buffer = io.BytesIO()
            with wave.open(wav_buffer, "wb") as wav_file:
                wav_file.setnchannels(1)
                wav_file.setsampwidth(2)
                wav_file.setframerate(16000)

                # Check for existing recording.wav to append to
                existing_wav_path = f"{meeting_id}/recording.wav"
                existing_wav_bytes = await StorageService.download_bytes(existing_wav_path)
                if existing_wav_bytes and len(existing_wav_bytes) > 44:
                    logger.info(f"Found existing GCP recording.wav for {meeting_id}, appending new PCM chunks.")
                    wav_file.writeframes(existing_wav_bytes[44:])

                for blob_name in chunk_files:
                    chunk = await StorageService.download_bytes(blob_name)
                    if chunk:
                        wav_file.writeframes(chunk)

            wav_buffer.seek(0)
            wav_bytes = wav_buffer.read()

            uploaded = await StorageService.upload_bytes(
                wav_bytes, f"{meeting_id}/recording.wav", content_type="audio/wav"
            )
            if not uploaded:
                logger.error("Failed to upload merged WAV to GCS")
                return False

            logger.info(
                f"✅ Uploaded merged WAV for {meeting_id} ({len(wav_bytes) / 1024 / 1024:.2f} MB)"
            )
            return True
        except Exception as e:
            logger.error(f"Merge PCM in backend failed: {e}", exc_info=True)
            return False

    async def _cleanup_gcp_chunks(self, meeting_id: str) -> bool:
        try:
            try:
                from ..storage import StorageService
            except (ImportError, ValueError):
                from services.storage import StorageService

            prefix = f"{meeting_id}/{self.chunk_prefix}/"
            return await StorageService.delete_prefix(prefix)
        except Exception as e:
            logger.error(f"GCS cleanup failed: {e}")
            return False

    async def _merge_chunks(self, meeting_id: str) -> Optional[bytes]:
        """Merge all PCM chunks for a meeting."""
        try:
            pcm_data = await AudioRecorder.merge_chunks(
                meeting_id, str(self.storage_path)
            )
            return pcm_data
        except Exception as e:
            logger.error(f"Failed to merge chunks: {e}")
            return None

    async def _convert_to_wav(self, meeting_id: str, pcm_data: bytes) -> Optional[Path]:
        """Convert PCM to WAV and append to existing WAV locally if present."""
        try:
            wav_path = self.storage_path / meeting_id / "recording.wav"
            import aiofiles
            
            existing_pcm = b""
            if wav_path.exists():
                logger.info(f"Found existing recording.wav for {meeting_id}, appending new PCM chunks.")
                async with aiofiles.open(wav_path, "rb") as f:
                    old_wav = await f.read()
                    if len(old_wav) > 44:
                        existing_pcm = old_wav[44:]
            
            total_pcm = existing_pcm + pcm_data
            wav_data = AudioRecorder.convert_pcm_to_wav(total_pcm)

            async with aiofiles.open(wav_path, "wb") as f:
                await f.write(wav_data)

            logger.info(
                f"WAV file saved: {wav_path} ({len(wav_data) / 1024 / 1024:.2f} MB)"
            )
            return wav_path

        except Exception as e:
            logger.error(f"Failed to convert to WAV: {e}")
            return None

    async def _upload_to_gcp(
        self, meeting_id: str, local_wav_path: Path
    ) -> Optional[str]:
        """Upload WAV file to GCP bucket."""
        try:
            gcp_path = f"{meeting_id}/recording.wav"

            success = await StorageService.upload_file(str(local_wav_path), gcp_path)

            if success:
                logger.info(f"✅ Uploaded to GCP: {gcp_path}")
                return gcp_path
            else:
                logger.error(f"GCP upload returned False")
                return None

        except Exception as e:
            logger.error(f"GCP upload failed: {e}")
            return None

    async def _cleanup_local(self, meeting_id: str, keep_wav: bool = True) -> bool:
        """
        Clean up local PCM chunks after successful GCP upload.

        Args:
            meeting_id: Meeting ID
            keep_wav: If True, keep the merged WAV file locally
        """
        try:
            recording_dir = self.storage_path / meeting_id

            if not recording_dir.exists():
                return True

            # Delete PCM chunks
            for pcm_file in recording_dir.glob("chunk_*.pcm"):
                pcm_file.unlink()
                logger.debug(f"Deleted: {pcm_file}")

            # Delete merged PCM if it exists
            merged_pcm = recording_dir / "merged_recording.pcm"
            if merged_pcm.exists():
                merged_pcm.unlink()

            # Optionally delete WAV
            if not keep_wav:
                wav_file = recording_dir / "recording.wav"
                if wav_file.exists():
                    wav_file.unlink()
                    logger.debug(f"Deleted WAV: {wav_file}")

                # Also try to delete merged_recording.wav
                merged_wav = recording_dir / "merged_recording.wav"
                if merged_wav.exists():
                    merged_wav.unlink()

            # Clean up empty directory
            remaining_files = list(recording_dir.iterdir())
            if not remaining_files:
                recording_dir.rmdir()
                logger.info(f"Removed empty recording directory: {recording_dir}")

            logger.info(f"Local cleanup complete for {meeting_id}")
            return True

        except Exception as e:
            logger.error(f"Local cleanup failed: {e}")
            return False

    async def _trigger_diarization(
        self, meeting_id: str, user_email: Optional[str] = None
    ):
        """Trigger background diarization job."""
        try:
            # Import here to avoid circular imports
            from ..audio.diarization import get_diarization_service

            service = get_diarization_service()
            logger.info(f"🎯 Auto-triggering diarization for {meeting_id}")

            # This would need proper integration with the diarization job system
            # For now, just log the intent
            # await service.diarize_meeting(meeting_id)

        except Exception as e:
            logger.error(f"Failed to trigger diarization: {e}")


# Singleton instance
_post_recording_service: Optional[PostRecordingService] = None


def get_post_recording_service() -> PostRecordingService:
    """Get or create the post-recording service singleton."""
    global _post_recording_service

    if _post_recording_service is None:
        storage_path = os.getenv("RECORDINGS_STORAGE_PATH", "./data/recordings")
        _post_recording_service = PostRecordingService(storage_path)

    return _post_recording_service
