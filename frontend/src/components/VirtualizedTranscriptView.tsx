'use client';

import { useCallback, useRef, useReducer, startTransition, useEffect, useState, memo } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { useAutoScroll } from "@/hooks/useAutoScroll";
import { useTranscriptStreaming } from "@/hooks/useTranscriptStreaming";
import { ConfidenceIndicator } from "./ConfidenceIndicator";
import { Tooltip, TooltipContent, TooltipTrigger } from "./ui/tooltip";
import { RecordingStatusBar } from "./RecordingStatusBar";
import { motion, AnimatePresence } from "framer-motion";
import { TranscriptSegmentData, TranscriptSpeaker } from "@/types";
import { Pencil, Check, X } from "lucide-react";
import { ClearMinutesIcon } from "@/components/ClearMinutesLogo";

export interface VirtualizedTranscriptViewProps {
    /** Transcript segments to display */
    segments: TranscriptSegmentData[];
    /** Whether recording is in progress */
    isRecording?: boolean;
    /** Whether recording is paused */
    isPaused?: boolean;
    /** Whether processing/finalizing transcription */
    isProcessing?: boolean;
    /** Whether stopping */
    isStopping?: boolean;
    /** Enable streaming effect for latest segment */
    enableStreaming?: boolean;
    /** Show confidence indicators */
    showConfidence?: boolean;
    /** Completely disable auto-scroll behavior (for meeting details page) */
    disableAutoScroll?: boolean;
    /** Map of speaker_id -> { display_name, color } for diarization labels */
    speakerMap?: Record<string, { display_name: string; color: string }>;
    /** Called when the user renames a speaker inline */
    onRenameSpeaker?: (speakerId: string, newName: string) => void;

    // Pagination props (infinite scroll)
    hasMore?: boolean;
    isLoadingMore?: boolean;
    totalCount?: number;
    loadedCount?: number;
    onLoadMore?: () => void;
}

// Threshold for enabling virtualization (below this, use simple rendering)
const VIRTUALIZATION_THRESHOLD = 10;

// Helper function to format seconds as recording-relative time [MM:SS]
function formatRecordingTime(seconds: number | undefined): string {
    if (seconds === undefined) return '[--:--]';

    const totalSeconds = Math.floor(seconds);
    const minutes = Math.floor(totalSeconds / 60);
    const secs = totalSeconds % 60;

    return `[${minutes.toString().padStart(2, '0')}:${secs.toString().padStart(2, '0')}]`;
}

// Helper function to remove filler words and repetitions
function cleanStopWords(text: string): string {
    const stopWords = ['uh', 'um', 'er', 'ah', 'hmm', 'hm', 'eh', 'oh'];

    let cleanedText = text;
    stopWords.forEach(word => {
        const pattern = new RegExp(`\\b${word}\\b[,\\s]*`, 'gi');
        cleanedText = cleanedText.replace(pattern, ' ');
    });

    return cleanedText.replace(/\s+/g, ' ').trim();
}

