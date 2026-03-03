"""Client for communicating with whisper.cpp server."""
import logging
import httpx
from typing import List, Dict, Any, Optional

logger = logging.getLogger(__name__)


class WhisperClient:
    """
    Client for the whisper.cpp HTTP server.

    Forwards audio to the whisper.cpp server and returns transcription segments.
    """

    def __init__(self, server_url: str, timeout: float = 300.0):
        """
        Initialize the Whisper client.

        Args:
            server_url: URL of the whisper.cpp server (e.g., 'http://localhost:8178')
            timeout: Request timeout in seconds (default 5 minutes for long audio)
        """
        self.server_url = server_url.rstrip("/")
        self.timeout = timeout

    async def transcribe(self, audio_path: str) -> List[Dict[str, Any]]:
        """
        Send audio to whisper.cpp server for transcription.

        Args:
            audio_path: Path to the audio file

        Returns:
            List of transcription segments with format:
            [{"text": "Hello world", "start": 0.0, "end": 1.5}, ...]
        """
        try:
            logger.info(f"Sending audio to Whisper server: {self.server_url}")

            async with httpx.AsyncClient(timeout=self.timeout) as client:
                with open(audio_path, "rb") as audio_file:
                    files = {"file": ("audio.wav", audio_file, "audio/wav")}
                    data = {
                        "response_format": "verbose_json",
                        "temperature": "0.0",
                    }

                    response = await client.post(
                        f"{self.server_url}/inference",
                        files=files,
                        data=data
                    )
                    response.raise_for_status()

            result = response.json()
            logger.debug(f"Whisper raw response keys: {result.keys() if isinstance(result, dict) else type(result)}")
            segments = self._parse_whisper_response(result)

            logger.info(f"Whisper transcription complete: {len(segments)} segments")
            if segments:
                logger.debug(f"First segment: start={segments[0].get('start')}, end={segments[0].get('end')}")
            return segments

        except httpx.TimeoutException:
            logger.error("Whisper server request timed out")
            return []
        except httpx.HTTPStatusError as e:
            logger.error(f"Whisper server HTTP error: {e.response.status_code}")
            return []
        except Exception as e:
            logger.error(f"Whisper transcription failed: {e}")
            return []

    def _parse_whisper_response(self, response: dict) -> List[Dict[str, Any]]:
        """
        Parse the whisper.cpp server response into segments.

        The whisper.cpp server returns different formats depending on configuration.
        This handles the common formats.
        """
        segments = []

        # Handle different response formats from whisper.cpp
        if "segments" in response:
            # Standard segments format
            for seg in response["segments"]:
                segments.append({
                    "text": seg.get("text", "").strip(),
                    "start": seg.get("start", 0.0),
                    "end": seg.get("end", 0.0)
                })
        elif "text" in response:
            # Simple text response (no timestamps)
            # Create a single segment
            segments.append({
                "text": response["text"].strip(),
                "start": 0.0,
                "end": 0.0
            })

        return segments

    async def health_check(self) -> bool:
        """Check if the Whisper server is available."""
        try:
            async with httpx.AsyncClient(timeout=5.0) as client:
                response = await client.get(f"{self.server_url}/")
                return response.status_code == 200
        except Exception:
            return False
