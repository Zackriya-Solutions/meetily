"""
Rolling audio buffer for streaming transcription.
Maintains a sliding window of audio for continuous processing.
"""

import numpy as np
from collections import deque
import logging

logger = logging.getLogger(__name__)


class RollingAudioBuffer:
    """
    Maintains a sliding window of audio for streaming transcription.

    Example:
        Time 0.0s: [--2s buffer--] → Transcribe
        Time 0.5s: [--2s buffer--] → Transcribe (500ms overlap)
        Time 1.0s: [--2s buffer--] → Transcribe

    The overlap provides context for better transcription accuracy.
    """

    def __init__(
        self,
        window_duration_ms: int = 2000,  # 2 seconds
        slide_duration_ms: int = 500,  # 500ms step
        sample_rate: int = 16000,
    ):
        """
        Args:
            window_duration_ms: Size of sliding window in milliseconds
            slide_duration_ms: How often to process (step size)
            sample_rate: Audio sample rate in Hz
        """
        self.window_duration_ms = window_duration_ms
        self.slide_duration_ms = slide_duration_ms
        self.sample_rate = sample_rate

        # Calculate sizes in samples
        self.window_size = int((window_duration_ms / 1000) * sample_rate)
        self.slide_size = int((slide_duration_ms / 1000) * sample_rate)

        # Buffer storage (circular buffer)
        self.buffer = deque(maxlen=self.window_size)
        self.samples_since_last_slide = 0

        logger.info(
            f"✅ RollingBuffer initialized: "
            f"window={window_duration_ms}ms ({self.window_size} samples), "
            f"slide={slide_duration_ms}ms ({self.slide_size} samples)"
        )

    def add_samples(self, samples: np.ndarray) -> bool:
        """
        Add audio samples to buffer.

        Args:
            samples: NumPy array of audio samples (int16)

        Returns:
            True if enough samples accumulated for next window
        """
        # Add samples to circular buffer
        self.buffer.extend(samples)
        self.samples_since_last_slide += len(samples)

        # Check if we should process next window
        should_process = self.samples_since_last_slide >= self.slide_size

        if should_process:
            self.samples_since_last_slide = 0

        return should_process

    def get_window(self) -> np.ndarray:
        """
        Get current audio window as NumPy array.

        Returns:
            Array of shape (window_size,) with audio samples (int16)
        """
        if len(self.buffer) < self.window_size:
            # Pad with zeros if buffer not full yet
            window = np.zeros(self.window_size, dtype=np.int16)
            window[-len(self.buffer) :] = list(self.buffer)
            return window

        return np.array(self.buffer, dtype=np.int16)

    def get_window_bytes(self) -> bytes:
        """
        Get current window as raw bytes for sending to Groq API.

        Returns:
            Raw PCM bytes (16-bit, mono, 16kHz)
        """
        return self.get_window().tobytes()

    def get_buffer_duration_ms(self) -> float:
        """Get current buffer duration in milliseconds"""
        return (len(self.buffer) / self.sample_rate) * 1000

    def is_buffer_full(self) -> bool:
        """Check if buffer has reached minimum viable window size (90% of target)"""
        # Use 90% threshold to avoid edge cases where buffer never fully fills
        min_viable_size = int(self.window_size * 0.9)
        return len(self.buffer) >= min_viable_size

    def get_all_samples_bytes(self) -> bytes:
        """
        Get all samples currently in the buffer as raw bytes.
        Useful for flushing remaining audio.
        """
        return np.array(self.buffer, dtype=np.int16).tobytes()

    def reconfigure(
        self,
        window_duration_ms: int,
        slide_duration_ms: int,
        preserve_tail: bool = True,
    ) -> bool:
        """
        Dynamically update window/slide settings.

        Returns:
            True when configuration changed, False when unchanged/invalid.
        """
        if window_duration_ms <= 0 or slide_duration_ms <= 0:
            return False
        if window_duration_ms == self.window_duration_ms and slide_duration_ms == self.slide_duration_ms:
            return False

        old_samples = list(self.buffer) if preserve_tail else []
        self.window_duration_ms = int(window_duration_ms)
        self.slide_duration_ms = int(slide_duration_ms)
        self.window_size = int((self.window_duration_ms / 1000) * self.sample_rate)
        self.slide_size = int((self.slide_duration_ms / 1000) * self.sample_rate)
        self.buffer = deque(maxlen=self.window_size)
        if old_samples:
            self.buffer.extend(old_samples[-self.window_size :])
        self.samples_since_last_slide = min(self.samples_since_last_slide, self.slide_size)
        logger.info(
            "🔧 RollingBuffer reconfigured: window=%sms (%s samples), slide=%sms (%s samples)",
            self.window_duration_ms,
            self.window_size,
            self.slide_duration_ms,
            self.slide_size,
        )
        return True

    def clear(self):
        """Clear the buffer"""
        self.buffer.clear()
        self.samples_since_last_slide = 0
        logger.debug("Buffer cleared")