// Memoized transcript segment component
const TranscriptSegment = memo(function TranscriptSegment({
    id,
    timestamp,
    text,
    confidence,
    speaker,
    isStreaming,
    showConfidence,
    speakerDisplayName,
    speakerColor,
    onRenameSpeaker,
}: {
    id: string;
    timestamp: number;
    text: string;
    confidence?: number;
    speaker?: TranscriptSpeaker;
    isStreaming: boolean;
    showConfidence: boolean;
    speakerDisplayName?: string;
    speakerColor?: string;
    onRenameSpeaker?: (speakerId: string, newName: string) => void;
}) {
    const [isEditing, setIsEditing] = useState(false);
    const [editValue, setEditValue] = useState('');
    const inputRef = useRef<HTMLInputElement>(null);

    const displayText = cleanStopWords(text) || (text.trim() === '' ? '[Silence]' : text);
    const deviceLabel = speaker === 'mic' ? 'Speaker 1 (You)' : speaker === 'system' ? 'System' : null;
    const isDiarizationSpeaker = typeof speaker === 'string' && speaker.startsWith('speaker_');
    const rawFallback = isDiarizationSpeaker
        ? speaker.replace('speaker_', 'Speaker ')
        : deviceLabel;
    const speakerLabel = speakerDisplayName || rawFallback;
    const labelColor = (isDiarizationSpeaker || speaker === 'mic') ? (speakerColor || '#6366f1') : undefined;
    const canRename = !!onRenameSpeaker && (speaker === 'mic' || isDiarizationSpeaker);

    const startEdit = () => {
        setEditValue(speakerLabel || '');
        setIsEditing(true);
        setTimeout(() => inputRef.current?.focus(), 0);
    };

    const commitEdit = () => {
        const trimmed = editValue.trim();
        if (trimmed && trimmed !== speakerLabel && speaker) {
            onRenameSpeaker?.(speaker, trimmed);
        }
        setIsEditing(false);
    };

    const cancelEdit = () => setIsEditing(false);

    return (
        <div id={`segment-${id}`} className="mb-3">
            <div className="flex items-start gap-2">
                <Tooltip>
                    <TooltipTrigger>
                        <span className="text-xs text-gray-400 mt-1 flex-shrink-0 min-w-[50px]">
                            {formatRecordingTime(timestamp)}
                        </span>
                    </TooltipTrigger>
                    <TooltipContent>
                        {confidence !== undefined && showConfidence && (
                            <ConfidenceIndicator confidence={confidence} showIndicator={showConfidence} />
                        )}
                    </TooltipContent>
                </Tooltip>
                <div className="flex-1">
                    {speakerLabel && (
                        <div className="flex items-center gap-1 mb-1 group">
                            {isEditing ? (
                                <>
                                    <input
                                        ref={inputRef}
                                        value={editValue}
                                        onChange={e => setEditValue(e.target.value)}
                                        onKeyDown={e => { if (e.key === 'Enter') commitEdit(); if (e.key === 'Escape') cancelEdit(); }}
                                        className="text-[11px] font-semibold uppercase tracking-wide border-b border-gray-400 bg-transparent outline-none w-28"
                                        style={{ color: labelColor || '#6b7280' }}
                                    />
                                    <button onClick={commitEdit} className="text-green-500 hover:text-green-600">
                                        <Check className="w-3 h-3" />
                                    </button>
                                    <button onClick={cancelEdit} className="text-red-400 hover:text-red-500">
                                        <X className="w-3 h-3" />
                                    </button>
                                </>
                            ) : (
                                <>
                                    <span
                                        className="text-[11px] font-semibold uppercase tracking-wide"
                                        style={{ color: labelColor || '#6b7280' }}
                                    >
                                        {speakerLabel}
                                    </span>
                                    {canRename && (
                                        <button
                                            onClick={startEdit}
                                            className="opacity-0 group-hover:opacity-100 transition-opacity text-gray-400 hover:text-gray-600 ml-0.5"
                                            title="Rename speaker"
                                        >
                                            <Pencil className="w-2.5 h-2.5" />
                                        </button>
                                    )}
                                </>
                            )}
                        </div>
                    )}
                    {isStreaming ? (
                        <div className="bg-gray-100 dark:bg-secondary border border-gray-200 dark:border-border rounded-lg px-3 py-2">
                            <p className="text-base text-gray-800 leading-relaxed">{displayText}</p>
                        </div>
                    ) : (
                        <p className="text-base text-gray-800 leading-relaxed">{displayText}</p>
                    )}
                </div>
            </div>
        </div>
    );
});

