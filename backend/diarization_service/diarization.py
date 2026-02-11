"""Speaker diarization service using pyannote.audio."""
import logging
from typing import List, Dict, Any, Optional
import torch

logger = logging.getLogger(__name__)


class DiarizationService:
    """
    Speaker diarization service using pyannote.audio.

    Identifies different speakers in audio and returns time-stamped speaker turns.
    """

    def __init__(self, pipeline_name: str, auth_token: Optional[str], device: str = "cpu"):
        """
        Initialize the diarization service.

        Args:
            pipeline_name: Hugging Face model name (e.g., 'pyannote/speaker-diarization-3.1')
            auth_token: Hugging Face authentication token
            device: Device to run on ('cuda' or 'cpu')
        """
        self.pipeline_name = pipeline_name
        self.auth_token = auth_token
        self.device = torch.device(device)
        self.pipeline = None
        self._load_pipeline()

    def _load_pipeline(self) -> None:
        """Load the pyannote diarization pipeline."""
        try:
            from pyannote.audio import Pipeline

            logger.info(f"Loading diarization pipeline: {self.pipeline_name}")

            if not self.auth_token:
                logger.warning(
                    "No HF auth token provided. Pipeline loading may fail for gated models."
                )

            # pyannote.audio 3.x uses 'use_auth_token' parameter
            self.pipeline = Pipeline.from_pretrained(
                self.pipeline_name,
                use_auth_token=self.auth_token if self.auth_token else None
            )

            # Move pipeline to appropriate device
            if self.device.type == "cuda":
                self.pipeline = self.pipeline.to(self.device)
                logger.info("Diarization pipeline moved to GPU")

            logger.info("Diarization pipeline loaded successfully")

        except Exception as e:
            logger.error(f"Failed to load diarization pipeline: {e}")
            self.pipeline = None

    @property
    def is_available(self) -> bool:
        """Check if diarization pipeline is available."""
        return self.pipeline is not None

    def get_speaker_turns(self, audio_path: str, num_speakers: Optional[int] = None) -> List[Dict[str, Any]]:
        """
        Perform speaker diarization on an audio file.

        Args:
            audio_path: Path to the audio file (should be WAV 16kHz mono)
            num_speakers: Optional hint for number of speakers (constrains pyannote output)

        Returns:
            List of speaker turns with format:
            [{"speaker": "SPEAKER_00", "start": 0.5, "end": 2.3}, ...]
        """
        if not self.is_available:
            logger.warning("Diarization pipeline not available")
            return []

        try:
            logger.info(f"Running diarization on: {audio_path}" +
                       (f" (num_speakers={num_speakers})" if num_speakers else ""))

            # Run diarization with optional speaker count constraint
            if num_speakers:
                diarization_result = self.pipeline(audio_path, num_speakers=num_speakers)
            else:
                diarization_result = self.pipeline(audio_path)

            # Extract speaker turns
            speaker_turns = []
            for turn, _, speaker_label in diarization_result.itertracks(yield_label=True):
                speaker_turns.append({
                    "speaker": speaker_label,
                    "start": turn.start,
                    "end": turn.end
                })

            logger.info(f"Diarization complete: {len(speaker_turns)} speaker turns found")
            return speaker_turns

        except Exception as e:
            logger.error(f"Diarization failed: {e}")
            return []
