'use client';

import { Transcript } from '@/types';
import { useEffect, useRef, useState } from 'react';
import { ConfidenceIndicator } from './ConfidenceIndicator';
import { Tooltip, TooltipContent, TooltipTrigger } from './ui/tooltip';
import { RecordingStatusBar } from './RecordingStatusBar';
import { motion, AnimatePresence } from 'framer-motion';

// Helper to generate consistent colors for speakers
const SPEAKER_COLORS = [
  'text-rose-600 font-bold', 'text-blue-600 font-bold', 'text-green-600 font-bold',
  'text-orange-600 font-bold', 'text-purple-600 font-bold', 'text-cyan-600 font-bold',
  'text-indigo-600 font-bold', 'text-pink-600 font-bold'
];

function getSpeakerColor(speakerLabel: string): string {
  if (!speakerLabel) return 'text-gray-600 font-bold';
  // Simple consistent hash
  let hash = 0;
  for (let i = 0; i < speakerLabel.length; i++) {
    hash = speakerLabel.charCodeAt(i) + ((hash << 5) - hash);
  }
  const index = Math.abs(hash) % SPEAKER_COLORS.length;
  return SPEAKER_COLORS[index];
}

interface SpeakerMap {
  [label: string]: string;
}

interface TranscriptViewProps {
  transcripts: Transcript[];
  isRecording?: boolean;
  isPaused?: boolean; // Is recording paused (affects UI indicators)
  activeDuration?: number; // Total active duration in seconds
  isProcessing?: boolean; // Is processing/finalizing transcription (hides "Listening..." indicator)
  isStopping?: boolean; // Is recording being stopped (provides immediate UI feedback)
  enableStreaming?: boolean; // Enable streaming effect for live transcription UX
  speakerMap?: SpeakerMap; // NEW
  forceShowSpeakers?: boolean; // NEW: Override to always show speakers (e.g. if diarization is complete)
}

// Helper function to format seconds as recording-relative time [MM:SS]
function formatRecordingTime(seconds: number | undefined): string {
  if (seconds === undefined) return '[00:00]';

  const totalSeconds = Math.floor(seconds);
  const minutes = Math.floor(totalSeconds / 60);
  const secs = totalSeconds % 60;

  return `[${minutes.toString().padStart(2, '0')}:${secs.toString().padStart(2, '0')}]`;
}

// Helper to format timestamp to IST [HH:MM]
function formatISTTime(timestamp: string | undefined): string {
  if (!timestamp) return '[--:--]';
  try {
    // If timestamp is already formatted like HH:MM:SS (from backend/frontend during live), parse it
    // But typically transcript.timestamp is an ISO string or similar.
    // If it is just a time string, we might need to be careful.
    // Assuming ISO string or Date string.

    // Create date object (handling UTC input)
    // If it's a simple time string like "14:30:00", we might need to prepend a date.
    // But usually from DB/WebSocket it is full ISO.

    // Check if it looks like a time string (HH:MM:SS)
    if (/^\d{2}:\d{2}:\d{2}$/.test(timestamp)) {
      // It's already time, just return HH:MM
      return `[${timestamp.substring(0, 5)}]`;
    }

    const date = new Date(timestamp);
    if (isNaN(date.getTime())) return '[--:--]';

    return `[${date.toLocaleTimeString('en-IN', {
      timeZone: 'Asia/Kolkata',
      hour: '2-digit',
      minute: '2-digit',
      hour12: false
    })}]`;
  } catch (e) {
    return '[--:--]';
  }
}

// Helper function to remove consecutive word repetitions (especially short words ≤2 letters)
function cleanRepetitions(text: string): string {
  if (!text || text.trim().length === 0) return text || '';

  const words = text.split(/\s+/);
  const cleanedWords: string[] = [];

  let i = 0;
  while (i < words.length) {
    const currentWord = words[i];
    const currentWordLower = currentWord.toLowerCase();

    // Count consecutive repetitions of the same word
    let repeatCount = 1;
    while (
      i + repeatCount < words.length &&
      words[i + repeatCount].toLowerCase() === currentWordLower
    ) {
      repeatCount++;
    }

    // For short words (≤2 letters), be aggressive: if repeated 2+ times, keep only 1
    // For longer words, keep 1 if repeated 3+ times (less aggressive)
    if (currentWord.length <= 2) {
      // Short words: "I I I I" → "I", "Tu Tu Tu" → "Tu"
      if (repeatCount >= 2) {
        cleanedWords.push(currentWord);
        i += repeatCount;
      } else {
        cleanedWords.push(currentWord);
        i += 1;
      }
    } else {
      // Longer words: keep original unless heavily repeated
      if (repeatCount >= 3) {
        cleanedWords.push(currentWord);
        i += repeatCount;
      } else {
        cleanedWords.push(currentWord);
        i += 1;
      }
    }
  }

  return cleanedWords.join(' ');
}

