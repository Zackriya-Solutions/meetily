"use client";

import { Transcript, TranscriptSegmentData } from '@/types';
import { VirtualizedTranscriptView } from '@/components/VirtualizedTranscriptView';
import { TranscriptButtonGroup } from './TranscriptButtonGroup';
import { SpeakerManager } from './SpeakerManager';
import { useMemo, useState } from 'react';
import { ChevronDown, ChevronRight, ChevronLeft, Users, MessageSquarePlus } from 'lucide-react';
import { useSpeakers } from '@/contexts/SpeakerContext';

interface TranscriptPanelProps {
  transcripts: Transcript[];
  customPrompt: string;
  onPromptChange: (value: string) => void;
  onCopyTranscript: () => void;
  isRecording: boolean;
  disableAutoScroll?: boolean;
  speakerMap?: Record<string, { display_name: string; color: string }>;

  // Optional pagination props (when using virtualization)
  usePagination?: boolean;
  segments?: TranscriptSegmentData[];
  hasMore?: boolean;
  isLoadingMore?: boolean;
  totalCount?: number;
  loadedCount?: number;
  onLoadMore?: () => void;

  // Retranscription props
  meetingId?: string;
  meetingFolderPath?: string | null;
  onRefetchTranscripts?: () => Promise<void>;
  onSpeakerUpdated?: () => void;
  onCollapse?: () => void;
}

export function TranscriptPanel({
  transcripts,
  customPrompt,
  onPromptChange,
  onCopyTranscript,
  isRecording,
  disableAutoScroll = false,
  speakerMap,
  usePagination = false,
  segments,
  hasMore,
  isLoadingMore,
  totalCount,
  loadedCount,
  onLoadMore,
  meetingId,
  meetingFolderPath,
  onRefetchTranscripts,
  onSpeakerUpdated,
  onCollapse,
}: TranscriptPanelProps) {
  const [speakerSectionOpen, setSpeakerSectionOpen] = useState(false);
  const [contextSectionOpen, setContextSectionOpen] = useState(false);
  const { speakerMap: contextSpeakerMap, renameSpeaker } = useSpeakers();
  // Prefer the shared context speaker map (always up to date after renames)
  const activeSpeakerMap = contextSpeakerMap;

  // Convert transcripts to segments if pagination is not used but we want virtualization
  const convertedSegments = useMemo(() => {
    if (usePagination && segments) {
      return segments;
    }
    // Convert transcripts to segments for virtualization
    return transcripts.map(t => ({
      id: t.id,
      timestamp: t.audio_start_time ?? 0,
      endTime: t.audio_end_time,
      text: t.text,
      confidence: t.confidence,
      speaker: t.speaker,
    }));
  }, [transcripts, usePagination, segments]);

  return (
    <div className="hidden md:flex w-[303px] min-w-[303px] border-r border-gray-200 bg-background flex-col relative shrink-0">
      {/* Title area */}
      <div className="px-4 py-3 border-b border-gray-200 relative flex items-center justify-center">
        <TranscriptButtonGroup
          transcriptCount={usePagination ? (totalCount ?? convertedSegments.length) : (transcripts?.length || 0)}
          onCopyTranscript={onCopyTranscript}
          meetingId={meetingId}
          meetingFolderPath={meetingFolderPath}
          onRefetchTranscripts={onRefetchTranscripts}
        />
        {onCollapse && convertedSegments.length > 0 && (
          <button
            onClick={onCollapse}
            title="Collapse transcript"
            className="absolute right-3 p-1 rounded hover:bg-gray-100 text-gray-400 hover:text-gray-600 transition-colors"
          >
            <ChevronLeft className="w-4 h-4" />
          </button>
        )}
      </div>

      {/* Transcript content - use virtualized view for better performance */}
      <div className="flex-1 overflow-hidden pb-4">
        <VirtualizedTranscriptView
          segments={convertedSegments}
          isRecording={isRecording}
          isPaused={false}
          isProcessing={false}
          isStopping={false}
          enableStreaming={false}
          showConfidence={true}
          disableAutoScroll={disableAutoScroll}
          speakerMap={activeSpeakerMap}
          onRenameSpeaker={meetingId ? renameSpeaker : undefined}
          hasMore={hasMore}
          isLoadingMore={isLoadingMore}
          totalCount={totalCount}
          loadedCount={loadedCount}
          onLoadMore={onLoadMore}
        />
      </div>

      {/* Speaker manager section */}
      {!isRecording && meetingId && (
        <div className="border-t border-gray-200">
          <button
            className="w-full flex items-center justify-between px-4 py-2 text-xs font-medium text-gray-600 hover:bg-gray-50 transition-colors"
            onClick={() => setSpeakerSectionOpen(open => !open)}
          >
            <span className="flex items-center gap-1.5">
              <Users className="w-3 h-3" />
              Speakers
            </span>
            {speakerSectionOpen ? <ChevronDown className="w-3 h-3" /> : <ChevronRight className="w-3 h-3" />}
          </button>
          {speakerSectionOpen && (
            <div className="px-4 pb-4 max-h-64 overflow-y-auto">
              <SpeakerManager meetingId={meetingId} onSpeakerUpdated={onSpeakerUpdated} />
            </div>
          )}
        </div>
      )}

      {/* Add context section */}
      {!isRecording && convertedSegments.length > 0 && (
        <div className="border-t border-gray-200">
          <button
            className="w-full flex items-center justify-between px-4 py-2 text-xs font-medium text-gray-600 hover:bg-gray-50 transition-colors"
            onClick={() => setContextSectionOpen(open => !open)}
          >
            <span className="flex items-center gap-1.5">
              <MessageSquarePlus className="w-3 h-3" />
              Add Context
            </span>
            {contextSectionOpen ? <ChevronDown className="w-3 h-3" /> : <ChevronRight className="w-3 h-3" />}
          </button>
          {contextSectionOpen && (
            <div className="px-3 pb-3">
              <textarea
                placeholder="People involved, meeting objective, background info…"
                className="w-full px-3 py-2 border border-gray-200 rounded-md text-sm focus:outline-none focus:ring-1 focus:ring-blue-500 focus:border-blue-500 bg-white shadow-sm min-h-[80px] resize-y"
                value={customPrompt}
                onChange={(e) => onPromptChange(e.target.value)}
              />
            </div>
          )}
        </div>
      )}
    </div>
  );
}
