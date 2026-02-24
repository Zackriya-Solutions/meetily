"""
Streaming transcription manager.
Orchestrates: Audio → VAD → Rolling Buffer → Groq API → Partial/Final transcripts

QUALITY IMPROVEMENTS (Phase 3):
- Better deduplication with sentence hashing
- Silero VAD for accurate speech detection (with SimpleVAD fallback)
- Reduced overlap (1.5s instead of 3s)
- Sentence boundary detection
- Debounced final emissions
"""

import asyncio
import numpy as np
import logging
import time
import hashlib
from typing import Optional, Callable, Set
from concurrent.futures import ThreadPoolExecutor
from .groq_client import GroqTranscriptionClient
from .buffer import RollingAudioBuffer
from .vad import SimpleVAD, SileroVAD, TenVAD

logger = logging.getLogger(__name__)


class StreamingTranscriptionManager:
    """
    Orchestrates real-time transcription pipeline:
    Audio → VAD → Rolling Buffer → Groq API → Partial/Final
    """

    def __init__(self, groq_api_key: str):
        """
        Args:
            groq_api_key: Groq API key for Whisper Large v3
        """
        self.groq = GroqTranscriptionClient(groq_api_key)

        # VAD Initialization Strategy: ONLY TenVAD
        self.vad = None

        # 1. TenVAD (High Performance C++) - STRICT enforcement
        try:
            self.vad = TenVAD(threshold=0.5)
            logger.info("✅ Using TenVAD (C++ based) with threshold 0.5")
        except Exception as e:
            logger.error(f"❌ TenVAD failed to load: {e}")
            raise RuntimeError("TenVAD is required but failed to load. Aborting.")

        # COMMENTED OUT: No fallbacks allowed per user request
        # 2. Try SileroVAD (ML based, PyTorch)
        # if self.vad is None:
        #     try:
        #         self.vad = SileroVAD(threshold=0.3)
        #         logger.info("✅ Using SileroVAD (ML-based)")
        #     except Exception as e:
        #         logger.warning(f"⚠️ SileroVAD failed to load: {e}")

        # 3. Fallback to SimpleVAD (Amplitude based)
        # if self.vad is None:
        #     self.vad = SimpleVAD(threshold=0.08)
        #     logger.info("ℹ️ Using SimpleVAD (Fallback)")

        # IMPROVED: Optimized for real-time responsiveness
        # 6s window provides enough context for grammar, but is short enough to fail fast
        self.buffer = RollingAudioBuffer(
            window_duration_ms=6000,  # 6s window (was 12s)
            slide_duration_ms=2000,  # 2s slide (matches transcription interval)
        )

        # Transcript state
        self.last_partial_text = ""
        self.last_final_text = ""
        self.silence_duration_ms = 0
        self.same_text_count = 0
        self.is_speaking = False

        # IMPROVED: Track finalized sentence hashes to prevent duplicates
        self.finalized_hashes: Set[str] = set()
        self.finalized_words: Set[str] = (
            set()
        )  # Track individual words for overlap detection

        # Thread pool for Groq API calls (blocking)
        self.executor = ThreadPoolExecutor(max_workers=2)

        # Performance metrics
        self.total_chunks_processed = 0
        self.total_transcriptions = 0

        # SMART TIMER CONFIG
        self.session_start_time = (
            time.time()
        )  # Track when the entire streaming session started
        self.last_transcription_time = 0
        self.last_chunk_timestamp = (
            0.0  # Track last client timestamp for monotonicity validation
        )
        self.last_raw_client_timestamp: Optional[float] = None
        self.client_timestamp_offset = 0.0
        self.speech_start_time = (
            0.0  # Start time of current speech segment (client time)
        )
        self.speech_end_time = 0.0  # End time of current speech segment (client time)

        self.last_speech_time = time.time()  # Track last time speech was detected

        # Smart trigger thresholds
        self.silence_threshold_ms = 1000  # 1.0s silence → finalize
        self.max_buffer_duration_ms = 6000  # 6s max → force finalize (matches window)
        self.punctuation_min_duration_ms = 2000  # Punctuation + 2s → finalize
        self.min_transcription_interval = (
            3.0  # Check every 3.0s (less frequency to avoid Groq 429)
        )

        logger.info(
            "✅ StreamingTranscriptionManager initialized (SMART TIMER: 1.0s silence, 6s max)"
        )

    async def process_audio_chunk(
        self,
        audio_data: bytes,
        client_timestamp: Optional[float] = None,
        on_partial: Optional[Callable] = None,
        on_final: Optional[Callable] = None,
        on_error: Optional[Callable] = None,
    ):
        """
        Process incoming audio chunk with client-provided timestamp for precision.

        Args:
            audio_data: Raw PCM audio bytes (16kHz, mono, 16-bit)
            client_timestamp: Start time from client's AudioContext.currentTime (seconds)
                             This is the SOURCE OF TRUTH for timing, not server arrival time
            on_partial: Callback for partial transcripts
            on_final: Callback for final transcripts
            on_error: Callback for error messages
        """
        # Calculate chunk duration early so timestamp normalization can use it.
        # 16kHz, 16-bit (2 bytes) = 32000 bytes/sec
        chunk_duration = len(audio_data) / 32000.0

        # Use client timestamp as source of truth (prevents network jitter)
        if client_timestamp is None:
            # Fallback: estimate based on session start (legacy mode)
            if self.session_start_time == 0:
                self.session_start_time = time.time()
            timestamp = time.time() - self.session_start_time
            logger.warning(
                "No client_timestamp provided, falling back to server time (not recommended)"
            )
        else:
            raw_timestamp = client_timestamp

            # Handle websocket reconnect / AudioContext reset:
            # if client timestamp jumps backwards, shift by an offset so timeline stays continuous.
            if (
                self.last_raw_client_timestamp is not None
                and raw_timestamp + 0.05 < self.last_raw_client_timestamp
            ):
                proposed_offset = (self.last_chunk_timestamp + chunk_duration) - raw_timestamp
                if proposed_offset > self.client_timestamp_offset:
                    self.client_timestamp_offset = proposed_offset
                logger.info(
                    "Client timestamp reset detected: raw %.3fs -> %.3fs, applying offset %.3fs",
                    self.last_raw_client_timestamp,
                    raw_timestamp,
                    self.client_timestamp_offset,
                )

            timestamp = raw_timestamp + self.client_timestamp_offset

            # Final monotonic guard for any residual jitter.
            if timestamp < self.last_chunk_timestamp:
                logger.warning(
                    f"Non-monotonic timestamp detected: {timestamp:.3f}s < {self.last_chunk_timestamp:.3f}s. "
                    f"Adjusting to prevent time travel."
                )
                timestamp = self.last_chunk_timestamp + min(max(chunk_duration, 0.02), 0.1)

            self.last_raw_client_timestamp = raw_timestamp
            self.last_chunk_timestamp = timestamp

        # Track processing time for performance monitoring
        start_time = time.time()

        current_end_time = timestamp + chunk_duration

        # Convert bytes to numpy array
        audio_samples = np.frombuffer(audio_data, dtype=np.int16)

        # Check for speech
        is_speech = self.vad.is_speech(audio_samples)

        # CRITICAL FIX: Always add to buffer to maintain time continuity
        # Previously, silence was dropped, causing the buffer to never fill if speech was sparse
        self.buffer.add_samples(audio_samples)

        if is_speech:
            self.last_speech_time = time.time()
            if not self.is_speaking:
                logger.debug(f"🎤 Speech started at {timestamp:.3f}s")
                self.is_speaking = True
                self.speech_start_time = timestamp

            # Update end time continuously while speaking
            self.speech_end_time = current_end_time
            self.silence_duration_ms = 0

        else:
            # Silence detected
            if self.is_speaking:
                self.silence_duration_ms += chunk_duration * 1000  # Use actual duration

                # SMART TRIGGER: Finalize if silence > 1200ms
                if (
                    self.silence_duration_ms > self.silence_threshold_ms
                    and self.last_partial_text
                ):
                    # Hash-based deduplication check
                    sentence_hash = self._get_sentence_hash(self.last_partial_text)

                    if sentence_hash not in self.finalized_hashes:
                        logger.info(
                            f"🔇 SMART TRIGGER: Silence ({self.silence_duration_ms:.0f}ms > {self.silence_threshold_ms}ms)"
                        )

                        if on_final:
                            await on_final(
                                {
                                    "text": self.last_partial_text,
                                    "confidence": 1.0,
                                    "reason": "silence",
                                    "audio_start_time": self.speech_start_time,
                                    "audio_end_time": self.speech_end_time,
                                    "duration": self.speech_end_time
                                    - self.speech_start_time,
                                }
                            )

                        self.finalized_hashes.add(sentence_hash)
                        self.last_final_text += " " + self.last_partial_text
                    else:
                        logger.debug(
                            f"⏭️  Skipping duplicate (silence): '{self.last_partial_text[:50]}...'"
                        )

                    self.last_partial_text = ""
                    self.same_text_count = 0
                    self.is_speaking = False
                    self.speech_start_time = 0  # Reset speech timer

        # Check triggers regardless of current speech state (since buffer is filling)
        buffer_duration = self.buffer.get_buffer_duration_ms()
        is_full = self.buffer.is_buffer_full()
        current_time = time.time()
        time_since_last = current_time - self.last_transcription_time

        # Trigger transcription if:
        # 1. Buffer is full
        # 2. Enough time passed since last transcribe
        # 3. We have heard speech recently (within the window duration)
        #    This prevents transcribing 12s of pure silence.
        has_recent_speech = (current_time - self.last_speech_time) < (
            self.buffer.window_duration_ms / 1000
        )

        if is_full and time_since_last >= self.min_transcription_interval:
            if has_recent_speech:
                logger.info(
                    f"🚀 Transcription triggered (buffer={buffer_duration:.0f}ms)"
                )

                # Get window and transcribe
                window_bytes = self.buffer.get_window_bytes()
                self.last_transcription_time = current_time

                # Transcribe with Groq (run in thread pool since it's blocking)
                loop = asyncio.get_event_loop()
                result = await loop.run_in_executor(
                    self.executor,
                    self.groq.transcribe_audio_sync,
                    window_bytes,
                    "auto",  # Auto-detect language
                    self.last_final_text[-100:] if self.last_final_text else None,
                    True,  # translate_to_english=True
                )

                if result.get("error") == "rate_limit_exceeded":
                    logger.warning("⚠️ Groq Rate Limit Exceeded")
                    if on_error:
                        await on_error(
                            "Groq API Rate Limit Reached. Please wait a moment or check your plan.",
                            code="GROQ_RATE_LIMIT",
                        )
                elif result.get("error") and (
                    "401" in str(result.get("error"))
                    or "invalid_api_key" in str(result.get("error"))
                ):
                    logger.error("❌ Groq Invalid API Key")
                    if on_error:
                        await on_error(
                            "Groq API Key is invalid or missing. Please check your settings.",
                            code="GROQ_KEY_REQUIRED",
                        )
                elif result["text"]:
                    self.total_transcriptions += 1
                    await self._handle_transcript(
                        text=result["text"],
                        confidence=result.get("confidence", 1.0),
                        on_partial=on_partial,
                        on_final=on_final,
                        metadata=result,
                    )
            else:
                # Buffer is full but it's just silence.
                # Update timestamp to prevent spinning, but don't call API.
                # effectively "skipping" this silent window.
                self.last_transcription_time = current_time
                # logger.debug("Skipping transcription (silence)")

        self.total_chunks_processed += 1

        processing_time = time.time() - start_time
        if processing_time > 0.1:  # Log if taking >100ms
            logger.debug(f"⏱️  Chunk processing: {processing_time * 1000:.1f}ms")

    def _word_similarity(self, words1: list, words2: list) -> float:
        """Calculate similarity between two word lists (0.0 to 1.0)."""
        if not words1 or not words2:
            return 0.0
        # Count matching words
        set1 = set(w.lower() for w in words1)
        set2 = set(w.lower() for w in words2)
        intersection = len(set1 & set2)
        union = len(set1 | set2)
        return intersection / union if union > 0 else 0.0

    def _remove_overlap(self, new_text: str) -> str:
        """
        Remove overlapping text using fuzzy matching.
        Handles Whisper's slight wording variations in overlapping audio.

        Uses word-level similarity to detect overlaps even when exact text differs.
        """
        if not self.last_final_text:
            return new_text

        final_words = self.last_final_text.split()
        new_words = new_text.split()

        if len(new_words) < 4:
            return new_text

        # Look for overlaps in the first 40% of new text (overlap window)
        max_overlap_check = min(20, len(new_words) // 2 + 5)

        # Get last 30 words of previous transcript for comparison
        search_window = min(
            50, len(final_words)
        )  # Increased from 30 to catch more context
        final_tail_words = final_words[-search_window:]

        best_overlap = 0
        similarity_threshold = 0.5  # Lowered from 0.6 to catch more overlaps

        # Try different overlap sizes, largest first
        for overlap_size in range(max_overlap_check, 2, -1):
            new_head = new_words[:overlap_size]

            # Check similarity against different positions in final tail
            for start in range(0, min(15, search_window)):
                if start + overlap_size > search_window:
                    break

                final_segment = final_tail_words[start : start + overlap_size]
                similarity = self._word_similarity(new_head, final_segment)

                if similarity >= similarity_threshold:
                    best_overlap = max(best_overlap, overlap_size)
                    break

            # Also check if new_head matches the END of final_tail
            if overlap_size <= search_window:
                final_end = final_tail_words[-overlap_size:]
                similarity = self._word_similarity(new_head, final_end)
                if similarity >= similarity_threshold:
                    best_overlap = max(best_overlap, overlap_size)

            if best_overlap > 0:
                break

        # Remove overlapping words
        if best_overlap > 0:
            deduplicated = " ".join(new_words[best_overlap:])
            logger.info(f"🔄 Removed ~{best_overlap} overlapping words (fuzzy match)")
            return deduplicated.strip()

        return new_text

    def _get_sentence_hash(self, text: str) -> str:
        """Generate hash for normalized text to detect exact duplicates."""
        # Normalize: lowercase, collapse whitespace
        normalized = " ".join(text.lower().split())
        return hashlib.md5(normalized.encode()).hexdigest()[:16]

    def _is_complete_sentence(self, text: str) -> bool:
        """Check if text ends with sentence-ending punctuation."""
        text = text.strip()
        # Check for common sentence endings in English/Hindi
        sentence_endings = (".", "!", "?", "。", "？", "！", "।")
        return text.endswith(sentence_endings)

    def _extract_new_words(self, text: str) -> str:
        """Remove words already seen in finalized transcripts."""
        words = text.split()
        new_words = []
        for word in words:
            word_lower = word.lower().strip(".,!?;:")
            if word_lower and word_lower not in self.finalized_words:
                new_words.append(word)
                self.finalized_words.add(word_lower)
        return " ".join(new_words)

    def _get_ngrams(self, text: str, n: int = 4) -> set:
        """Get n-grams (word sequences) from text for similarity matching."""
        words = text.lower().split()
        if len(words) < n:
            return {" ".join(words)} if words else set()
        return {" ".join(words[i : i + n]) for i in range(len(words) - n + 1)}

    def _is_near_duplicate(self, text: str, threshold: float = 0.5) -> bool:
        """Check if text is a near-duplicate of already finalized content.

        Uses n-gram matching to catch phrases like 'we can jump on the call'
        that appear in both old and new text even if exact hash differs.
        """
        if not self.last_final_text or len(text.split()) < 5:
            return False

        # Get 3-grams from new text (changed from 4 for finer detection)
        new_ngrams = self._get_ngrams(text, n=3)
        if not new_ngrams:
            return False

        # Get 3-grams from last portion of finalized text
        final_words = self.last_final_text.split()
        # Only check last ~100 words for performance
        recent_final = (
            " ".join(final_words[-100:])
            if len(final_words) > 100
            else self.last_final_text
        )
        final_ngrams = self._get_ngrams(recent_final, n=3)

        if not final_ngrams:
            return False

        # Calculate overlap ratio
        overlap = len(new_ngrams & final_ngrams)
        overlap_ratio = overlap / len(new_ngrams)

        if overlap_ratio >= threshold:
            logger.debug(
                f"⏭️  Near-duplicate detected ({overlap_ratio:.0%} overlap): '{text[:50]}...'"
            )
            return True

        return False

    def _is_hallucination(self, text: str) -> bool:
        """Check for common Whisper hallucinations."""
        text = text.strip().lower()
        hallucinations = {
            "you",
            "thank you.",
            "thanks for watching",
            "watching",
            "subtitles by",
            "amara.org",
            "mbc",
            "foreign",
            "so machen wir government",
            "so machen wir",
            "sous-titrage",
            "copyright",
            "all rights reserved",
        }

        # Exact match or starts with hallucination
        if text in hallucinations:
            return True

        # Check for "foreign" or repeated "you you"
        if text == "foreign" or text == "foreign.":
            return True

        # Check for specific German hallucination seen in logs
        if "so machen wir" in text or "government gestolken" in text:
            return True

        return False

    async def _handle_transcript(
        self,
        text: str,
        confidence: float,
        on_partial: Optional[Callable],
        on_final: Optional[Callable],
        metadata: Optional[dict] = None,
    ):
        """Handle partial vs final transcript logic with improved deduplication.

        QUALITY IMPROVEMENTS:
        - Hash-based duplicate detection
        - Sentence boundary detection
        - Debounced final emissions
        """

        # Skip empty or very short text
        if not text or len(text.strip()) < 2:
            return

        # HALLUCINATION FILTER
        if self._is_hallucination(text):
            logger.info(f"👻 Filtered hallucination: '{text}'")
            return

        # IMPROVED: Remove overlapping words from start
        deduplicated_text = self._remove_overlap(text)

        # Skip if nothing left after deduplication
        if not deduplicated_text or len(deduplicated_text.strip()) < 3:
            logger.debug(f"⏭️  Skipped - fully overlapping: '{text[:50]}...'")
            return

        # Use deduplicated text for processing
        text = deduplicated_text

        # IMPROVED: Check if this exact text was already finalized (hash check)
        sentence_hash = self._get_sentence_hash(text)
        if sentence_hash in self.finalized_hashes:
            logger.debug(f"⏭️  Skipping duplicate hash: '{text[:50]}...'")
            return

        # IMPROVED: Check for near-duplicates using n-gram matching
        if self._is_near_duplicate(
            text, threshold=0.35
        ):  # Lowered from 0.4 to catch more
            # 40% of 4-grams match = likely duplicate
            return

        # Check if text stabilized
        if text == self.last_partial_text:
            self.same_text_count += 1
        else:
            self.same_text_count = 0
            self.last_partial_text = text

        # Emit partial
        # REMOVED: User requested no partial transcription
        # if on_partial:
        #     await on_partial(
        #         {
        #             "text": text,
        #             "confidence": confidence,
        #             "is_stable": self.same_text_count >= 2,
        #         }
        #     )

        # SMART TIMER TRIGGER LOGIC
        is_complete_sentence = self._is_complete_sentence(text)

        # Calculate speech duration (using client-synced timestamps)
        speech_duration_ms = (
            (self.speech_end_time - self.speech_start_time) * 1000
            if self.speech_start_time > 0
            else 0
        )

        # Determine trigger reason
        trigger_reason = None

        # Trigger 1: Punctuation + 3s buffer
        if (
            is_complete_sentence
            and speech_duration_ms >= self.punctuation_min_duration_ms
        ):
            trigger_reason = "punctuation"
            logger.info(
                f"⏱️ SMART TRIGGER: Punctuation + {speech_duration_ms:.0f}ms speech"
            )

        # Trigger 2: Max buffer timeout (12s)
        elif speech_duration_ms >= self.max_buffer_duration_ms:
            trigger_reason = "timeout"
            logger.info(
                f"⏱️ SMART TRIGGER: Max timeout ({speech_duration_ms:.0f}ms >= {self.max_buffer_duration_ms}ms)"
            )

        # Trigger 3: Stability (text unchanged 4+ times)
        elif self.same_text_count >= 4:
            trigger_reason = "stability"
            logger.info(
                f"⏱️ SMART TRIGGER: Text stable ({self.same_text_count} repeats)"
            )

        # Trigger 4: Complete sentence + stable (2+ repeats)
        elif self.same_text_count >= 2 and is_complete_sentence:
            trigger_reason = "sentence_complete"
            logger.info(f"⏱️ SMART TRIGGER: Sentence complete + stable")

        if trigger_reason:
            # Debounce - check hash before emitting
            if sentence_hash in self.finalized_hashes:
                logger.debug(f"⏭️  Debounced duplicate final: '{text[:50]}...'")
                return

            if on_final:
                logger.debug(
                    f"✅ Finalizing: '{text[:50]}...' (reason={trigger_reason})"
                )

                # Calculate timing (using accurate client timestamps)
                audio_start = self.speech_start_time
                audio_end = self.speech_end_time
                duration = audio_end - audio_start

                final_data = {
                    "text": text,
                    "confidence": confidence,
                    "reason": trigger_reason,
                    "audio_start_time": audio_start,
                    "audio_end_time": audio_end,
                    "duration": duration,
                }

                # Include translation metadata if available
                if metadata:
                    if metadata.get("original_text"):
                        final_data["original_text"] = metadata["original_text"]
                    if metadata.get("translated"):
                        final_data["translated"] = metadata["translated"]

                await on_final(final_data)

                # Track finalized text
                self.finalized_hashes.add(sentence_hash)
                self.last_final_text += " " + text
                self.last_partial_text = ""
                self.same_text_count = 0
                # Reset for next segment (continue from where we left off)
                self.speech_start_time = self.speech_end_time

    def get_stats(self) -> dict:
        """Get performance statistics"""
        return {
            "chunks_processed": self.total_chunks_processed,
            "transcriptions": self.total_transcriptions,
            "buffer_duration_ms": self.buffer.get_buffer_duration_ms(),
            "is_speaking": self.is_speaking,
            "final_text_length": len(self.last_final_text),
            "partial_text": self.last_partial_text,
        }

    def reset(self):
        """Reset manager state for new recording"""
        self.buffer.clear()
        self.last_partial_text = ""
        self.last_final_text = ""
        self.silence_duration_ms = 0
        self.same_text_count = 0
        self.is_speaking = False
        self.total_chunks_processed = 0
        self.total_transcriptions = 0
        # Clear deduplication tracking
        self.finalized_hashes.clear()
        self.finalized_words.clear()
        logger.info("🔄 Manager reset")

    def cleanup(self):
        """Cleanup resources"""
        self.executor.shutdown(wait=False)
        logger.info("🧹 Manager cleanup complete")

    async def force_flush(self):
        """
        Flush any pending audio in the buffer and finalize partial transcripts.
        Called on disconnect or manual stop.
        """
        logger.info("🚨 Force flush triggered")

        # Get remaining audio
        remaining_bytes = self.buffer.get_all_samples_bytes()

        if len(remaining_bytes) > 16000:  # At least 0.5s of audio
            logger.info(f"Flushing {len(remaining_bytes)} bytes of remaining audio")

            # Transcribe final chunk
            loop = asyncio.get_event_loop()
            result = await loop.run_in_executor(
                None,  # Use default executor
                self.groq.transcribe_audio_sync,
                remaining_bytes,
                "auto",
                self.last_final_text[-100:] if self.last_final_text else None,
                True,
            )

            if result["text"]:
                logger.info(f"✅ Flushed final segment: '{result['text'][:50]}...'")
                # Emit as final
                return {
                    "text": result["text"],
                    "confidence": result.get("confidence", 1.0),
                    "is_flush": True,
                }

        return None
