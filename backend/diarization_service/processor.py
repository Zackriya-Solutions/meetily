"""Main audio processing service that combines transcription and diarization."""
import os
import logging
from typing import List, Dict, Any, Optional

from .config import DiarizationConfig
from .audio_utils import AudioConverter
from .diarization import DiarizationService
from .whisper_client import WhisperClient
from .speaker_tracker import SpeakerTracker

logger = logging.getLogger(__name__)


class AudioProcessor:
    """
    Main service that orchestrates transcription and diarization.

    Workflow:
    1. Receive audio file
    2. Forward to whisper.cpp for transcription
    3. Run pyannote for speaker diarization
    4. Use speaker tracker for consistent IDs across chunks (if session_id provided)
    5. Merge results by time overlap
    6. Return segments with speaker labels
    """

    def __init__(self, config: DiarizationConfig):
        """
        Initialize the audio processor.

        Args:
            config: Configuration object with settings
        """
        self.config = config
        self.audio_converter = AudioConverter()

        # Initialize whisper client
        self.whisper_client = WhisperClient(config.whisper_server_url)

        # Initialize diarization service
        self.diarization_service = DiarizationService(
            pipeline_name=config.diarization_pipeline_name,
            auth_token=config.hf_auth_token,
            device=config.device_str
        )

        # Initialize speaker tracker for cross-chunk consistency
        persist_dir = config.speaker_embedding_dir if config.speaker_embedding_dir else None
        self.speaker_tracker = SpeakerTracker(
            auth_token=config.hf_auth_token,
            device=config.device_str,
            persist_dir=persist_dir
        )

        logger.info(f"AudioProcessor initialized")
        logger.info(f"  Whisper server: {config.whisper_server_url}")
        logger.info(f"  Diarization available: {self.diarization_service.is_available}")
        logger.info(f"  Speaker tracking available: {self.speaker_tracker.is_available}")
        logger.info(f"  Device: {config.device_str}")

    @property
    def diarization_available(self) -> bool:
        """Check if diarization is available."""
        return self.diarization_service.is_available

    @property
    def speaker_tracking_available(self) -> bool:
        """Check if speaker tracking is available."""
        return self.speaker_tracker.is_available

    async def process_audio(
        self,
        audio_path: str,
        enable_diarization: bool = True,
        session_id: Optional[str] = None,
        num_speakers: Optional[int] = None
    ) -> List[Dict[str, Any]]:
        """
        Process audio file with transcription and optional diarization.

        Args:
            audio_path: Path to the audio file
            enable_diarization: Whether to run speaker diarization
            session_id: Optional session/meeting ID for cross-chunk speaker tracking

        Returns:
            List of segments with format:
            [{"text": "Hello", "start": 0.0, "end": 1.5, "speaker": "SPEAKER_00"}, ...]
        """
        temp_wav_path: Optional[str] = None

        try:
            # Step 1: Get transcription from whisper.cpp
            logger.info("Step 1: Transcribing audio with Whisper")
            whisper_segments = await self.whisper_client.transcribe(audio_path)

            if not whisper_segments:
                logger.warning("Whisper returned no segments")
                return []

            logger.info(f"Got {len(whisper_segments)} segments from Whisper")

            # Step 2: Run diarization if enabled and available
            diarization_turns: List[Dict[str, Any]] = []

            if enable_diarization and self.diarization_service.is_available:
                logger.info("Step 2: Running speaker diarization")

                # Convert audio to WAV format for pyannote (16kHz mono)
                base, _ = os.path.splitext(audio_path)
                temp_wav_path = f"{base}_diarization.wav"

                conversion_success = self.audio_converter.convert_to_wav(
                    input_path=audio_path,
                    output_path=temp_wav_path,
                    sample_rate=self.config.audio_convert_sample_rate,
                    channels=self.config.audio_convert_channels
                )

                if conversion_success:
                    diarization_turns = self.diarization_service.get_speaker_turns(
                        temp_wav_path,
                        num_speakers=num_speakers
                    )
                    logger.info(f"Got {len(diarization_turns)} speaker turns")

                    # Step 2b: Apply speaker tracking for consistent IDs
                    if session_id and self.speaker_tracker.is_available and diarization_turns:
                        logger.info(f"Step 2b: Applying speaker tracking for session {session_id}" +
                                   (f" (max {num_speakers} speakers)" if num_speakers else ""))
                        diarization_turns = self.speaker_tracker.assign_speakers(
                            session_id=session_id,
                            audio_path=temp_wav_path,
                            diarization_turns=diarization_turns,
                            num_speakers=num_speakers
                        )
                else:
                    logger.warning("Audio conversion failed, skipping diarization")
            else:
                if not enable_diarization:
                    logger.info("Diarization disabled by request")
                else:
                    logger.warning("Diarization not available")

            # Step 3: Merge transcription with speaker labels
            logger.info("Step 3: Merging results")
            merged_segments = self._merge_results(whisper_segments, diarization_turns)

            logger.info(f"Processing complete: {len(merged_segments)} segments")
            return merged_segments

        except Exception as e:
            logger.error(f"Audio processing failed: {e}")
            return []

        finally:
            # Cleanup temp WAV file
            if temp_wav_path:
                self.audio_converter.cleanup_temp_file(temp_wav_path)

    def _merge_results(
        self,
        whisper_segments: List[Dict[str, Any]],
        diarization_turns: List[Dict[str, Any]]
    ) -> List[Dict[str, Any]]:
        """
        Merge Whisper transcription segments with speaker diarization.

        Uses maximum time overlap to assign speakers to segments.

        Args:
            whisper_segments: Transcription segments from Whisper
            diarization_turns: Speaker turns from pyannote

        Returns:
            Merged segments with speaker labels
        """
        if not diarization_turns:
            # No diarization available - return segments with UNKNOWN speaker
            logger.info("No diarization turns, marking all segments as UNKNOWN")
            return [
                {**seg, "speaker": "UNKNOWN"}
                for seg in whisper_segments
            ]

        merged = []

        for seg in whisper_segments:
            seg_start = seg.get("start", 0.0)
            seg_end = seg.get("end", 0.0)

            best_speaker = "UNKNOWN"
            max_overlap = 0.0

            # Find the speaker with maximum time overlap
            for turn in diarization_turns:
                turn_start = turn["start"]
                turn_end = turn["end"]

                # Calculate overlap
                overlap_start = max(seg_start, turn_start)
                overlap_end = min(seg_end, turn_end)
                overlap_duration = max(0.0, overlap_end - overlap_start)

                if overlap_duration > max_overlap:
                    max_overlap = overlap_duration
                    best_speaker = turn["speaker"]

            merged.append({
                "text": seg.get("text", "").strip(),
                "start": seg_start,
                "end": seg_end,
                "speaker": best_speaker
            })

        return merged

    async def transcribe_only(self, audio_path: str) -> List[Dict[str, Any]]:
        """
        Transcribe audio without diarization.

        Args:
            audio_path: Path to the audio file

        Returns:
            List of transcription segments (no speaker labels)
        """
        return await self.whisper_client.transcribe(audio_path)

    def get_session_speakers(self, session_id: str) -> List[Dict[str, Any]]:
        """Get summary of speakers in a session."""
        return self.speaker_tracker.get_session_speakers(session_id)

    def clear_session(self, session_id: str):
        """Clear a session's speaker tracking data."""
        self.speaker_tracker.clear_session(session_id)
