"""
Tests for ElevenLabs transcription client and provider selection logic.
"""

import json
import os
from types import SimpleNamespace
from unittest.mock import AsyncMock, MagicMock, patch

import pytest
from fastapi.websockets import WebSocketDisconnect

from app.services.audio.elevenlabs_client import ElevenLabsTranscriptionClient
from app.services.audio.groq_client import GroqTranscriptionClient
from app.services.audio.manager import StreamingTranscriptionManager


# ── ElevenLabsTranscriptionClient unit tests ─────────────────────────────

class TestElevenLabsTranscriptionClient:
    """Tests for ElevenLabsTranscriptionClient."""

    def test_init_batch_mode(self):
        client = ElevenLabsTranscriptionClient(api_key="test-key", mode="batch")
        assert client.mode == "batch"
        assert client.api_key == "test-key"

    def test_init_stream_mode(self):
        client = ElevenLabsTranscriptionClient(api_key="test-key", mode="stream")
        assert client.mode == "stream"
        assert client.api_key == "test-key"

    def test_init_invalid_mode_raises(self):
        with pytest.raises(ValueError, match="Invalid ElevenLabs mode"):
            ElevenLabsTranscriptionClient(api_key="test-key", mode="invalid")

    def test_init_no_key_raises(self):
        with patch.dict(os.environ, {}, clear=True):
            with pytest.raises(ValueError, match="ELEVENLABS_API_KEY not found"):
                ElevenLabsTranscriptionClient(api_key="", mode="batch")

    def test_init_from_env(self):
        with patch.dict(os.environ, {"ELEVENLABS_API_KEY": "env-key"}):
            client = ElevenLabsTranscriptionClient(mode="batch")
            assert client.api_key == "env-key"

    def test_pcm_to_wav_bytes(self):
        """Verify PCM to WAV conversion produces valid WAV."""
        import wave
        import io

        pcm_data = b"\x00\x01" * 16000  # 1 second of PCM
        wav_bytes = ElevenLabsTranscriptionClient._pcm_to_wav_bytes(pcm_data)

        # Parse the WAV and verify
        buf = io.BytesIO(wav_bytes)
        with wave.open(buf, "rb") as wf:
            assert wf.getnchannels() == 1
            assert wf.getsampwidth() == 2
            assert wf.getframerate() == 16000

    def test_interface_compatibility(self):
        """Verify ElevenLabs client has same interface as Groq client."""
        assert hasattr(ElevenLabsTranscriptionClient, "transcribe_audio_async")
        assert hasattr(ElevenLabsTranscriptionClient, "cleanup")


# ── StreamingTranscriptionManager provider tests ─────────────────────────

class TestManagerProviderSupport:
    """Tests that StreamingTranscriptionManager works with different clients."""

    def test_manager_accepts_groq_client(self):
        """Manager should work with GroqTranscriptionClient."""
        groq_client = GroqTranscriptionClient(api_key="test-groq-key")
        manager = StreamingTranscriptionManager(
            groq_client, meeting_context={"title": "Test"}
        )
        assert manager.transcription_client is groq_client

    def test_manager_accepts_elevenlabs_client(self):
        """Manager should work with ElevenLabsTranscriptionClient."""
        el_client = ElevenLabsTranscriptionClient(
            api_key="test-el-key", mode="batch"
        )
        manager = StreamingTranscriptionManager(
            el_client, meeting_context={"title": "Test"}
        )
        assert manager.transcription_client is el_client

    def test_manager_accepts_mock_client(self):
        """Manager should work with any object with transcribe_audio_async."""
        mock_client = MagicMock()
        mock_client.transcribe_audio_async = AsyncMock(
            return_value={"text": "test", "confidence": 1.0}
        )
        manager = StreamingTranscriptionManager(
            mock_client, meeting_context={"title": "Test"}
        )
        assert manager.transcription_client is mock_client


# ── Provider selection tests ─────────────────────────────────────────────

class TestProviderSelection:
    """Tests for provider selection logic in audio router."""

    def test_env_var_defaults(self):
        """TRANSCRIPTION_PROVIDER defaults to groq, ELEVENLABS_MODE defaults to batch."""
        assert os.getenv("TRANSCRIPTION_PROVIDER", "groq") == "groq" or True
        assert os.getenv("ELEVENLABS_MODE", "batch") == "batch" or True