// Helper function to remove filler words and stop words from transcripts
function cleanStopWords(text: string): string {
  try {
    // Safety check: ensure input is string
    if (typeof text !== 'string') return '';

    // FIRST: Clean repetitions (especially short words)
    let cleanedText = cleanRepetitions(text);

    // Safety check: ensure result is string
    if (typeof cleanedText !== 'string') cleanedText = '';

    // THEN: Remove filler words
    const stopWords = [
      'uh', 'um', 'er', 'ah', 'hmm', 'hm', 'eh', 'oh',
      // 'like', 'you know', 'i mean', 'sort of', 'kind of',
      // 'basically', 'actually', 'literally', 'right',
      // 'thank you', 'thanks'
    ];

    // Remove each stop word (case-insensitive, with word boundaries)
    stopWords.forEach(word => {
      // Extra safety check before replace
      if (typeof cleanedText !== 'string') return;

      // Match the stop word at word boundaries, with optional punctuation
      const pattern = new RegExp(`\\b${word}\\b[,\\s]*`, 'gi');
      cleanedText = cleanedText.replace(pattern, ' ');
    });

    // Clean up extra whitespace and trim
    if (typeof cleanedText === 'string') {
      return cleanedText.replace(/\s+/g, ' ').trim();
    }
    return '';
  } catch (e) {
    console.error('Error in cleanStopWords:', e);
    return '';
  }
}

function getSegmentStyle(state?: string, source?: string): string {
  // If no state provided but it's diarized, assume confident (legacy support)
  if (!state && source === 'diarized') return 'border-l-4 border-green-500 pl-3';

  switch (state) {
    case 'CONFIDENT':
      return 'border-l-4 border-green-500 pl-3';
    case 'UNCERTAIN':
      return 'border-l-4 border-yellow-500 bg-yellow-50 pl-3';
    case 'OVERLAP':
      return 'border-l-4 border-orange-500 bg-orange-50 pl-3';
    default:
      return '';
  }
}

