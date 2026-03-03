"""Audio utility functions for format conversion."""
import subprocess
import logging
import os
from typing import Optional

logger = logging.getLogger(__name__)


class AudioConverter:
    """Handles audio format conversion using ffmpeg."""

    @staticmethod
    def convert_to_wav(
        input_path: str,
        output_path: str,
        sample_rate: int = 16000,
        channels: int = 1
    ) -> bool:
        """
        Convert audio file to WAV format suitable for diarization.

        Args:
            input_path: Path to input audio file
            output_path: Path to output WAV file
            sample_rate: Target sample rate (default 16kHz for pyannote)
            channels: Number of audio channels (default 1 for mono)

        Returns:
            True if conversion successful, False otherwise
        """
        try:
            cmd = [
                "ffmpeg",
                "-i", input_path,
                "-ar", str(sample_rate),
                "-ac", str(channels),
                "-y",  # Overwrite output
                output_path
            ]

            result = subprocess.run(
                cmd,
                check=True,
                capture_output=True,
                text=True
            )

            logger.debug(f"Audio conversion successful: {input_path} -> {output_path}")
            return True

        except subprocess.CalledProcessError as e:
            logger.error(f"FFmpeg conversion failed: {e.stderr}")
            return False
        except FileNotFoundError:
            logger.error("FFmpeg not found. Please install ffmpeg.")
            return False
        except Exception as e:
            logger.error(f"Audio conversion error: {e}")
            return False

    @staticmethod
    def cleanup_temp_file(file_path: str) -> None:
        """Safely remove a temporary file."""
        try:
            if file_path and os.path.exists(file_path):
                os.remove(file_path)
                logger.debug(f"Cleaned up temp file: {file_path}")
        except Exception as e:
            logger.warning(f"Failed to cleanup temp file {file_path}: {e}")