export const VirtualizedTranscriptView: React.FC<VirtualizedTranscriptViewProps> = ({
    segments,
    isRecording = false,
    isPaused = false,
    isProcessing = false,
    isStopping = false,
    enableStreaming = false,
    showConfidence = true,
    disableAutoScroll = false,
    speakerMap,
    onRenameSpeaker,
    hasMore = false,
    isLoadingMore = false,
    totalCount = 0,
    loadedCount = 0,
    onLoadMore,
}) => {
    // Create scroll ref first - shared between virtualizer and auto-scroll hook
    const scrollRef = useRef<HTMLDivElement>(null);
    // Ref for infinite scroll trigger element
    const loadMoreTriggerRef = useRef<HTMLDivElement>(null);

    // Force re-render without flushSync (avoids React warning)
    const [, rerender] = useReducer((x: number) => x + 1, 0);

    // Setup virtualizer for efficient rendering of large lists
    const virtualizer = useVirtualizer({
        count: segments.length,
        getScrollElement: () => scrollRef.current,
        estimateSize: () => 60, // Estimated height per segment
        overscan: 10, // Render extra items above/below viewport
        onChange: () => {
            startTransition(() => {
                rerender();
            });
        },
    });

    // Custom hook for auto-scrolling (supports both virtualized and non-virtualized)
    useAutoScroll({
        scrollRef,
        segments,
        isRecording,
        isPaused,
        virtualizer,
        virtualizationThreshold: VIRTUALIZATION_THRESHOLD,
        disableAutoScroll,
    });

    // Streaming text effect hook (typewriter animation for new transcripts)
    const { streamingSegmentId, getDisplayText } = useTranscriptStreaming(
        segments,
        isRecording,
        enableStreaming
    );

    // Infinite scroll: IntersectionObserver to trigger loading more
    useEffect(() => {
        if (!onLoadMore || !hasMore || isLoadingMore || isRecording || segments.length === 0) {
            return;
        }

        const triggerElement = loadMoreTriggerRef.current;
        if (!triggerElement) return;

        const observer = new IntersectionObserver(
            (entries) => {
                if (entries[0].isIntersecting && hasMore && !isLoadingMore) {
                    onLoadMore();
                }
            },
            {
                root: null,
                rootMargin: '100px',
                threshold: 0,
            }
        );

        observer.observe(triggerElement);

        return () => observer.disconnect();
    }, [hasMore, isLoadingMore, onLoadMore, isRecording, segments.length]);

    // Scroll-based fallback for fast scrolling
    useEffect(() => {
        if (!onLoadMore || !hasMore || isLoadingMore || isRecording) return;

        const scrollElement = scrollRef.current;
        if (!scrollElement) return;

        let ticking = false;

        const handleScroll = () => {
            if (ticking || isLoadingMore || !hasMore) return;

            ticking = true;
            requestAnimationFrame(() => {
                const { scrollTop, scrollHeight, clientHeight } = scrollElement;
                const scrollBottom = scrollHeight - scrollTop - clientHeight;

                // Trigger load when within 200px of bottom
                if (scrollBottom < 200 && hasMore && !isLoadingMore) {
                    onLoadMore();
                }
                ticking = false;
            });
        };

        scrollElement.addEventListener('scroll', handleScroll, { passive: true });
        return () => scrollElement.removeEventListener('scroll', handleScroll);
    }, [onLoadMore, hasMore, isLoadingMore, isRecording]);

    // Use simple rendering for small lists, virtualization for large lists
    const useVirtualization = segments.length >= VIRTUALIZATION_THRESHOLD;

    // Empty state — suppressed when live transcription is off (TranscriptPanel shows its own notice)
    const emptyState = isRecording && !enableStreaming ? null : (
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
                        {isPaused ? 'Click resume to continue recording' : 'Speak to see live transcription'}
                    </p>
                </>
            ) : (
                <>
                    <div className="flex flex-col items-center gap-3 mb-3">
                        <ClearMinutesIcon size={72} />
                        <span
                            className="text-2xl font-extrabold tracking-tight text-gray-700 dark:text-foreground"
                            style={{ fontFamily: 'var(--font-syne, system-ui)', letterSpacing: '-0.03em' }}
                        >
                            clearminutes
                        </span>
                    </div>
                    <p className="text-sm mt-1 text-gray-400 dark:text-muted-foreground">Start recording to see live transcription</p>
                </>
            )}
        </motion.div>
    );

    return (
        <div ref={scrollRef} className="flex flex-col h-full overflow-y-auto px-4 py-2">
            {/* Recording Status Bar - Sticky at top, always visible when recording */}
            <AnimatePresence>
                {isRecording && (
                    <div className="sticky top-0 z-10 bg-white dark:bg-card pb-2">
                        <RecordingStatusBar isPaused={isPaused} />
                    </div>
                )}
            </AnimatePresence>

            {/* Content - add padding when recording to prevent overlap */}
            <div className={isRecording ? 'pt-2' : ''}>
            {segments.length === 0 ? emptyState : useVirtualization ? (
                // Virtualized rendering for large lists
                <>
                    <div
                        style={{
                            height: virtualizer.getTotalSize(),
                            width: "100%",
                            position: "relative",
                        }}
                    >
                        {virtualizer.getVirtualItems().map((virtualRow) => {
                            const segment = segments[virtualRow.index];
                            const isStreaming = streamingSegmentId === segment.id;

                            return (
                                <div
                                    key={segment.id}
                                    data-index={virtualRow.index}
                                    ref={virtualizer.measureElement}
                                    style={{
                                        position: "absolute",
                                        top: 0,
                                        left: 0,
                                        width: "100%",
                                        transform: `translateY(${virtualRow.start}px)`,
                                    }}
                                >
                                    <TranscriptSegment
                                        id={segment.id}
                                        timestamp={segment.timestamp}
                                        text={getDisplayText(segment)}
                                        confidence={segment.confidence}
                                        speaker={segment.speaker}
                                        isStreaming={isStreaming}
                                        showConfidence={showConfidence}
                                        speakerDisplayName={segment.speaker && speakerMap?.[segment.speaker]?.display_name}
                                        speakerColor={segment.speaker && speakerMap?.[segment.speaker]?.color}
                                        onRenameSpeaker={onRenameSpeaker}
                                    />
                                </div>
                            );
                        })}
                    </div>

                    {/* Infinite scroll trigger and loading indicator */}
                    {(hasMore || isLoadingMore) && !isRecording && segments.length > 0 && (
                        <div ref={loadMoreTriggerRef} className="flex justify-center items-center py-4 mt-2">
                            {isLoadingMore ? (
                                <div className="flex items-center gap-2 text-gray-500">
                                    <div className="w-4 h-4 border-2 border-gray-300 border-t-gray-600 rounded-full animate-spin" />
                                    <span className="text-sm">Loading more...</span>
                                </div>
                            ) : hasMore && totalCount > 0 ? (
                                <span className="text-sm text-gray-400">
                                    Showing {loadedCount} of {totalCount} segments
                                </span>
                            ) : null}
                        </div>
                    )}

                    {/* Listening indicator when recording */}
                    {!isStopping && isRecording && !isPaused && !isProcessing && segments.length > 0 && (
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
                </>
            ) : (
                // Simple rendering for small lists (better animations)
                <>
                    <div className="space-y-1">
                        {segments.map((segment) => {
                            const isStreaming = streamingSegmentId === segment.id;

                            return (
                                <motion.div
                                    key={segment.id}
                                    initial={{ opacity: 0, y: 5 }}
                                    animate={{ opacity: 1, y: 0 }}
                                    transition={{ duration: 0.15 }}
                                >
                                    <TranscriptSegment
                                        id={segment.id}
                                        timestamp={segment.timestamp}
                                        text={getDisplayText(segment)}
                                        confidence={segment.confidence}
                                        speaker={segment.speaker}
                                        isStreaming={isStreaming}
                                        showConfidence={showConfidence}
                                        speakerDisplayName={segment.speaker && speakerMap?.[segment.speaker]?.display_name}
                                        speakerColor={segment.speaker && speakerMap?.[segment.speaker]?.color}
                                        onRenameSpeaker={onRenameSpeaker}
                                    />
                                </motion.div>
                            );
                        })}
                    </div>

                    {/* Infinite scroll trigger (for small lists that grow) */}
                    {(hasMore || isLoadingMore) && !isRecording && segments.length > 0 && (
                        <div ref={loadMoreTriggerRef} className="flex justify-center items-center py-4 mt-2">
                            {isLoadingMore ? (
                                <div className="flex items-center gap-2 text-gray-500">
                                    <div className="w-4 h-4 border-2 border-gray-300 border-t-gray-600 rounded-full animate-spin" />
                                    <span className="text-sm">Loading more...</span>
                                </div>
                            ) : hasMore && totalCount > 0 ? (
                                <span className="text-sm text-gray-400">
                                    Showing {loadedCount} of {totalCount} segments
                                </span>
                            ) : null}
                        </div>
                    )}

                    {/* Listening indicator when recording */}
                    {!isStopping && isRecording && !isPaused && !isProcessing && segments.length > 0 && (
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
                </>
            )}
            </div>
        </div>
    );
};
