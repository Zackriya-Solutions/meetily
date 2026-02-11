"""Session-based speaker tracking using embeddings for consistent speaker IDs across chunks."""
import logging
import numpy as np
from typing import Dict, List, Any, Optional, Tuple
from dataclasses import dataclass, field
from pathlib import Path
import json
import torch

logger = logging.getLogger(__name__)

# Similarity threshold for matching speakers (cosine similarity)
SPEAKER_MATCH_THRESHOLD = 0.60

# Minimum duration (seconds) to persist embedding (skip noisy short segments)
MIN_DURATION_TO_PERSIST = 1.5


@dataclass
class SpeakerProfile:
    """Speaker profile with embedding and metadata."""
    speaker_id: str
    embeddings: List[np.ndarray] = field(default_factory=list)
    total_duration: float = 0.0
    chunk_count: int = 0

    @property
    def centroid(self) -> Optional[np.ndarray]:
        """Get the centroid (average) embedding for this speaker."""
        if not self.embeddings:
            return None
        return np.mean(self.embeddings, axis=0)

    def add_embedding(self, embedding: np.ndarray, duration: float = 0.0):
        """Add a new embedding observation, weighted by duration."""
        self.embeddings.append(embedding)
        self.total_duration += duration
        self.chunk_count += 1

        # Keep only last N embeddings to prevent memory bloat
        max_embeddings = 50
        if len(self.embeddings) > max_embeddings:
            # Keep embeddings with best coverage - simple approach: keep recent ones
            self.embeddings = self.embeddings[-max_embeddings:]


class SessionSpeakerStore:
    """In-memory store for speaker profiles per session."""

    def __init__(self, persist_dir: Optional[str] = None):
        """
        Initialize the session store.

        Args:
            persist_dir: Optional directory to persist embeddings between restarts
        """
        self.sessions: Dict[str, Dict[str, SpeakerProfile]] = {}
        self.persist_dir = Path(persist_dir) if persist_dir else None

        if self.persist_dir:
            self.persist_dir.mkdir(parents=True, exist_ok=True)
            logger.info(f"Speaker embedding persistence enabled: {self.persist_dir}")

    def get_session(self, session_id: str) -> Dict[str, SpeakerProfile]:
        """Get or create a session's speaker profiles."""
        if session_id not in self.sessions:
            # Try to load from disk if persistence is enabled
            if self.persist_dir:
                self._load_session(session_id)

            if session_id not in self.sessions:
                self.sessions[session_id] = {}
                logger.info(f"Created new session: {session_id}")

        return self.sessions[session_id]

    def get_next_speaker_id(self, session_id: str) -> str:
        """Get the next available speaker ID for a session."""
        profiles = self.get_session(session_id)
        return f"SPEAKER_{len(profiles):02d}"

    def add_speaker(self, session_id: str, speaker_id: str, embedding: np.ndarray, duration: float = 0.0):
        """Add or update a speaker profile."""
        profiles = self.get_session(session_id)

        if speaker_id not in profiles:
            profiles[speaker_id] = SpeakerProfile(speaker_id=speaker_id)
            logger.debug(f"Session {session_id}: Created new speaker {speaker_id}")

        profiles[speaker_id].add_embedding(embedding, duration)

    def save_session(self, session_id: str):
        """Persist session embeddings to disk."""
        if not self.persist_dir:
            return

        profiles = self.get_session(session_id)
        if not profiles:
            return

        session_file = self.persist_dir / f"{session_id}.npz"

        try:
            # Save embeddings as numpy arrays
            data = {}
            for speaker_id, profile in profiles.items():
                if profile.centroid is not None:
                    data[f"{speaker_id}_centroid"] = profile.centroid
                    data[f"{speaker_id}_duration"] = np.array([profile.total_duration])
                    data[f"{speaker_id}_count"] = np.array([profile.chunk_count])

            np.savez(session_file, **data)
            logger.info(f"Saved session {session_id} with {len(profiles)} speakers")
        except Exception as e:
            logger.error(f"Failed to save session {session_id}: {e}")

    def _load_session(self, session_id: str):
        """Load session embeddings from disk."""
        if not self.persist_dir:
            return

        session_file = self.persist_dir / f"{session_id}.npz"

        if not session_file.exists():
            return

        try:
            data = np.load(session_file)
            profiles = {}

            # Find all speaker IDs
            speaker_ids = set()
            for key in data.files:
                if key.endswith("_centroid"):
                    speaker_ids.add(key.replace("_centroid", ""))

            for speaker_id in speaker_ids:
                centroid = data.get(f"{speaker_id}_centroid")
                duration = data.get(f"{speaker_id}_duration", [0.0])[0]
                count = int(data.get(f"{speaker_id}_count", [1])[0])

                if centroid is not None:
                    profile = SpeakerProfile(speaker_id=speaker_id)
                    profile.embeddings = [centroid]  # Start with centroid
                    profile.total_duration = duration
                    profile.chunk_count = count
                    profiles[speaker_id] = profile

            self.sessions[session_id] = profiles
            logger.info(f"Loaded session {session_id} with {len(profiles)} speakers")
        except Exception as e:
            logger.error(f"Failed to load session {session_id}: {e}")

    def clear_session(self, session_id: str):
        """Clear a session from memory and optionally disk."""
        if session_id in self.sessions:
            del self.sessions[session_id]

        if self.persist_dir:
            session_file = self.persist_dir / f"{session_id}.npz"
            if session_file.exists():
                session_file.unlink()


