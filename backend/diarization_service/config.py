"""Configuration for the diarization service."""
import os
import torch
import logging
from dataclasses import dataclass, field
from typing import Optional

logger = logging.getLogger(__name__)


@dataclass
class DiarizationConfig:
    """Configuration for transcription and diarization services."""

    # Whisper.cpp server settings
    whisper_server_url: str = field(
        default_factory=lambda: os.getenv("WHISPER_SERVER_URL", "http://localhost:8178")
    )

    # Pyannote diarization settings
    diarization_pipeline_name: str = field(
        default_factory=lambda: os.getenv(
            "DIARIZATION_PIPELINE",
            "pyannote/speaker-diarization-3.1"
        )
    )
    hf_auth_token: Optional[str] = field(
        default_factory=lambda: os.getenv("HF_AUTH_TOKEN", "")
    )

    # Audio conversion settings (for diarization)
    audio_convert_sample_rate: int = 16000
    audio_convert_channels: int = 1

    # Speaker embedding persistence directory (optional, for cross-chunk tracking)
    speaker_embedding_dir: Optional[str] = field(
        default_factory=lambda: os.getenv("SPEAKER_EMBEDDING_DIR", "")
    )

    # Device settings
    device_str: str = field(init=False)

    def __post_init__(self):
        """Detect available device after initialization."""
        if torch.cuda.is_available():
            self.device_str = "cuda"
            logger.info("CUDA available - using GPU for diarization")
        else:
            self.device_str = "cpu"
            logger.info("CUDA not available - using CPU for diarization")

        if not self.hf_auth_token:
            logger.warning(
                "HF_AUTH_TOKEN not set. Pyannote models require authentication. "
                "Set HF_AUTH_TOKEN environment variable with your Hugging Face token."
            )
