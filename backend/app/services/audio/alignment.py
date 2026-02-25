"""
Alignment Engine for Speaker Diarization.

3-Tier Alignment Strategy:
1. Time Overlap (Primary) - Assign speaker based on time overlap
2. Word Density (Secondary) - Count words in speaker windows
3. Uncertain Fallback - Explicit UNCERTAIN state when confidence is low

This approach handles:
- Whisper timestamp drift
- Speaker changes mid-sentence
- Overlapping speech
- Low-confidence scenarios
"""

from dataclasses import dataclass
from typing import List, Dict, Tuple
import logging
from intervaltree import IntervalTree

logger = logging.getLogger(__name__)


@dataclass
class AlignmentResult:
    """Result of aligning a transcript segment to a speaker."""
    speaker: str
    confidence: float  # 0.0 to 1.0
    method: str        # 'time_overlap' | 'word_density' | 'uncertain'
    state: str         # 'CONFIDENT' | 'UNCERTAIN' | 'OVERLAP' | 'UNKNOWN_SPEAKER'


class AlignmentEngine:
    """
    3-Tier alignment strategy for matching transcript text to speaker labels.

    Tier 1: Time overlap - Primary method, checks time intersection
    Tier 2: Word density - Fallback for timestamp drift, counts words in windows
    Tier 3: Uncertain - Explicit state when confidence is too low

    Philosophy: "Silence is better than wrong attribution"
    """

    # Configuration thresholds
    CONFIDENCE_THRESHOLD = 0.6      # Minimum confidence to be CONFIDENT
    OVERLAP_THRESHOLD = 0.5         # 50% time overlap required for Tier 1
    MULTI_SPEAKER_THRESHOLD = 0.3   # If 2+ speakers > 30% overlap, mark as OVERLAP
    WORD_DENSITY_THRESHOLD = 0.7    # 70% of words in speaker window = CONFIDENT

    def align_segment(
        self,
        text: str,
        start_time: float,
        end_time: float,
        speaker_segments: List[Dict]
    ) -> AlignmentResult:
        """
        Align a single transcript segment to speaker labels using 3-tier strategy.

        Args:
            text: Transcript text to align
            start_time: Segment start time (seconds)
            end_time: Segment end time (seconds)
            speaker_segments: List of dicts with keys:
                             {speaker, start_time, end_time, text, confidence}

        Returns:
            AlignmentResult with speaker, confidence, method, and state
        """
        if not speaker_segments:
            return AlignmentResult(
                speaker="Unknown",
                confidence=0.0,
                method="no_speakers",
                state="UNKNOWN_SPEAKER"
            )

        # Tier 1: Time Overlap (Primary)
        time_result = self._align_by_time_overlap(start_time, end_time, speaker_segments)
        if time_result.confidence >= self.CONFIDENCE_THRESHOLD:
            return time_result

        # Tier 2: Word Density (Secondary - handles timestamp drift)
        if text and len(text.split()) > 2:  # Only if we have meaningful text
            density_result = self._align_by_word_density(
                text, start_time, end_time, speaker_segments
            )
            if density_result.confidence >= self.CONFIDENCE_THRESHOLD:
                return density_result

        # Tier 3: Uncertain Fallback
        # Choose the better of time vs density, but mark as UNCERTAIN
        best_result = time_result
        if text and len(text.split()) > 2:
            density_result = self._align_by_word_density(
                text, start_time, end_time, speaker_segments
            )
            if density_result.confidence > time_result.confidence:
                best_result = density_result

        return AlignmentResult(
            speaker=best_result.speaker if best_result.speaker != "Unknown" else "Unknown",
            confidence=best_result.confidence,
            method="uncertain",
            state="UNCERTAIN"
        )

    def _align_by_time_overlap(
        self,
        start: float,
        end: float,
        speaker_segments: List[Dict]
    ) -> AlignmentResult:
        """
        Tier 1: Calculate time overlap with each speaker segment.

        Returns CONFIDENT if overlap ratio > threshold, otherwise lower confidence.
        Detects OVERLAP state if multiple speakers are speaking simultaneously.
        """
        segment_duration = end - start
        if segment_duration <= 0:
            return AlignmentResult("Unknown", 0.0, "time_overlap", "UNCERTAIN")

        speaker_overlaps = {}
        total_overlap = 0.0

        # Calculate overlap with each speaker
        for seg in speaker_segments:
            overlap_start = max(start, seg['start_time'])
            overlap_end = min(end, seg['end_time'])

            if overlap_end > overlap_start:
                overlap_duration = overlap_end - overlap_start
                speaker_overlaps[seg['speaker']] = overlap_duration
                total_overlap += overlap_duration

        if not speaker_overlaps:
            return AlignmentResult("Unknown", 0.0, "time_overlap", "UNCERTAIN")

        # Find best speaker
        best_speaker = max(speaker_overlaps, key=speaker_overlaps.get)
        best_overlap = speaker_overlaps[best_speaker]

        # Calculate confidence based on overlap ratio
        overlap_ratio = best_overlap / segment_duration
        confidence = min(overlap_ratio / self.OVERLAP_THRESHOLD, 1.0)

        # Check for overlapping speakers (multiple speakers speaking)
        speakers_with_significant_overlap = [
            s for s, o in speaker_overlaps.items()
            if o > self.MULTI_SPEAKER_THRESHOLD * segment_duration
        ]

        if len(speakers_with_significant_overlap) > 1:
            state = "OVERLAP"
            logger.debug(
                f"⚠️  Multiple speakers detected: {speakers_with_significant_overlap} "
                f"at {start:.1f}s-{end:.1f}s"
            )
        else:
            state = "CONFIDENT" if confidence >= self.CONFIDENCE_THRESHOLD else "UNCERTAIN"

        logger.debug(
            f"Time alignment: {best_speaker} ({overlap_ratio:.2%} overlap, "
            f"confidence={confidence:.2f}, state={state})"
        )

        return AlignmentResult(best_speaker, confidence, "time_overlap", state)

    def _align_by_word_density(
        self,
        text: str,
        start: float,
        end: float,
        speaker_segments: List[Dict]
    ) -> AlignmentResult:
        """
        Tier 2: Count words inside each speaker's time window.

        This handles cases where Whisper timestamps drift, but the bulk of
        words still fall within a speaker's segment.

        Uses uniform word distribution as approximation (good enough for alignment).
        """
        words = text.split()
        if len(words) == 0:
            return AlignmentResult("Unknown", 0.0, "word_density", "UNCERTAIN")

        # Estimate word timing (uniform distribution for simplicity)
        duration = end - start
        if duration <= 0:
            duration = 0.1  # Prevent division by zero

        word_duration = duration / len(words)
        speaker_word_counts = {}

        # Assign each word to speaker(s)
        for i, word in enumerate(words):
            word_start = start + i * word_duration
            word_end = word_start + word_duration
            word_mid = (word_start + word_end) / 2

            # Find which speaker(s) this word overlaps with
            for seg in speaker_segments:
                if seg['start_time'] <= word_mid <= seg['end_time']:
                    speaker = seg['speaker']
                    speaker_word_counts[speaker] = speaker_word_counts.get(speaker, 0) + 1

        if not speaker_word_counts:
            return AlignmentResult("Unknown", 0.0, "word_density", "UNCERTAIN")

        # Find speaker with most words
        best_speaker = max(speaker_word_counts, key=speaker_word_counts.get)
        words_in_speaker = speaker_word_counts[best_speaker]

        # Confidence based on percentage of words in best speaker's window
        confidence = words_in_speaker / len(words)
        state = "CONFIDENT" if confidence >= self.WORD_DENSITY_THRESHOLD else "UNCERTAIN"

        logger.debug(
            f"Word density alignment: {best_speaker} "
            f"({words_in_speaker}/{len(words)} words, confidence={confidence:.2f})"
        )

        return AlignmentResult(best_speaker, confidence, "word_density", state)

    def align_batch(
        self,
        transcripts: List[Dict],
        speaker_segments: List[Dict]
    ) -> Tuple[List[Dict], Dict]:
        """
        Align a batch of transcripts and return metrics.

        Args:
            transcripts: List of transcript dicts with 'text', 'start_time', 'end_time'
            speaker_segments: List of speaker segments from diarization

        Returns:
            Tuple of (aligned_transcripts, metrics)
        """
        aligned = []
        metrics = {
            'total_segments': len(transcripts),
            'confident_count': 0,
            'uncertain_count': 0,
            'overlap_count': 0,
            'unknown_count': 0,
            'avg_confidence': 0.0,
            'method_breakdown': {}
        }

        total_confidence = 0.0

        # Optimization: Build IntervalTree for O(N log M) fast overlap lookups
        tree = IntervalTree()
        for seg in speaker_segments:
            s_start = seg.get('start_time', 0.0)
            s_end = seg.get('end_time', s_start)
            # IntervalTree requires strict start < end
            if s_start >= s_end:
                s_end = s_start + 0.001
            tree.addi(s_start, s_end, seg)

        for transcript in transcripts:
            # Extract timing (handle different field names)
            start = transcript.get('audio_start_time', transcript.get('start', 0))
            end = transcript.get('audio_end_time', transcript.get('end', start + 2))
            text = transcript.get('text', transcript.get('transcript', ''))

            # Fast interval query: find only speaker segments that overlap this transcript's time
            query_end = end if end > start else start + 0.001
            overlapping_intervals = tree.overlap(start, query_end)
            relevant_speaker_segments = [interval.data for interval in overlapping_intervals]

            # Run alignment with the tiny subset of relevant segments (O(1) instead of O(M))
            result = self.align_segment(text, start, end, relevant_speaker_segments)

            # Track metrics
            metrics['method_breakdown'][result.method] = \
                metrics['method_breakdown'].get(result.method, 0) + 1

            if result.state == 'CONFIDENT':
                metrics['confident_count'] += 1
            elif result.state == 'UNCERTAIN':
                metrics['uncertain_count'] += 1
            elif result.state == 'OVERLAP':
                metrics['overlap_count'] += 1
            elif result.state == 'UNKNOWN_SPEAKER':
                metrics['unknown_count'] += 1

            total_confidence += result.confidence

            # Add to aligned transcripts
            aligned.append({
                **transcript,
                'speaker': result.speaker,
                'speaker_confidence': result.confidence,
                'alignment_method': result.method,
                'alignment_state': result.state
            })

        # Finalize metrics
        metrics['avg_confidence'] = total_confidence / len(transcripts) if transcripts else 0.0

        logger.info(
            f"📊 Alignment complete: {metrics['confident_count']}/{metrics['total_segments']} confident, "
            f"{metrics['uncertain_count']} uncertain, {metrics['overlap_count']} overlap, "
            f"{metrics['unknown_count']} unknown (avg conf={metrics['avg_confidence']:.2f})"
        )

        return aligned, metrics