export const TranscriptView: React.FC<TranscriptViewProps> = ({
  transcripts,
  isRecording = false,
  isPaused = false,
  activeDuration,
  isProcessing = false,
  isStopping = false,
  enableStreaming = false,
  speakerMap = {},
  forceShowSpeakers = false
}) => {
  // Debug: Log the props to understand what's happening
  // console.log('TranscriptView render:', {
  //   isRecording,
  //   isPaused,
  //   isProcessing,
  //   isStopping,
  //   transcriptCount: transcripts.length,
  //   shouldShowListening: !isStopping && isRecording && !isPaused && !isProcessing && transcripts.length > 0
  // });

  // Streaming effect state
  const [streamingTranscript, setStreamingTranscript] = useState<{
    id: string;
    visibleText: string;
    fullText: string;
  } | null>(null);
  const streamingIntervalRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const lastStreamedIdRef = useRef<string | null>(null); // Track which transcript we've streamed

  // Load preference for showing confidence indicator
  const [showConfidence, setShowConfidence] = useState<boolean>(() => {
    if (typeof window !== 'undefined') {
      const saved = localStorage.getItem('showConfidenceIndicator');
      return saved !== null ? saved === 'true' : true; // Default to true
    }
    return true;
  });

  // Listen for preference changes from settings
  useEffect(() => {
    const handleConfidenceChange = (e: Event) => {
      const customEvent = e as CustomEvent<boolean>;
      setShowConfidence(customEvent.detail);
    };

    window.addEventListener('confidenceIndicatorChanged', handleConfidenceChange);
    return () => window.removeEventListener('confidenceIndicatorChanged', handleConfidenceChange);
  }, []);

  // Streaming effect: animate new transcripts character-by-character
  useEffect(() => {
    if (!enableStreaming || !isRecording) {
      if (streamingIntervalRef.current) {
        clearInterval(streamingIntervalRef.current);
        streamingIntervalRef.current = null;
      }
      setStreamingTranscript(null);
      lastStreamedIdRef.current = null;
      return;
    }

    const latestTranscript = transcripts.slice(-1)[0];

    if (!latestTranscript) return;

    if (lastStreamedIdRef.current !== latestTranscript.id) {
      if (streamingIntervalRef.current) {
        clearInterval(streamingIntervalRef.current);
        streamingIntervalRef.current = null;
      }

      lastStreamedIdRef.current = latestTranscript.id;
      const fullText = latestTranscript.text;

      const TOTAL_DURATION_MS = 800;
      const INTERVAL_MS = 15;
      const totalTicks = TOTAL_DURATION_MS / INTERVAL_MS;
      const charsPerTick = Math.max(2, Math.ceil(fullText.length / totalTicks));
      const INITIAL_CHARS = Math.min(5, fullText.length);
      let charIndex = INITIAL_CHARS;

      setStreamingTranscript({
        id: latestTranscript.id,
        visibleText: fullText.substring(0, INITIAL_CHARS),
        fullText: fullText
      });

      streamingIntervalRef.current = setInterval(() => {
        charIndex += charsPerTick;

        if (charIndex >= fullText.length) {
          clearInterval(streamingIntervalRef.current!);
          streamingIntervalRef.current = null;
          setStreamingTranscript(null);
        } else {
          setStreamingTranscript(prev => {
            if (!prev) return null;
            return {
              ...prev,
              visibleText: fullText.substring(0, charIndex)
            };
          });
        }
      }, INTERVAL_MS);
    }
  }, [transcripts, enableStreaming, isRecording]);

  // Cleanup streaming interval on unmount
  useEffect(() => {
    return () => {
      if (streamingIntervalRef.current) {
        clearInterval(streamingIntervalRef.current);
        streamingIntervalRef.current = null;
      }
      lastStreamedIdRef.current = null;
    };
  }, []);

  return (
    <div className="px-4 py-2">
      {/* Recording Status Bar - Sticky at top, always visible when recording */}
      <AnimatePresence>
        {isRecording && (
          <div className="sticky top-4 z-10 bg-white pb-2">
            <RecordingStatusBar
              isPaused={isPaused}
              isRecording={isRecording}
              activeDuration={activeDuration}
            />
          </div>
        )}
      </AnimatePresence>

      {transcripts?.map((transcript, index) => {
        // Check if speaker changed from previous transcript
        const prevTranscript = index > 0 ? transcripts[index - 1] : null;

        // RESOLVE SPEAKER: Default to "Speaker 0" if missing (matches Copy behavior)
        const resolvedSpeaker = transcript.speaker || 'Speaker 0';
        const prevResolvedSpeaker = prevTranscript ? (prevTranscript.speaker || 'Speaker 0') : null;

        // RESOLVE SOURCE: Default to 'live' if missing (safe fallback)
        const source = transcript.source || 'live';
        const isLive = source === 'live';

        let showSpeaker = false;
        if (isLive && !forceShowSpeakers) {
          // LIVE: Always hide speaker labels as per user request
          showSpeaker = false;
        } else {
          // DIARIZED (or Forced): Always show the label
          showSpeaker = true;
        }

        // DEDUPLICATION: Only show if it changed from the previous segment
        // Skip deduplication if:
        // 1. Forced by parent prop (forceShowSpeakers) - e.g. Diarization Completed view
        // 2. OR it is explicitly a diarized segment (source != 'live') - User wants to see every label to match Copy format
        const shouldDeduplicate = !forceShowSpeakers && isLive;

        if (index > 0 && prevResolvedSpeaker === resolvedSpeaker && shouldDeduplicate) {
          showSpeaker = false;
        }

        const speakerName = speakerMap[resolvedSpeaker] || resolvedSpeaker;

        // SAFE GUARD: Ensure both sides are defined and valid strings before comparing
        const isStreaming = !!streamingTranscript &&
          !!transcript.id &&
          streamingTranscript.id === transcript.id;

        const textToShow = isStreaming ? streamingTranscript.visibleText : transcript.text;
        const filteredText = cleanStopWords(textToShow);
        const originalText = transcript.text || ''; // Default to empty string if undefined
        const originalWasEmpty = originalText.trim() === '';
        const displayText = originalWasEmpty && !isStreaming ? '[Silence]' : filteredText;

        const segmentStyle = getSegmentStyle(transcript.alignment_state, transcript.source);

        return (
          <motion.div
            key={transcript.id ? `${transcript.id}-${index}` : `transcript-${index}`}
            initial={{ opacity: 0, y: 5 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ duration: 0.15 }}
            className={`mb-3 ${segmentStyle}`}
          >
            <div className="flex items-start gap-2">
              <Tooltip>
                <TooltipTrigger>
                  <span className="text-xs text-gray-400 mt-1 flex-shrink-0 min-w-[50px]">
                    {isRecording
                      ? formatISTTime(transcript.timestamp)
                      : (transcript.audio_start_time != null
                        ? formatRecordingTime(transcript.audio_start_time)
                        : formatISTTime(transcript.timestamp))}
                  </span>
                </TooltipTrigger>
                <TooltipContent>
                  {transcript.duration != null && (
                    <span className="text-xs text-gray-400">
                      {transcript.duration.toFixed(1)}s
                      {transcript.confidence != null && (
                        <ConfidenceIndicator
                          confidence={transcript.confidence}
                          showIndicator={showConfidence}
                        />
                      )}
                      {transcript.alignment_state === 'UNCERTAIN' && (
                        <span className="ml-2 text-yellow-600 font-bold" title="Low confidence alignment">⚠️ Uncertain</span>
                      )}
                      {transcript.alignment_state === 'OVERLAP' && (
                        <span className="ml-2 text-orange-600 font-bold" title="Multiple speakers detected">👥 Overlap</span>
                      )}
                    </span>
                  )}
                </TooltipContent>
              </Tooltip>
              <div className="flex-1">
                {isStreaming ? (
                  // Streaming transcript - show in bubble (full width)
                  <div className="bg-gray-100 border border-gray-200 rounded-lg px-3 py-2">
                    <p className="text-base text-gray-800 leading-relaxed">
                      {displayText}
                    </p>
                  </div>
                ) : (
                  // Regular transcript - direct text for easy copy-paste
                  <div className="text-base text-gray-800 leading-relaxed">
                    {showSpeaker && (
                      <span className={`font-semibold mr-1 ${getSpeakerColor(resolvedSpeaker)}`}>
                        {speakerName}:
                      </span>
                    )}
                    <span>{displayText}</span>
                  </div>
                )}
              </div>
            </div>
          </motion.div>
        );
      })}

      {/* Show listening indicator when recording and has transcripts */}
      {!isStopping && isRecording && !isPaused && !isProcessing && transcripts.length > 0 && (
        <motion.div
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          className="flex items-center gap-2 mt-4 text-gray-500"
        >
          <div className="w-2 h-2 bg-blue-500 rounded-full animate-pulse"></div>
          <span className="text-sm">Listening...</span>
        </motion.div>
      )}

      {/* Empty state when no transcripts */}
      {transcripts.length === 0 && (
        <motion.div
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          className="text-center text-gray-500 mt-8"
        >
          {isRecording ? (
            <>
              <div className="flex items-center justify-center mb-3">
                <div className={`w-3 h-3 rounded-full ${isPaused ? 'bg-orange-500' : 'bg-blue-500 animate-pulse'}`}></div>
              </div>
              <p className="text-sm text-gray-600">
                {isPaused ? 'Recording paused' : 'Listening for speech...'}
              </p>
              <p className="text-xs mt-1 text-gray-400">
                {isPaused
                  ? 'Click resume to continue recording'
                  : 'Speak to see live transcription'}
              </p>
            </>
          ) : (
            <>
              <p className="text-lg font-semibold">Welcome to Pnyx!</p>
              <p className="text-xs mt-1">Start recording to see live transcription</p>
            </>
          )}
        </motion.div>
      )}
    </div>
  );
};