class SpeakerTracker:
    """
    Tracks speakers across audio chunks using embedding similarity.

    Uses pyannote's embedding model to extract speaker voice prints
    and matches them across chunks for consistent speaker IDs.
    """

    def __init__(self, auth_token: Optional[str], device: str = "cpu", persist_dir: Optional[str] = None):
        """
        Initialize the speaker tracker.

        Args:
            auth_token: Hugging Face auth token
            device: Device to run on ('cuda' or 'cpu')
            persist_dir: Directory to persist embeddings
        """
        self.auth_token = auth_token
        self.device = torch.device(device)
        self.embedding_model = None
        self.store = SessionSpeakerStore(persist_dir)

        self._load_embedding_model()

    def _load_embedding_model(self):
        """Load the speaker embedding model."""
        try:
            from pyannote.audio import Model, Inference

            logger.info("Loading speaker embedding model...")

            # Use pyannote's embedding model
            model = Model.from_pretrained(
                "pyannote/embedding",
                use_auth_token=self.auth_token
            )

            self.embedding_model = Inference(
                model,
                window="whole"
            )

            # Move to device
            if self.device.type == "cuda":
                self.embedding_model.model = self.embedding_model.model.to(self.device)

            logger.info("Speaker embedding model loaded successfully")

        except Exception as e:
            logger.error(f"Failed to load embedding model: {e}")
            self.embedding_model = None

    @property
    def is_available(self) -> bool:
        """Check if embedding model is available."""
        return self.embedding_model is not None

    def extract_embedding(self, audio_path: str, start: float, end: float) -> Optional[np.ndarray]:
        """
        Extract speaker embedding from an audio segment.

        Args:
            audio_path: Path to audio file
            start: Start time in seconds
            end: End time in seconds

        Returns:
            Speaker embedding vector or None if extraction fails
        """
        if not self.is_available:
            return None

        try:
            from pyannote.core import Segment

            # Extract embedding for the segment
            segment = Segment(start, end)
            embedding = self.embedding_model.crop(audio_path, segment)

            return embedding.flatten()

        except Exception as e:
            logger.debug(f"Embedding extraction failed for {start:.2f}-{end:.2f}: {e}")
            return None

    def cosine_similarity(self, a: np.ndarray, b: np.ndarray) -> float:
        """Calculate cosine similarity between two embeddings."""
        norm_a = np.linalg.norm(a)
        norm_b = np.linalg.norm(b)

        if norm_a == 0 or norm_b == 0:
            return 0.0

        return float(np.dot(a, b) / (norm_a * norm_b))

    def find_matching_speaker(
        self,
        session_id: str,
        embedding: np.ndarray
    ) -> Tuple[Optional[str], float]:
        """
        Find the best matching speaker in the session.

        Args:
            session_id: Session/meeting ID
            embedding: Speaker embedding to match

        Returns:
            Tuple of (speaker_id, similarity_score) or (None, 0.0) if no match
        """
        profiles = self.store.get_session(session_id)

        best_match = None
        best_similarity = 0.0

        for speaker_id, profile in profiles.items():
            centroid = profile.centroid
            if centroid is None:
                continue

            similarity = self.cosine_similarity(embedding, centroid)

            if similarity > best_similarity:
                best_similarity = similarity
                best_match = speaker_id

        return best_match, best_similarity

    def assign_speakers(
        self,
        session_id: str,
        audio_path: str,
        diarization_turns: List[Dict[str, Any]],
        num_speakers: Optional[int] = None
    ) -> List[Dict[str, Any]]:
        """
        Assign consistent speaker IDs to diarization turns using embeddings.

        Args:
            session_id: Session/meeting ID for tracking
            audio_path: Path to the audio file
            diarization_turns: Speaker turns from pyannote diarization
                              [{"speaker": "SPEAKER_00", "start": 0.5, "end": 2.3}, ...]

        Returns:
            Updated turns with consistent speaker IDs across chunks
        """
        if not self.is_available or not diarization_turns:
            logger.warning("Speaker tracking not available, returning original turns")
            return diarization_turns

        logger.info(f"Assigning speakers for session {session_id}, {len(diarization_turns)} turns")

        # Group turns by local speaker label to extract better embeddings
        local_speaker_turns: Dict[str, List[Dict]] = {}
        for turn in diarization_turns:
            local_label = turn["speaker"]
            if local_label not in local_speaker_turns:
                local_speaker_turns[local_label] = []
            local_speaker_turns[local_label].append(turn)

        # Map local speaker labels to session-consistent labels
        label_mapping: Dict[str, str] = {}

        for local_label, turns in local_speaker_turns.items():
            # Find the longest segment for this speaker (better embedding quality)
            best_turn = max(turns, key=lambda t: t["end"] - t["start"])
            duration = best_turn["end"] - best_turn["start"]

            # Skip very short segments
            if duration < 0.5:
                logger.debug(f"Skipping short segment for {local_label}: {duration:.2f}s")
                label_mapping[local_label] = local_label
                continue

            # Extract embedding
            embedding = self.extract_embedding(
                audio_path,
                best_turn["start"],
                best_turn["end"]
            )

            if embedding is None:
                logger.debug(f"Could not extract embedding for {local_label}")
                label_mapping[local_label] = local_label
                continue

            # Find matching speaker in session
            match_id, similarity = self.find_matching_speaker(session_id, embedding)

            if match_id and similarity >= SPEAKER_MATCH_THRESHOLD:
                # Found a match - use existing speaker ID
                label_mapping[local_label] = match_id
                logger.debug(f"Matched {local_label} -> {match_id} (similarity: {similarity:.3f})")

                # Only persist embedding if duration is good (skip noisy short segments)
                if duration >= MIN_DURATION_TO_PERSIST:
                    self.store.add_speaker(session_id, match_id, embedding, duration)
            else:
                # Check if we've hit the speaker limit (if user specified num_speakers)
                profiles = self.store.get_session(session_id)
                if num_speakers and len(profiles) >= num_speakers:
                    # At limit - assign to best match even if below threshold
                    if match_id:
                        label_mapping[local_label] = match_id
                        logger.debug(f"At speaker limit, using best match: {local_label} -> {match_id} (similarity: {similarity:.3f})")
                    else:
                        label_mapping[local_label] = "SPEAKER_00"
                else:
                    # New speaker
                    new_id = self.store.get_next_speaker_id(session_id)
                    label_mapping[local_label] = new_id
                    logger.info(f"New speaker: {local_label} -> {new_id} (best similarity: {similarity:.3f})")

                    # Only persist if duration is good
                    if duration >= MIN_DURATION_TO_PERSIST:
                        self.store.add_speaker(session_id, new_id, embedding, duration)

        # Apply mapping to all turns
        updated_turns = []
        for turn in diarization_turns:
            updated_turn = turn.copy()
            updated_turn["speaker"] = label_mapping.get(turn["speaker"], turn["speaker"])
            updated_turns.append(updated_turn)

        # Persist session
        self.store.save_session(session_id)

        return updated_turns

    def get_session_speakers(self, session_id: str) -> List[Dict[str, Any]]:
        """Get summary of speakers in a session."""
        profiles = self.store.get_session(session_id)

        return [
            {
                "speaker_id": speaker_id,
                "total_duration": profile.total_duration,
                "chunk_count": profile.chunk_count
            }
            for speaker_id, profile in profiles.items()
        ]

    def clear_session(self, session_id: str):
        """Clear a session's speaker data."""
        self.store.clear_session(session_id)
        logger.info(f"Cleared session: {session_id}")
