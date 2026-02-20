"""
Audio Recorder Module for Parallel Audio Capture.

This module captures and stores audio chunks in parallel with the live transcription
pipeline. It is designed to be non-blocking and fault-tolerant.

Features:
- Parallel audio capture (doesn't affect transcription latency)
- Chunk-based storage for crash resilience
- Async file I/O for non-blocking operations
- Automatic directory management
"""

import asyncio
import aiofiles
import logging
import os
import time
import tempfile
import json
from pathlib import Path
from typing import Optional, Dict, List
from datetime import datetime
import struct
import uuid

logger = logging.getLogger(__name__)


class AudioRecorder:
    """
    Records audio chunks in parallel to live transcription.
    Non-blocking, async, fault-tolerant.

    Audio is stored as raw PCM (16kHz, mono, 16-bit) chunks that can be
    merged and processed later for speaker diarization.
    """

    def __init__(
        self,
        meeting_id: str,
        storage_path: str = "./data/recordings",
        chunk_duration_seconds: float = 30.0,
    ):
        """
        Initialize the audio recorder.

        Args:
            meeting_id: Unique identifier for the meeting
            storage_path: Base path for storing recordings
            chunk_duration_seconds: Duration of each audio chunk (default 30s)
        """
        self.meeting_id = meeting_id
        self.storage_path = Path(storage_path) / meeting_id
        self.storage_type = os.getenv("STORAGE_TYPE", "local").lower()
        self.chunk_prefix = os.getenv("AUDIO_CHUNK_PREFIX", "pcm_chunks")
        self.chunk_duration_seconds = chunk_duration_seconds
        self.parallel_encoding_enabled = (
            os.getenv("AUDIO_PARALLEL_ENCODING_ENABLED", "true").lower() == "true"
        )
        self.archive_format = os.getenv("AUDIO_ARCHIVE_FORMAT", "opus").lower()

        # Recording state
        self.is_recording = False
        self.chunk_index = 0
        self.recording_start_time: Optional[float] = None
        self.chunk_start_time: Optional[float] = None

        # Audio buffer for current chunk
        self.current_chunk_buffer = bytearray()

        # PCM audio parameters
        self.sample_rate = 16000  # 16kHz
        self.bytes_per_sample = 2  # 16-bit
        self.channels = 1  # Mono

        # Calculate target chunk size in bytes
        self.target_chunk_bytes = int(
            self.chunk_duration_seconds
            * self.sample_rate
            * self.bytes_per_sample
            * self.channels
        )

        # Chunk metadata for reconstruction
        self.chunks_metadata: List[Dict] = []

        # NEW: Lock to serialize background saves and prevent race conditions
        self._lock = asyncio.Lock()
        self.encoder_process: Optional[asyncio.subprocess.Process] = None
        self.encoder_output_path: Optional[Path] = None
        self.encoder_failed = False

        # Feature flag check
        self.enabled = os.getenv("ENABLE_AUDIO_RECORDING", "true").lower() == "true"

        logger.info(
            f"AudioRecorder initialized for meeting {meeting_id} (enabled={self.enabled})"
        )

    async def start(self) -> bool:
        """
        Initialize recording session.
        Creates storage directory and prepares for recording.

        Returns:
            bool: True if started successfully
        """
        if not self.enabled:
            logger.info(f"Audio recording disabled for meeting {self.meeting_id}")
            return False

        try:
            if self.storage_type == "gcp":
                try:
                    from ..storage import get_gcp_bucket
                except (ImportError, ValueError):
                    from services.storage import get_gcp_bucket

                bucket = get_gcp_bucket()
                if not bucket:
                    logger.error(
                        "GCS storage is enabled but bucket initialization failed. "
                        "Check STORAGE_TYPE, GCP_BUCKET_NAME, and credentials."
                    )
                    return False

            # Create storage directory for local mode only
            if self.storage_type != "gcp":
                self.storage_path.mkdir(parents=True, exist_ok=True)

            self.is_recording = True
            self.recording_start_time = time.time()
            self.chunk_start_time = self.recording_start_time
            self.chunk_index = await self._get_next_chunk_index()
            self.current_chunk_buffer = bytearray()
            self.chunks_metadata = []
            self.encoder_failed = False

            # Start parallel compressed archival encoder (does not impact PCM/STT path)
            if self.parallel_encoding_enabled:
                await self._start_parallel_encoder()

            logger.info(f"🎙️ Audio recording started for meeting {self.meeting_id}")
            logger.info(f"   Storage path: {self.storage_path}")
            logger.info(f"   Chunk duration: {self.chunk_duration_seconds}s")
            logger.info(f"   Next chunk index: {self.chunk_index}")

            return True

        except Exception as e:
            logger.error(f"Failed to start audio recording: {e}")
            self.is_recording = False
            return False

    async def _get_next_chunk_index(self) -> int:
        """
        Determine next chunk index by scanning existing chunks.
        Prevents overwrite when a meeting is resumed.
        """
        try:
            if self.storage_type == "gcp":
                try:
                    from ..storage import StorageService
                except (ImportError, ValueError):
                    from services.storage import StorageService

                prefix = f"{self.meeting_id}/{self.chunk_prefix}/"
                files: List[str] = []
                last_error: Optional[Exception] = None

                # GCS listing can fail transiently; retry before defaulting to 0.
                for _ in range(3):
                    try:
                        files = await StorageService.list_files(prefix)
                        last_error = None
                        break
                    except Exception as e:
                        last_error = e
                        await asyncio.sleep(0.2)

                if last_error:
                    raise last_error

                indices = []
                for path in files:
                    name = path.split("/")[-1]
                    if name.startswith("chunk_") and name.endswith(".pcm"):
                        try:
                            idx = int(name.replace("chunk_", "").replace(".pcm", ""))
                            indices.append(idx)
                        except ValueError:
                            continue

                if indices:
                    return max(indices) + 1

                # Fallback: derive last index from metadata when chunk listing is empty.
                # This protects resumed meetings from overwriting chunk_00000.
                metadata_path = f"{self.meeting_id}/{self.chunk_prefix}/metadata.json"
                metadata_bytes = await StorageService.download_bytes(metadata_path)
                if metadata_bytes:
                    try:
                        metadata = json.loads(metadata_bytes.decode("utf-8"))
                        chunks = metadata.get("chunks", []) or []
                        meta_indices = [
                            int(c.get("chunk_index"))
                            for c in chunks
                            if isinstance(c, dict) and c.get("chunk_index") is not None
                        ]
                        if meta_indices:
                            return max(meta_indices) + 1
                    except Exception:
                        pass

                return 0

            if not self.storage_path.exists():
                return 0

            indices = []
            for chunk_path in self.storage_path.glob("chunk_*.pcm"):
                name = chunk_path.name
                try:
                    idx = int(name.replace("chunk_", "").replace(".pcm", ""))
                    indices.append(idx)
                except ValueError:
                    continue

            return (max(indices) + 1) if indices else 0
        except Exception as e:
            logger.warning(
                f"Failed to determine next chunk index for {self.meeting_id}: {e}"
            )
            return 0

    async def add_chunk(self, audio_data: bytes) -> Optional[str]:
        """
        Add audio data to the recording buffer.
        When buffer reaches target size, saves to disk.
        """
        if not self.is_recording or not self.enabled:
            return None

        try:
            # Feed raw PCM into the long-running archive encoder.
            # This keeps post-save latency low because encoding is done during recording.
            if self.parallel_encoding_enabled:
                await self._feed_parallel_encoder(audio_data)

            # Synchronous extension: no await here ensures no race during addition
            self.current_chunk_buffer.extend(audio_data)

            # Check if we should save the chunk
            if len(self.current_chunk_buffer) >= self.target_chunk_bytes:
                # IMPORTANT: Swap buffer immediately to prevent data loss during 'await'
                data_to_save = bytes(self.current_chunk_buffer)
                self.current_chunk_buffer = bytearray()

                # Update chunk start time for calculations before background save
                current_time = time.time()
                old_chunk_start = self.chunk_start_time
                self.chunk_start_time = current_time

                return await self._actually_save_chunk(
                    data_to_save, old_chunk_start, current_time
                )

            return None

        except Exception as e:
            logger.error(f"Error adding audio chunk: {e}")
            return None

    async def _actually_save_chunk(
        self, data: bytes, chunk_start: float, chunk_end: float
    ) -> Optional[str]:
        """Internal method to perform the actual file I/O safely"""
        async with self._lock:  # NEW: Serialize all saves to disk
            try:
                chunk_filename = f"chunk_{self.chunk_index:05d}.pcm"
                chunk_rel_path = f"{self.meeting_id}/{self.chunk_prefix}/{chunk_filename}"

                # Calculate timing relative to meeting start
                start_offset = chunk_start - self.recording_start_time
                end_offset = chunk_end - self.recording_start_time
                duration = len(data) / (self.sample_rate * self.bytes_per_sample)

                # Save audio data (GCS or local)
                if self.storage_type == "gcp":
                    try:
                        from ..storage import StorageService
                    except (ImportError, ValueError):
                        from services.storage import StorageService

                    # Guard against overwrite if resume index recovery was wrong.
                    while await StorageService.check_file_exists(chunk_rel_path):
                        self.chunk_index += 1
                        chunk_filename = f"chunk_{self.chunk_index:05d}.pcm"
                        chunk_rel_path = (
                            f"{self.meeting_id}/{self.chunk_prefix}/{chunk_filename}"
                        )

                    success = await StorageService.upload_bytes(
                        data, chunk_rel_path, content_type="application/octet-stream"
                    )
                    if not success:
                        raise RuntimeError("Failed to upload chunk to GCS")
                else:
                    chunk_path = self.storage_path / chunk_filename
                    # Guard against overwrite in local mode as well.
                    while chunk_path.exists():
                        self.chunk_index += 1
                        chunk_filename = f"chunk_{self.chunk_index:05d}.pcm"
                        chunk_rel_path = (
                            f"{self.meeting_id}/{self.chunk_prefix}/{chunk_filename}"
                        )
                        chunk_path = self.storage_path / chunk_filename
                    async with aiofiles.open(chunk_path, "wb") as f:
                        await f.write(data)

                # Record metadata
                metadata = {
                    "chunk_index": self.chunk_index,
                    "filename": chunk_filename,
                    "storage_path": chunk_rel_path,
                    "start_time_seconds": start_offset,
                    "end_time_seconds": end_offset,
                    "duration_seconds": duration,
                    "size_bytes": len(data),
                    "created_at": datetime.utcnow().isoformat(),
                }
                self.chunks_metadata.append(metadata)

                logger.info(
                    f"💾 Saved audio chunk {self.chunk_index} ({duration:.1f}s)"
                )
                self.chunk_index += 1
                return chunk_rel_path

            except Exception as e:
                logger.error(f"Failed to save audio chunk: {e}")
                return None

    async def _save_current_chunk(self):
        """Standard wrapper for saving the remaining buffer at the end"""
        if not self.current_chunk_buffer:
            return None
        data = bytes(self.current_chunk_buffer)
        self.current_chunk_buffer = bytearray()
        return await self._actually_save_chunk(data, self.chunk_start_time, time.time())

    async def stop(self) -> Dict:
        """
        Finalize recording session.
        Saves any remaining audio and returns metadata.

        Returns:
            Dict containing recording metadata
        """
        if not self.is_recording:
            return {"status": "not_recording"}

        try:
            self.is_recording = False

            # Save any remaining audio in buffer
            if self.current_chunk_buffer:
                await self._save_current_chunk()

            compressed_uploaded = await self._finalize_parallel_encoder()

            recording_metadata = {
                "meeting_id": self.meeting_id,
                "recording_start": datetime.fromtimestamp(
                    self.recording_start_time
                ).isoformat()
                if self.recording_start_time
                else None,
                "recording_end": datetime.utcnow().isoformat(),
                "total_duration_seconds": time.time() - self.recording_start_time
                if self.recording_start_time
                else 0,
                "chunk_count": len(self.chunks_metadata),
                "storage_path": str(self.storage_path),
                "audio_format": {
                    "sample_rate": self.sample_rate,
                    "channels": self.channels,
                    "bits_per_sample": self.bytes_per_sample * 8,
                    "format": "PCM",
                },
                "chunks": self.chunks_metadata,
                "compressed_archive": {
                    "enabled": self.parallel_encoding_enabled,
                    "format": self.archive_format,
                    "local_path": str(self.encoder_output_path)
                    if self.encoder_output_path
                    else None,
                    "uploaded": compressed_uploaded,
                },
            }

            import json

            if self.storage_type == "gcp":
                try:
                    from ..storage import StorageService
                except (ImportError, ValueError):
                    from services.storage import StorageService

                metadata_path = f"{self.meeting_id}/{self.chunk_prefix}/metadata.json"
                await StorageService.upload_bytes(
                    json.dumps(recording_metadata, indent=2).encode("utf-8"),
                    metadata_path,
                    content_type="application/json",
                )
            else:
                metadata_path = self.storage_path / "metadata.json"
                async with aiofiles.open(metadata_path, "w") as f:
                    await f.write(json.dumps(recording_metadata, indent=2))

            logger.info(
                f"🎙️ Audio recording stopped for meeting {self.meeting_id}: "
                f"{len(self.chunks_metadata)} chunks, "
                f"{recording_metadata['total_duration_seconds']:.1f}s total"
            )

            return recording_metadata

        except Exception as e:
            logger.error(f"Error stopping audio recording: {e}")
            return {"status": "error", "error": str(e), "meeting_id": self.meeting_id}

    async def _start_parallel_encoder(self):
        """
        Start an ffmpeg process that continuously converts PCM stream -> compressed archive.
        """
        try:
            ext = "opus" if self.archive_format == "opus" else "m4a"
            if self.storage_type == "gcp":
                output_dir = Path(tempfile.gettempdir()) / "meeting-archives"
            else:
                output_dir = self.storage_path
            output_dir.mkdir(parents=True, exist_ok=True)

            self.encoder_output_path = output_dir / f"recording.{ext}"

            if ext == "opus":
                cmd = [
                    "ffmpeg",
                    "-hide_banner",
                    "-loglevel",
                    "error",
                    "-y",
                    "-f",
                    "s16le",
                    "-ar",
                    "16000",
                    "-ac",
                    "1",
                    "-i",
                    "pipe:0",
                    "-c:a",
                    "libopus",
                    "-b:a",
                    "24k",
                    str(self.encoder_output_path),
                ]
            else:
                cmd = [
                    "ffmpeg",
                    "-hide_banner",
                    "-loglevel",
                    "error",
                    "-y",
                    "-f",
                    "s16le",
                    "-ar",
                    "16000",
                    "-ac",
                    "1",
                    "-i",
                    "pipe:0",
                    "-c:a",
                    "aac",
                    "-b:a",
                    "48k",
                    str(self.encoder_output_path),
                ]

            self.encoder_process = await asyncio.create_subprocess_exec(
                *cmd,
                stdin=asyncio.subprocess.PIPE,
                stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.PIPE,
            )
            logger.info(
                f"🎧 Parallel archive encoder started for {self.meeting_id}: {self.encoder_output_path}"
            )
        except Exception as e:
            logger.warning(
                f"Failed to start parallel archive encoder for {self.meeting_id}: {e}"
            )
            self.encoder_failed = True
            self.encoder_process = None
            self.encoder_output_path = None

    async def _feed_parallel_encoder(self, audio_data: bytes):
        """
        Stream PCM bytes to the encoder process.
        """
        if self.encoder_failed or not self.encoder_process or not self.encoder_process.stdin:
            return
        try:
            self.encoder_process.stdin.write(audio_data)
            transport = getattr(self.encoder_process.stdin, "transport", None)
            if transport and transport.get_write_buffer_size() > 256_000:
                await self.encoder_process.stdin.drain()
        except Exception as e:
            logger.warning(
                f"Parallel archive encoder write failed for {self.meeting_id}: {e}"
            )
            self.encoder_failed = True

    async def _finalize_parallel_encoder(self) -> bool:
        """
        Close encoder input, wait for file finalization, and upload compressed artifact.
        """
        if not self.parallel_encoding_enabled:
            return False
        if self.encoder_failed or not self.encoder_process:
            return False

        uploaded = False
        try:
            if self.encoder_process.stdin:
                self.encoder_process.stdin.close()

            try:
                await asyncio.wait_for(self.encoder_process.wait(), timeout=60)
            except asyncio.TimeoutError:
                self.encoder_process.kill()
                await self.encoder_process.wait()
                logger.warning(f"Parallel encoder timeout for {self.meeting_id}")
                return False

            if self.encoder_process.returncode != 0:
                stderr = b""
                if self.encoder_process.stderr:
                    stderr = await self.encoder_process.stderr.read()
                logger.warning(
                    "Parallel encoder exited non-zero for %s: %s",
                    self.meeting_id,
                    stderr.decode("utf-8", errors="ignore")[:400],
                )
                return False

            if not self.encoder_output_path or not self.encoder_output_path.exists():
                return False

            try:
                from ..storage import StorageService
            except (ImportError, ValueError):
                from services.storage import StorageService

            ext = self.encoder_output_path.suffix.lower().lstrip(".")
            destination = f"{self.meeting_id}/recording.{ext}"
            uploaded = await StorageService.upload_file(
                str(self.encoder_output_path), destination
            )
            if uploaded:
                logger.info(
                    f"⬆️ Uploaded compressed archive for {self.meeting_id}: {destination}"
                )
        except Exception as e:
            logger.warning(
                f"Failed finalizing/uploading compressed archive for {self.meeting_id}: {e}"
            )
            uploaded = False
        finally:
            # Keep local file in local mode; remove temp file for GCP mode
            try:
                if (
                    self.storage_type == "gcp"
                    and self.encoder_output_path
                    and self.encoder_output_path.exists()
                ):
                    self.encoder_output_path.unlink()
            except Exception:
                pass
            self.encoder_process = None

        return uploaded

    @staticmethod
    async def merge_chunks(
        meeting_id: str, storage_path: str = "./data/recordings"
    ) -> Optional[bytes]:
        """
        Merge all audio chunks for a meeting into a single audio buffer.
        If chunks are missing but a merged file exists, returns that.

        Args:
            meeting_id: Meeting ID to merge chunks for
            storage_path: Base path for recordings

        Returns:
            Optional[bytes]: Merged audio data or None if failed
        """
        try:
            storage_type = os.getenv("STORAGE_TYPE", "local").lower()
            chunk_prefix = os.getenv("AUDIO_CHUNK_PREFIX", "pcm_chunks")

            if storage_type == "gcp":
                try:
                    from ..storage import StorageService
                except (ImportError, ValueError):
                    from services.storage import StorageService

                prefix = f"{meeting_id}/{chunk_prefix}/"
                files = await StorageService.list_files(prefix)
                chunk_files = sorted([f for f in files if f.endswith(".pcm")])

                if not chunk_files:
                    logger.error(f"No audio chunks found in GCS for {meeting_id}")
                    return None

                merged_audio = bytearray()
                for blob_name in chunk_files:
                    data = await StorageService.download_bytes(blob_name)
                    if data:
                        merged_audio.extend(data)

                logger.info(
                    f"Merged {len(chunk_files)} chunks from GCS "
                    f"({len(merged_audio) / (16000 * 2):.1f}s of audio)"
                )
                return bytes(merged_audio)

            chunk_dir = Path(storage_path) / meeting_id

            if not chunk_dir.exists():
                logger.error(f"Recording directory not found: {chunk_dir}")
                return None

            # Check for existing merged files first
            merged_pcm = chunk_dir / "merged_recording.pcm"
            if merged_pcm.exists():
                logger.info(f"Found existing merged PCM file: {merged_pcm}")
                async with aiofiles.open(merged_pcm, "rb") as f:
                    return await f.read()

            merged_wav = chunk_dir / "merged_recording.wav"
            if merged_wav.exists():
                logger.info(f"Found existing merged WAV file: {merged_wav}")
                async with aiofiles.open(merged_wav, "rb") as f:
                    return await f.read()

            # Sort chunks by filename (ensures correct order)
            chunks = sorted(chunk_dir.glob("chunk_*.pcm"))

            if not chunks:
                logger.error(f"No audio chunks found in {chunk_dir}")
                return None

            # Merge all chunks
            merged_audio = bytearray()
            for chunk_path in chunks:
                async with aiofiles.open(chunk_path, "rb") as f:
                    chunk_data = await f.read()
                    merged_audio.extend(chunk_data)

            logger.info(
                f"Merged {len(chunks)} chunks "
                f"({len(merged_audio) / (16000 * 2):.1f}s of audio)"
            )

            return bytes(merged_audio)

        except Exception as e:
            logger.error(f"Failed to merge audio chunks: {e}")
            return None

    @staticmethod
    def convert_pcm_to_wav(pcm_data: bytes, sample_rate: int = 16000) -> bytes:
        """
        Convert raw PCM data to WAV format.

        Args:
            pcm_data: Raw PCM audio bytes
            sample_rate: Sample rate (default 16kHz)

        Returns:
            bytes: WAV file data
        """
        import io
        import wave

        wav_buffer = io.BytesIO()

        with wave.open(wav_buffer, "wb") as wav_file:
            wav_file.setnchannels(1)  # Mono
            wav_file.setsampwidth(2)  # 16-bit
            wav_file.setframerate(sample_rate)
            wav_file.writeframes(pcm_data)

        return wav_buffer.getvalue()

    async def get_status(self) -> Dict:
        """Get current recording status."""
        if not self.is_recording:
            return {
                "status": "stopped",
                "meeting_id": self.meeting_id,
                "chunks_saved": len(self.chunks_metadata),
            }

        current_duration = (
            time.time() - self.recording_start_time if self.recording_start_time else 0
        )
        buffer_duration = len(self.current_chunk_buffer) / (
            self.sample_rate * self.bytes_per_sample
        )

        return {
            "status": "recording",
            "meeting_id": self.meeting_id,
            "duration_seconds": current_duration,
            "chunks_saved": len(self.chunks_metadata),
            "buffer_duration": buffer_duration,
        }

    @staticmethod
    async def rename_recorder_folder(
        old_id: str, new_id: str, storage_path: str = "./data/recordings"
    ) -> bool:
        """
        Rename a recording directory (e.g. from session_id to meeting_id).
        """
        import shutil

        storage_type = os.getenv("STORAGE_TYPE", "local").lower()

        if storage_type == "gcp":
            try:
                try:
                    from ..storage import StorageService
                except (ImportError, ValueError):
                    from services.storage import StorageService

                old_prefix = f"{old_id}/"
                new_prefix = f"{new_id}/"
                files = await StorageService.list_files(old_prefix)

                if not files:
                    return False

                for f in files:
                    new_path = f.replace(old_prefix, new_prefix, 1)
                    await StorageService.copy_file(f, new_path)

                await StorageService.delete_prefix(old_prefix)
                logger.info(f"☁️ Renamed GCS prefix: {old_id} -> {new_id}")
                return True
            except Exception as e:
                logger.error(f"Error renaming GCS prefix: {e}")
                return False

        old_dir = Path(storage_path) / old_id
        new_dir = Path(storage_path) / new_id

        if not old_dir.exists():
            return False

        try:
            # If new_dir already exists (unlikely but possible), merge contents
            if new_dir.exists():
                for f in old_dir.iterdir():
                    shutil.move(str(f), str(new_dir / f.name))
                old_dir.rmdir()
            else:
                os.rename(str(old_dir), str(new_dir))

            logger.info(f"📁 Linked audio recording: {old_id} -> {new_id}")
            return True
        except Exception as e:
            logger.error(f"Error renaming recording folder: {e}")
            return False


# Global registry of active recorders
active_recorders: Dict[str, AudioRecorder] = {}


async def get_or_create_recorder(
    meeting_id: str, storage_path: str = "./data/recordings"
) -> AudioRecorder:
    """
    Get existing recorder or create new one for a meeting.

    Args:
        meeting_id: Meeting ID
        storage_path: Base storage path

    Returns:
        AudioRecorder instance
    """
    if meeting_id not in active_recorders:
        recorder = AudioRecorder(meeting_id, storage_path)
        await recorder.start()
        active_recorders[meeting_id] = recorder

    return active_recorders[meeting_id]


async def stop_recorder(meeting_id: str) -> Optional[Dict]:
    """
    Stop and cleanup recorder for a meeting.

    Args:
        meeting_id: Meeting ID

    Returns:
        Recording metadata or None if no recorder found
    """
    if meeting_id in active_recorders:
        recorder = active_recorders[meeting_id]
        metadata = await recorder.stop()
        del active_recorders[meeting_id]
        return metadata

    return None
